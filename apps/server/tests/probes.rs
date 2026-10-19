//! Probes in process: registration on the tree, the agent protocol spoken through the router as
//! `v1per probe` speaks it, and the quorum over live probes on a manual clock. A real agent
//! process against a real server is in `agent_process.rs`.

mod common;

use std::future::Future;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{Answer, Harness};
use serde_json::{Value, json};
use v1per_storage::{MonitorId, ProbeId, Series, UptimeWindow};

/// Waits in real time until `holds` is true.
async fn eventually<F, Fut>(what: &str, holds: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..500 {
        if holds().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Talks to `/api/probe/*` as an agent with `token` does.
struct Agent<'a> {
    harness: &'a Harness,
    token: String,
}

impl Agent<'_> {
    async fn post(&self, path: &str, protocol: &str, body: Value) -> Answer {
        let request = Request::post(format!("/api/probe/{path}"))
            .header(header::HOST, common::HOST)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .header("x-v1per-probe-protocol", protocol)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.harness.send(request).await
    }

    async fn poll(&self, version: Option<&str>) -> Answer {
        let now = self.harness.state.clock.now_ms();
        let health = json!({
            "version": "9.9.9", "checks_started": 12, "checks_late": 1,
            "buffered_results": 3, "dropped_results": 2,
        });
        self.post(
            "assignments",
            "1",
            json!({ "version": version, "sent_at": now, "health": health }),
        )
        .await
    }

    /// Sends one batch of results, each `(monitor, revision, time, status)`.
    async fn send(&self, batch: &str, sent_at: i64, results: &[(&str, i64, i64, &str)]) -> Answer {
        let results: Vec<Value> = results
            .iter()
            .map(|(monitor, revision, time, status)| {
                json!({
                    "monitor": monitor, "revision": revision, "time": time, "status": status,
                    "retries": 0, "latency_ms": 10, "message": format!("probe says {status}"),
                    "http_status": 200, "cert_expiry": null,
                })
            })
            .collect();
        self.post(
            "results",
            "1",
            json!({ "batch": batch, "sent_at": sent_at, "results": results }),
        )
        .await
    }

    /// One current result, which must be admitted.
    async fn report(&self, monitor: &str, revision: i64, status: &str) {
        let now = self.harness.state.clock.now_ms();
        let batch = format!("{}-{monitor}-{now}", &self.token[..8]);
        let answer = self
            .send(&batch, now, &[(monitor, revision, now, status)])
            .await;
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.json());
    }
}

/// Registers a probe on `node`: its ID and an agent holding its token.
async fn register<'a>(
    harness: &'a Harness,
    cookie: &str,
    node: &str,
    name: &str,
) -> (String, Agent<'a>) {
    let created = harness
        .ok(
            cookie,
            "probes.create",
            json!({ "node": node, "name": name }),
        )
        .await;
    let agent = Agent {
        harness,
        token: created["secret"].as_str().unwrap().to_owned(),
    };
    (created["probe"]["id"].as_str().unwrap().to_owned(), agent)
}

/// An http monitor on `node` checked by `probes`: its ID and revision.
async fn monitor(
    harness: &Harness,
    cookie: &str,
    node: &str,
    probes: &[&str],
    extra: Value,
) -> (String, i64) {
    let mut body = json!({
        "node": node, "name": "site", "interval": 20, "probes": probes,
        "settings": { "type": "http", "url": "http://site.test/health" },
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    let created = harness.ok(cookie, "monitors.create", body).await;
    (
        created["monitor"]["id"].as_str().unwrap().to_owned(),
        created["monitor"]["revision"].as_i64().unwrap(),
    )
}

fn ids(list: &Value) -> Vec<&str> {
    list["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|probe| probe["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn probes_serve_the_subtree_of_their_node() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let team = harness.node(&admin, &root, "team").await;
    let client = harness.node(&admin, &team, "client").await;
    let other = harness.node(&admin, &root, "other").await;
    let builtin = ids(&harness
        .query(Some(&admin), "probes.list", &format!("node={root}"))
        .await
        .json())[0]
        .to_owned();
    let (shared, _) = register(&harness, &admin, &root, "Frankfurt").await;

    // The token shows once; a replay withholds it.
    let key = json!({ "idempotency_key": "probe-1", "node": team, "name": "Team office" });
    let first = harness
        .command(Some(&admin), "probes.create", key.clone())
        .await;
    let replay = harness.command(Some(&admin), "probes.create", key).await;
    assert!(first.json()["secret"].is_string());
    assert_eq!(replay.json()["secret"], Value::Null);
    assert_eq!(replay.json()["secret_withheld"], true);
    let team_probe = first.json()["probe"]["id"].as_str().unwrap().to_owned();
    assert_eq!(first.json()["probe"]["status"], "waiting");

    let available = |node: String| {
        let (harness, admin) = (&harness, &admin);
        async move {
            harness
                .query(Some(admin), "probes.available", &format!("node={node}"))
                .await
                .json()
        }
    };
    assert_eq!(
        ids(&available(client.clone()).await),
        [&team_probe, &builtin, &shared]
    );
    assert_eq!(ids(&available(other.clone()).await), [&builtin, &shared]);
    assert_eq!(
        available(client.clone()).await["probes"][0]["node"],
        json!(team)
    );
    // Someone who reads only `client` sees the probes it may use, not where they live.
    let viewer = harness.person("viewer").await;
    harness.grant(&viewer, &client, "viewer").await;
    let session = harness.session(&viewer).await;
    let seen = harness
        .query(
            Some(&session),
            "probes.available",
            &format!("node={client}"),
        )
        .await
        .json();
    assert_eq!(seen["probes"].as_array().unwrap().len(), 3);
    assert!(
        seen["probes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|probe| probe.get("node").is_none())
    );

    // Monitors pick only what their node is offered; the default is the built-in probe.
    let refused = harness
        .run(
            Some(&admin),
            "monitors.create",
            json!({ "node": other, "name": "x", "interval": 20, "probes": [team_probe],
                    "settings": { "type": "http", "url": "http://x.test" } }),
        )
        .await;
    assert_eq!(
        refused.json()["errors"]["probes"][0]["code"],
        "invalid_value"
    );
    let pushed = harness
        .run(
            Some(&admin),
            "monitors.create",
            json!({ "node": client, "name": "cron", "interval": 60, "probes": [shared],
                    "settings": { "type": "push" } }),
        )
        .await;
    assert_eq!(
        pushed.json()["errors"]["probes"][0]["code"],
        "invalid_value"
    );
    let (default, _) = monitor(&harness, &admin, &other, &[], json!({})).await;
    let (served, _) = monitor(&harness, &admin, &client, &[&team_probe], json!({})).await;
    let probes_of = |monitor: String| {
        let (harness, admin) = (&harness, &admin);
        async move {
            harness
                .query(Some(admin), "monitors.get", &format!("monitor={monitor}"))
                .await
                .json()["monitor"]["probes"]
                .clone()
        }
    };
    assert_eq!(probes_of(default).await, json!([builtin]));
    assert_eq!(probes_of(served.clone()).await, json!([team_probe]));

    // Moving the client out of the team's subtree takes the team probe off its monitors.
    harness
        .ok(
            &admin,
            "nodes.move",
            json!({ "node": client, "parent": other }),
        )
        .await;
    assert_eq!(probes_of(served).await, json!([]));

    // Admins manage probes; members only read them. The built-in probe stays.
    let member = harness.person("member2").await;
    harness.grant(&member, &team, "member").await;
    let member = harness.session(&member).await;
    let forbidden = harness
        .run(
            Some(&member),
            "probes.create",
            json!({ "node": team, "name": "mine" }),
        )
        .await;
    assert_eq!(forbidden.code(), "forbidden");
    let listed = harness
        .query(Some(&member), "probes.list", &format!("node={team}"))
        .await;
    assert_eq!(ids(&listed.json()), [&team_probe]);
    let builtin_revoke = harness
        .run(Some(&admin), "probes.revoke", json!({ "probe": builtin }))
        .await;
    assert_eq!(
        builtin_revoke.json()["errors"]["probe"][0]["code"],
        "invalid_value"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_poll_assignments_and_deliver_each_batch_once() {
    let harness = Harness::new().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let (probe, agent) = register(&harness, &admin, &root, "Frankfurt").await;
    let secret_header = json!({ "settings": {
        "type": "http", "url": "http://site.test/health",
        "headers": [{ "name": "X-Api-Key", "secret": { "value": "key-sekrit" } }],
    }});
    let (first, revision) = monitor(&harness, &admin, &root, &[&probe], secret_header).await;

    let old = agent.post("assignments", "2", json!({})).await;
    assert_eq!(old.status, StatusCode::UPGRADE_REQUIRED);
    assert_eq!(old.json()["error"], "protocol_unsupported");
    assert!(
        old.json()["message"]
            .as_str()
            .unwrap()
            .contains("protocol 1")
    );
    let stranger = Agent {
        harness: &harness,
        token: "not-a-token".into(),
    };
    assert_eq!(stranger.poll(None).await.status, StatusCode::UNAUTHORIZED);

    // The list carries unsealed credentials to the agent, and nowhere else.
    let list = agent.poll(None).await.json();
    let version = list["version"].as_str().unwrap().to_owned();
    let assignments = list["assignments"].as_array().unwrap();
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0]["monitor"], first);
    assert_eq!(
        assignments[0]["definition"]["headers"]["X-Api-Key"],
        "key-sekrit"
    );

    // A poll at the current version waits until the list changes.
    let waiting = agent.poll(Some(&version));
    let change = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        monitor(&harness, &admin, &root, &[&probe], json!({})).await
    };
    let (answer, (second, _)) = tokio::time::timeout(
        Duration::from_secs(5),
        futures_util::future::join(waiting, change),
    )
    .await
    .expect("the poll returns with the change, not at its timeout");
    let changed = answer.json();
    assert_ne!(changed["version"], version);
    assert_eq!(changed["assignments"].as_array().unwrap().len(), 2);
    assert_eq!(changed["assignments"][1]["monitor"], second);

    let listed = harness
        .query(Some(&admin), "probes.list", &format!("node={root}"))
        .await
        .json();
    let health = &listed["probes"][1];
    assert_eq!(
        (&health["status"], &health["version"], &health["monitors"]),
        (&json!("online"), &json!("9.9.9"), &json!(2))
    );
    assert_eq!(
        (&health["buffered_results"], &health["dropped_results"]),
        (&json!(3), &json!(2))
    );

    // A batch is admitted once; a resend is a repeat.
    let now = harness.state.clock.now_ms();
    let results = [(first.as_str(), revision, now, "up")];
    let sent = agent.send("b-1", now, &results).await.json();
    assert_eq!(sent, json!({ "batch": "b-1", "repeated": false }));
    let again = agent.send("b-1", now + 5_000, &results).await.json();
    assert_eq!(again["repeated"], true);
    // A result that waited longer than an interval is history only.
    let late = agent
        .send(
            "b-2",
            now,
            &[(first.as_str(), revision, now - 100_000, "down")],
        )
        .await;
    assert_eq!(late.status, StatusCode::OK);
    let maintenance = agent
        .send(
            "b-3",
            now,
            &[(first.as_str(), revision, now, "maintenance")],
        )
        .await;
    assert_eq!(maintenance.status, StatusCode::BAD_REQUEST);

    harness.state.storage.drain().await;
    let page = harness
        .query(
            Some(&admin),
            "monitors.heartbeats",
            &format!("monitor={first}&probe={probe}"),
        )
        .await
        .json();
    let statuses: Vec<&str> = page["heartbeats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|beat| beat["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["up", "down"]);
    let bars = harness
        .query(
            Some(&admin),
            "monitors.dayBars",
            &format!("monitor={first}&probe={probe}&days=1"),
        )
        .await
        .json();
    assert_eq!(
        (
            bars["bars"][0]["up"].as_u64(),
            bars["bars"][0]["down"].as_u64()
        ),
        (Some(1), Some(1))
    );
    let detail = harness
        .query(Some(&admin), "monitors.get", &format!("monitor={first}"))
        .await
        .json();
    assert_eq!(detail["state"]["status"], "up");
    assert_eq!(detail["probes"][0]["status"], "up");
    assert_eq!(detail["probes"][0]["live"], true);
    background.stop().await;
}

/// The monitor-level state: Kuma status code, message, whether no probe is live.
async fn state(harness: &Harness, monitor: &str) -> (i64, String, bool) {
    sqlx::query_as("SELECT status, message, no_live_probe FROM monitor_state WHERE monitor_id = ?")
        .bind(monitor.parse::<i64>().unwrap())
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap()
}

/// Monitor-level series counts: (up, down, pending), after storage caught up.
async fn series(harness: &Harness, monitor: &str, series: Series) -> (u64, u64, u64) {
    harness.state.storage.drain().await;
    let id = MonitorId(monitor.parse().unwrap());
    let now = harness.state.clock.now_ms();
    let counts = harness
        .state
        .storage
        .uptime(&[id], series, UptimeWindow::Day, now)
        .await
        .unwrap()
        .value
        .remove(&id)
        .unwrap_or_default();
    (counts.up, counts.down, counts.pending)
}

/// Probes that went offline, in journal order.
async fn offline(harness: &Harness) -> Vec<i64> {
    sqlx::query_scalar("SELECT probe_id FROM probe_transitions WHERE online = 0 ORDER BY id")
        .fetch_all(&harness.state.db.reader)
        .await
        .unwrap()
}

/// Majority over three probes: a flapping probe changes nothing and the series keeps one entry
/// per round; a silent probe leaves the denominator after three intervals; with every probe
/// silent the monitor is pending and the probes go offline; a revoked probe leaves at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_monitor_follows_the_quorum_of_live_probes() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let (a, agent_a) = register(&harness, &admin, &root, "a").await;
    let (b, agent_b) = register(&harness, &admin, &root, "b").await;
    let (c, agent_c) = register(&harness, &admin, &root, "c").await;
    let (id, revision) = monitor(&harness, &admin, &root, &[&a, &b, &c], json!({})).await;
    let round = |reports: Vec<(&'static str, usize)>| {
        let agents = [&agent_a, &agent_b, &agent_c];
        let (harness, id) = (&harness, id.clone());
        async move {
            for (status, agent) in reports {
                agents[agent].report(&id, revision, status).await;
            }
            harness.state.clock.advance(Duration::from_secs(20));
        }
    };
    let (up, down, pending) = (1, 0, 2);

    // Flapping: a goes up and down while b and c stay up.
    for flap in ["up", "down", "up", "down"] {
        round(vec![(flap, 0), ("up", 1), ("up", 2)]).await;
        assert_eq!(state(&harness, &id).await.0, up);
    }
    let changes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM topic_events WHERE topic = ? AND kind = 'monitor.status_changed'",
    )
    .bind(format!("monitors:{root}"))
    .fetch_one(&harness.state.db.reader)
    .await
    .unwrap();
    assert_eq!(changes, 1);
    assert_eq!(series(&harness, &id, Series::Monitor).await, (4, 0, 0));
    let probe_a = Series::Probe(ProbeId(a.parse().unwrap()));
    assert_eq!(series(&harness, &id, probe_a).await, (2, 2, 0));

    // One offline: c falls silent, b goes down. Down needs half of the live probes, so the
    // monitor stays up until c leaves the denominator three intervals after its last result.
    for _ in 0..3 {
        round(vec![("up", 0), ("down", 1)]).await;
        assert_eq!(state(&harness, &id).await.0, up);
    }
    round(vec![("up", 0), ("down", 1)]).await;
    assert_eq!(state(&harness, &id).await.0, down);
    let c_id: i64 = c.parse().unwrap();
    eventually("c to go offline", || async {
        harness.state.clock.advance(Duration::from_secs(1));
        offline(&harness).await == [c_id]
    })
    .await;

    // All offline: without a live probe the monitor is pending once, with no series entry.
    let before = series(&harness, &id, Series::Monitor).await;
    eventually("the monitor to lose its last live probe", || async {
        harness.state.clock.advance(Duration::from_secs(5));
        state(&harness, &id).await.2
    })
    .await;
    assert_eq!(
        state(&harness, &id).await,
        (pending, "No live probe".to_owned(), true)
    );
    assert_eq!(series(&harness, &id, Series::Monitor).await, before);
    eventually("a and b to go offline", || async {
        harness.state.clock.advance(Duration::from_secs(5));
        offline(&harness).await.len() == 3
    })
    .await;

    // Revoked mid-run: a and b come back, b down makes it down, revoking b brings it up.
    agent_a.report(&id, revision, "up").await;
    assert_eq!(state(&harness, &id).await.0, up);
    agent_b.report(&id, revision, "down").await;
    assert_eq!(state(&harness, &id).await.0, down);
    harness
        .ok(&admin, "probes.revoke", json!({ "probe": b }))
        .await;
    harness.state.clock.advance(Duration::from_secs(20));
    agent_a.report(&id, revision, "up").await;
    assert_eq!(state(&harness, &id).await.0, up);
    let refused = agent_b.poll(None).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    let detail = harness
        .query(Some(&admin), "monitors.get", &format!("monitor={id}"))
        .await
        .json();
    assert_eq!(detail["monitor"]["probes"], json!([a, c]));
    let back_online: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM probe_transitions WHERE online = 1")
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap();
    assert_eq!(back_online, 2);
    background.stop().await;
}
