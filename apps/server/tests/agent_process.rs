//! A real agent process against a real server process, both the built `v1per` binary. The
//! agent only connects out, picks up assignment changes through its long poll, keeps checking
//! and buffering while the server is killed, and after the restart every check it ran arrives
//! exactly once. A revoked agent stops with a clear message.
//!
//! Real time with 20-second intervals and a 30-second outage: it takes under a minute.

use std::fs::File;
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::OriginalUri;
use serde_json::{Value, json};
use v1per_server::db::now_ms;

const BIN: &str = env!("CARGO_BIN_EXE_v1per");
const PASSWORD: &str = "correct horse battery";

/// Kills the process when the test ends, passed or not.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve(data: &Path, port: u16, log: &Path) -> Process {
    let log = File::options().create(true).append(true).open(log).unwrap();
    let child = Command::new(BIN)
        .args(["serve", "--data-dir"])
        .arg(data)
        .args(["--bind", &format!("127.0.0.1:{port}")])
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .unwrap();
    Process(child)
}

/// A local HTTP target that records when each path was hit.
struct Target {
    base: String,
    hits: Arc<Mutex<Vec<(String, i64)>>>,
}

impl Target {
    async fn start() -> Self {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let recorded = hits.clone();
        let app = Router::new().fallback(move |uri: OriginalUri| {
            recorded
                .lock()
                .unwrap()
                .push((uri.path().to_owned(), now_ms()));
            async { "ok" }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        Self {
            base: format!("http://{address}"),
            hits,
        }
    }

    fn hits(&self, path: &str) -> Vec<i64> {
        let hits = self.hits.lock().unwrap();
        hits.iter()
            .filter(|(hit, _)| hit == path)
            .map(|(_, at)| *at)
            .collect()
    }
}

/// The admin API as the web app calls it.
struct Api {
    client: reqwest::Client,
    origin: String,
    cookie: String,
}

impl Api {
    async fn send(&self, name: &str, body: Value) -> reqwest::Response {
        let mut body = body;
        body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
        self.client
            .post(format!("{}/api/commands/{name}", self.origin))
            .header("origin", &self.origin)
            .header("x-v1per-request", "1")
            .header("cookie", &self.cookie)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap()
    }

    async fn command(&self, name: &str, body: Value) -> Value {
        let response = self.send(name, body).await;
        let status = response.status();
        let answer: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert!(status.is_success(), "{name}: {answer}");
        answer
    }

    async fn query(&self, name: &str, query: &str) -> Value {
        let response = self
            .client
            .get(format!("{}/api/queries/{name}?{query}", self.origin))
            .header("cookie", &self.cookie)
            .send()
            .await
            .unwrap();
        serde_json::from_slice(&response.bytes().await.unwrap()).unwrap()
    }
}

async fn wait_ready(origin: &str) {
    let client = reqwest::Client::new();
    for _ in 0..300 {
        if let Ok(response) = client.get(format!("{origin}/health")).send().await
            && response.status().is_success()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the server did not come up");
}

async fn login(origin: &str) -> Api {
    let api = Api {
        client: reqwest::Client::new(),
        origin: origin.to_owned(),
        cookie: String::new(),
    };
    let response = api
        .send(
            "auth.login",
            json!({ "email": "admin@example.test", "password": PASSWORD }),
        )
        .await;
    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("v1per_session="))
        .and_then(|value| value.split(';').next())
        .expect("a session cookie")
        .to_owned();
    Api { cookie, ..api }
}

/// Waits in real time until `holds` is true.
async fn eventually<F, Fut>(what: &str, within: Duration, holds: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let until = Instant::now() + within;
    while Instant::now() < until {
        if holds().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Every heartbeat time from `probe` for `monitor`, following the pages.
async fn heartbeat_times(api: &Api, monitor: &str, probe: &str) -> Vec<i64> {
    let mut times = Vec::new();
    let mut cursor = String::new();
    loop {
        let page = api
            .query(
                "monitors.heartbeats",
                &format!("monitor={monitor}&probe={probe}&limit=100{cursor}"),
            )
            .await;
        times.extend(
            page["heartbeats"]
                .as_array()
                .unwrap()
                .iter()
                .map(|beat| beat["time"].as_i64().unwrap()),
        );
        match page["next_cursor"].as_str() {
            Some(next) => cursor = format!("&cursor={next}"),
            None => return times,
        }
    }
}

/// Whether every check hit before `settled` has exactly one heartbeat, and every heartbeat a
/// check: a heartbeat records when the check started, a moment before it reached the target.
fn delivered_once(heartbeats: &[i64], hits: &[i64], settled: i64) -> bool {
    let matches = |hit: i64| {
        heartbeats
            .iter()
            .filter(|beat| (0..2_000).contains(&(hit - **beat)))
            .count()
    };
    hits.iter()
        .filter(|hit| **hit < settled)
        .all(|hit| matches(*hit) == 1)
        && heartbeats
            .iter()
            .all(|beat| hits.iter().any(|hit| (0..2_000).contains(&(hit - beat))))
}

/// Listening TCP sockets of `pid`, or `None` without `lsof`.
fn listening(pid: u32) -> Option<String> {
    let output = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:LISTEN"])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_survives_a_server_restart_without_loss_or_duplicates() {
    // The test's own client; the agent configures its TLS itself.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().unwrap();
    let (data, agent_dir) = (dir.path().join("server"), dir.path().join("agent"));
    let (server_log, agent_log) = (dir.path().join("server.log"), dir.path().join("agent.log"));
    std::fs::write(dir.path().join("password"), PASSWORD).unwrap();
    let init = Command::new(BIN)
        .args(["init", "--data-dir"])
        .arg(&data)
        .args([
            "--admin-email",
            "admin@example.test",
            "--admin-password-file",
        ])
        .arg(dir.path().join("password"))
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let origin = format!("http://127.0.0.1:{port}");
    let mut server = serve(&data, port, &server_log);
    wait_ready(&origin).await;
    let api = login(&origin).await;
    let tree = api.query("nodes.tree", "").await;
    let root = tree["nodes"][0]["id"].as_str().unwrap().to_owned();
    let created = api
        .command("probes.create", json!({ "node": root, "name": "Office" }))
        .await;
    let probe = created["probe"]["id"].as_str().unwrap().to_owned();
    let token = created["secret"].as_str().unwrap();

    let agent = Command::new(BIN)
        .args(["probe", "--server", &origin, "--data-dir"])
        .arg(&agent_dir)
        .env("V1PER_PROBE_TOKEN", token)
        .stdout(Stdio::null())
        .stderr(File::create(&agent_log).unwrap())
        .spawn()
        .unwrap();
    let mut agent = Process(agent);
    eventually("the agent to connect", Duration::from_secs(10), || async {
        let listed = api.query("probes.list", &format!("node={root}")).await;
        listed["probes"][1]["status"] == "online"
    })
    .await;
    // Outbound only: the agent listens on nothing the server could dial.
    match listening(agent.0.id()) {
        Some(sockets) => assert!(sockets.trim().is_empty(), "the agent listens: {sockets}"),
        None => eprintln!("lsof is not available; listening sockets not checked"),
    }

    // A monitor assigned while the agent waits in its long poll is checked at once.
    let target = Target::start().await;
    let mut monitors = Vec::new();
    for path in ["/one", "/two"] {
        let assigned = now_ms();
        let monitor = api
            .command(
                "monitors.create",
                json!({
                    "node": root, "name": path, "interval": 20, "probes": [probe],
                    "settings": { "type": "http", "url": format!("{}{path}", target.base) },
                }),
            )
            .await;
        eventually("the first check", Duration::from_secs(5), || async {
            !target.hits(path).is_empty()
        })
        .await;
        assert!(target.hits(path)[0] - assigned < 5_000);
        monitors.push((monitor["monitor"]["id"].as_str().unwrap().to_owned(), path));
    }

    // Killed, not stopped: nothing is flushed. The agent keeps checking meanwhile.
    tokio::time::sleep(Duration::from_secs(5)).await;
    server.0.kill().unwrap();
    server.0.wait().unwrap();
    let killed = now_ms();
    tokio::time::sleep(Duration::from_secs(30)).await;
    let server = serve(&data, port, &server_log);
    wait_ready(&origin).await;
    let restarted = now_ms();
    for (_, path) in &monitors {
        let hits = target.hits(path);
        assert!(
            hits.iter().any(|hit| (killed..restarted).contains(hit)),
            "{path} was not checked while the server was down"
        );
    }

    eventually(
        "every check to arrive exactly once",
        Duration::from_secs(60),
        || async {
            let mut all = true;
            for (monitor, path) in &monitors {
                let settled = now_ms() - 3_000;
                let heartbeats = heartbeat_times(&api, monitor, &probe).await;
                let mut distinct = heartbeats.clone();
                distinct.dedup();
                assert_eq!(distinct.len(), heartbeats.len(), "duplicate heartbeats");
                all &= delivered_once(&heartbeats, &target.hits(path), settled);
            }
            all
        },
    )
    .await;

    // Revoked mid-run: the agent's waiting poll is refused and it stops with the reason.
    api.command("probes.revoke", json!({ "probe": probe }))
        .await;
    let exited = Instant::now();
    let status = loop {
        if let Some(status) = agent.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            exited.elapsed() < Duration::from_secs(10),
            "the agent kept running"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(status.code(), Some(3));
    let log = std::fs::read_to_string(&agent_log).unwrap();
    assert!(
        log.contains("probe_refused") && log.contains("revoked"),
        "{log}"
    );
    drop(server);
}
