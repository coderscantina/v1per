//! The Kuma import (milestone 10) on the synthetic Kuma 2.5.0 fixture: a full import
//! reconciles with uptime matching Kuma, the imported login, status page and push URL work,
//! an import killed in every step resumes without duplicates, and what must be refused is.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};
use tempfile::TempDir;
use v1per_kuma_fixture::{STATUS_PAGE_PASSWORD, USER_PASSWORD, USER_TOTP_SECRET};
use v1per_protocol::operations::imports::{ImportManifest, ImportedPassword};
use v1per_server::config::SecretsArgs;
use v1per_server::db::now_ms;
use v1per_server::identity::totp;
use v1per_server::imports::{self, KumaArgs, Options, Outcome};
use v1per_server::init::{self, InitArgs};
use v1per_server::lock::DataLock;

use common::{Answer, HOST, Harness, ORIGIN, PASSWORD};

const BIN: &str = env!("CARGO_BIN_EXE_v1per");
const PUSH_TOKEN: &str = "fixturePushToken0000000000000001";
const LOGIN: &str = "fixture-admin@kuma.invalid";
const MONITORS: u64 = 49;
const INVENTORY: usize = 56;

/// One generated fixture for the whole test binary; imports only read it.
fn fixture() -> &'static Path {
    static FIXTURE: OnceLock<(TempDir, PathBuf)> = OnceLock::new();
    let (_, path) = FIXTURE.get_or_init(|| {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("kuma.db");
        v1per_kuma_fixture::generate(&path).unwrap();
        (dir, path)
    });
    path
}

/// A raw window reaching 30 days into the fixture's history, whenever the test runs.
fn raw_days() -> u32 {
    let behind = (now_ms() - v1per_kuma_fixture::now().timestamp_millis()) / 86_400_000;
    behind as u32 + 30
}

/// A data directory `v1per init` made for `admin@example.test`.
async fn data_dir() -> TempDir {
    let data = TempDir::new().unwrap();
    init::init(InitArgs {
        data_dir: data.path().to_path_buf(),
        admin_email: "admin@example.test".into(),
        admin_password: Some(PASSWORD.into()),
        secrets_key: None,
        secrets_key_file: None,
    })
    .await
    .unwrap();
    data
}

fn args(data: &Path, into: &str, dry_run: bool) -> KumaArgs {
    KumaArgs {
        data_dir: data.to_path_buf(),
        source: fixture().to_path_buf(),
        into: into.to_owned(),
        dry_run,
        options: Options {
            login_domain: "kuma.invalid".into(),
            raw_days: raw_days(),
            chunk_rows: 5000,
        },
        secrets: SecretsArgs {
            secrets_key_file: None,
        },
    }
}

async fn import(data: &Path) -> ImportManifest {
    match imports::run(args(data, "Root", false), None).await.unwrap() {
        Outcome::Imported { manifest, .. } => *manifest,
        Outcome::DryRun(_) => unreachable!("not a dry run"),
    }
}

fn sha256(path: &Path) -> String {
    let digest = Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Everything the manifest compares, without times.
fn counts(manifest: &ImportManifest) -> Value {
    json!({
        "reconciled": manifest.reconciled,
        "tables": manifest.tables,
        "history": manifest.history,
        "uptime": manifest.uptime.per_monitor,
        "logins": manifest.logins,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fixture_import_reconciles_with_kuma_and_leaves_the_source_unchanged() {
    let before = sha256(fixture());
    let data = data_dir().await;
    let manifest = import(data.path()).await;

    assert!(manifest.reconciled, "{:#}", counts(&manifest));
    assert_eq!(manifest.tables.len(), 22);
    assert_eq!((manifest.nodes.expected, manifest.nodes.created), (6, 6));
    let heartbeats = manifest
        .history
        .iter()
        .find(|h| h.table == "heartbeat")
        .unwrap();
    assert!(heartbeats.imported > 10_000, "{heartbeats:?}");
    assert!(heartbeats.outside_window > 0, "{heartbeats:?}");
    assert_eq!(manifest.uptime.monitors, MONITORS);
    assert_eq!(manifest.uptime.matched, MONITORS);
    // The comparison covers outages, not only monitors that were always up.
    assert!(manifest.uptime.per_monitor.iter().any(|m| {
        m.windows
            .iter()
            .all(|w| w.kuma.down > 0 && w.kuma.up > 0 && w.kuma == w.v1per)
    }));
    assert_eq!(manifest.logins.len(), 1);
    assert_eq!(manifest.logins[0].email, LOGIN);
    assert!(manifest.logins[0].instance_admin && manifest.logins[0].two_factor);
    assert_eq!(manifest.logins[0].password, ImportedPassword::Kept);

    assert_eq!(manifest.source.sha256, before);
    assert!(manifest.source.unchanged);
    assert_eq!(sha256(fixture()), before);
    let written = std::fs::read_to_string(imports::manifest_path(data.path(), manifest.node));
    assert_eq!(
        written.unwrap(),
        serde_json::to_string_pretty(&manifest).unwrap()
    );

    let again = imports::run(args(data.path(), "Root", false), None)
        .await
        .err()
        .unwrap();
    assert_eq!(again.code, "already_imported", "{}", again.message);
    let Outcome::DryRun(report) = imports::run(args(data.path(), "Root", true), None)
        .await
        .unwrap()
    else {
        unreachable!("a dry run")
    };
    assert!(
        report.contains("refused: the node already holds an import"),
        "{report}"
    );
}

/// `name=value` of the `Set-Cookie` header named `name`.
fn set_cookie(answer: &Answer, name: &str) -> String {
    answer
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok()?.split(';').next())
        .find(|pair| pair.starts_with(&format!("{name}=")))
        .unwrap_or_else(|| panic!("no {name} cookie"))
        .to_owned()
}

async fn page_get(harness: &Harness, path: &str, cookie: Option<&str>) -> Answer {
    let mut request = Request::get(path).header(header::HOST, HOST);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    harness.send(request.body(Body::empty()).unwrap()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_imported_login_status_page_and_push_url_keep_working() {
    let data = data_dir().await;
    import(data.path()).await;
    let harness = Harness::over(data).await;
    assert_state_seeded_from_kuma(&harness).await;
    let background = harness.start();

    let push = harness
        .get(&format!("/api/push/{PUSH_TOKEN}?status=up&msg=OK&ping=12"))
        .await;
    assert_eq!(push.status, StatusCode::OK);
    assert_eq!(push.json(), json!({ "ok": true }));

    // Kuma's bcrypt hash signs in once and is replaced with argon2; the authenticator follows.
    let login = harness
        .run(
            None,
            "auth.login",
            json!({ "email": LOGIN, "password": USER_PASSWORD }),
        )
        .await;
    assert_eq!(
        login.json()["status"],
        "second_factor_required",
        "{}",
        login.json()
    );
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE email = ?")
        .bind(LOGIN)
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert!(hash.starts_with("$argon2"), "{hash}");
    let code = totp::code(USER_TOTP_SECRET.as_bytes(), totp::step(now_ms()));
    let verified = harness
        .run(
            Some(&set_cookie(&login, "v1per_challenge")),
            "auth.verifySecondFactor",
            json!({ "kind": "totp", "code": code }),
        )
        .await;
    assert_eq!(
        verified.json()["status"],
        "signed_in",
        "{}",
        verified.json()
    );
    assert_eq!(verified.json()["account"]["instance_admin"], true);
    let session = set_cookie(&verified, "v1per_session");
    let again = harness
        .run(
            None,
            "auth.login",
            json!({ "email": LOGIN, "password": USER_PASSWORD }),
        )
        .await;
    assert_eq!(again.json()["status"], "second_factor_required");

    // The Kuma page password opens the imported page.
    let gate = page_get(&harness, "/s/client-acme/snapshot.json", None).await;
    assert_eq!(gate.code(), "unauthenticated");
    let opened = harness
        .send(
            Request::post("/s/client-acme/access/password")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "password": STATUS_PAGE_PASSWORD }).to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(opened.status, StatusCode::NO_CONTENT);
    let cookie = opened
        .header("set-cookie")
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let snapshot = page_get(&harness, "/s/client-acme/snapshot.json", Some(&cookie)).await;
    assert_eq!(snapshot.status, StatusCode::OK, "{}", snapshot.json());
    assert!(!snapshot.json()["groups"].as_array().unwrap().is_empty());
    let public = page_get(&harness, "/s/status", None).await;
    assert_eq!(public.status, StatusCode::OK);

    // The import and its inventory, for instance admins only. Source values stay sealed.
    let root = harness.root().await;
    let run = harness
        .query(Some(&session), "imports.get", &format!("node={root}"))
        .await;
    assert_eq!(run.json()["state"], "complete", "{}", run.json());
    assert_eq!(run.json()["manifest"]["reconciled"], true);
    let inventory = harness
        .query(
            Some(&session),
            "imports.inventory",
            &format!("node={root}&limit=500"),
        )
        .await
        .json();
    let items = inventory["items"].as_array().unwrap();
    assert_eq!(items.len(), INVENTORY);
    let proxy = items.iter().find(|i| i["kind"] == "proxy").unwrap();
    assert!(
        proxy["columns"]
            .as_array()
            .unwrap()
            .contains(&json!("password"))
    );
    assert!(!inventory.to_string().contains("fixture-proxy-password"));
    let list = harness
        .query(Some(&session), "imports.list", "")
        .await
        .json();
    assert_eq!(
        list["imports"],
        json!([{
            "node": root, "node_name": "Root", "source": fixture().display().to_string(),
            "state": "complete", "started_at": run.json()["started_at"],
            "finished_at": run.json()["finished_at"], "reconciled": true,
        }])
    );
    let member = harness.session(&harness.member).await;
    for (name, args) in [
        ("imports.get", format!("node={root}")),
        ("imports.list", String::new()),
    ] {
        let refused = harness.query(Some(&member), name, &args).await;
        assert_eq!(refused.code(), "forbidden", "{name}");
    }

    background.stop().await;
}

/// Every imported monitor with Kuma history starts from its newest Kuma heartbeat, and so do
/// its probes, so a monitor down at cutover stays down without a new alert and a push monitor
/// keeps its deadline.
async fn assert_state_seeded_from_kuma(harness: &Harness) {
    let latest = v1per_kuma_import::Source::open(fixture())
        .unwrap()
        .latest_heartbeats()
        .unwrap();
    let seeded: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT o.source_id, s.status, s.retries, s.beat_at FROM monitor_state s
         JOIN import_objects o ON o.kind = 'monitor' AND CAST(o.target_id AS INTEGER) = s.monitor_id
         ORDER BY o.source_id",
    )
    .fetch_all(&harness.state.db.reader)
    .await
    .unwrap();
    assert_eq!(seeded.len(), latest.len());
    for (kuma, status, retries, beat_at) in &seeded {
        let beat = &latest[&v1per_kuma_import::MonitorId(*kuma)].heartbeat;
        assert_eq!(
            (*status, *retries, *beat_at),
            (beat.status as i64, i64::from(beat.retries), beat.time_ms),
            "Kuma monitor {kuma}"
        );
    }
    assert!(
        seeded.iter().any(|(_, status, ..)| *status == 0),
        "a monitor down at cutover"
    );
    let (probes, assigned): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM probe_state),
                (SELECT COUNT(*) FROM monitor_probes JOIN monitor_state USING (monitor_id))",
    )
    .fetch_one(&harness.state.db.reader)
    .await
    .unwrap();
    assert_eq!(probes, assigned);
    assert!(probes > 0);
    let transitions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM monitor_transitions")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(transitions, 0);
}

/// The progress the import process committed: per step, its chunks and whether it is done.
async fn progress(data: &Path) -> Vec<(String, i64, bool)> {
    let options = SqliteConnectOptions::new()
        .filename(data.join("control.sqlite"))
        .read_only(true);
    let Ok(mut conn) = SqliteConnection::connect_with(&options).await else {
        return Vec::new();
    };
    let steps = sqlx::query_as("SELECT step, chunks, done FROM import_steps")
        .fetch_all(&mut conn)
        .await
        .unwrap_or_default();
    let started: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM import_runs)")
        .fetch_one(&mut conn)
        .await
        .unwrap_or(false);
    let _ = conn.close().await;
    let mut steps: Vec<(String, i64, bool)> = steps;
    if started {
        steps.push(("started".into(), 0, false));
    }
    steps
}

fn import_command(data: &Path) -> Command {
    let mut command = Command::new(BIN);
    command
        .args(["import", "kuma", "--into", "Root", "--chunk-rows", "200"])
        .arg("--raw-days")
        .arg(raw_days().to_string())
        .arg("--data-dir")
        .arg(data)
        .arg("--source")
        .arg(fixture())
        .env_remove("V1PER_SECRETS_KEY")
        .env("V1PER_LOG", "warn");
    command
}

#[tokio::test(flavor = "multi_thread")]
async fn an_import_killed_in_each_step_resumes_without_duplicates() {
    let data = data_dir().await;
    let path = data.path();
    // Killed once the run exists (inside the control transaction), then after a few chunks
    // of each history step. Each process resumes where the one before stopped.
    for (step, chunks) in [("started", 0), ("stats", 3), ("heartbeats", 3)] {
        let mut child = import_command(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "the import finished before it was killed in {step}"
            );
            let reached = progress(path)
                .await
                .iter()
                .any(|(name, n, done)| name == step && *n >= chunks && !done);
            if reached {
                child.kill().unwrap();
                child.wait().unwrap();
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        if step == "stats" || step == "heartbeats" {
            let interrupted = progress(path).await;
            assert!(
                interrupted
                    .iter()
                    .any(|(name, _, done)| name == step && !done),
                "{step} finished before the kill: {interrupted:?}"
            );
        }

        // The control step is one transaction: all of it or nothing.
        let options = SqliteConnectOptions::new()
            .filename(path.join("control.sqlite"))
            .read_only(true);
        let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
        let monitors: i64 = sqlx::query_scalar("SELECT count(*) FROM monitors")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        assert!(monitors == 0 || monitors == MONITORS as i64, "{monitors}");

        let serve = Command::new(BIN)
            .args(["serve", "--bind", "127.0.0.1:0", "--data-dir"])
            .arg(path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&serve.stderr);
        assert_eq!(serve.status.code(), Some(3), "{stderr}");
        assert!(stderr.contains("import_in_progress"), "{stderr}");
    }

    let finished = import_command(path).output().unwrap();
    let stdout = String::from_utf8_lossy(&finished.stdout);
    assert!(
        finished.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&finished.stderr)
    );
    let manifest: ImportManifest =
        serde_json::from_slice(&std::fs::read(path.join("import-node-1.json")).unwrap()).unwrap();
    // Reconciled means every history count equals the source: a repeated chunk would not.
    assert!(manifest.reconciled, "{:#}", counts(&manifest));
    assert_eq!(manifest.uptime.matched, MONITORS);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_import_is_refused_beside_a_server_and_into_a_node_in_use() {
    let data = data_dir().await;
    let path = data.path();

    let lock = DataLock::acquire(path).unwrap();
    let running = imports::run(args(path, "Root", false), None)
        .await
        .err()
        .unwrap();
    assert_eq!(running.code, "server_running", "{}", running.message);
    drop(lock);
    // A child another test forks shares the lock's descriptor until it execs; wait that out.
    while DataLock::try_acquire(path).unwrap().is_none() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let missing = imports::run(args(path, "Root/Nowhere", false), None)
        .await
        .err()
        .unwrap();
    assert_eq!(missing.code, "node_not_found");

    let options = SqliteConnectOptions::new().filename(path.join("control.sqlite"));
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query(
        "INSERT INTO nodes (parent_id, name, position, created_at) VALUES (1, 'Clients', 0, 0)",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
    let Outcome::DryRun(report) = imports::run(args(path, "1", true), None).await.unwrap() else {
        unreachable!("a dry run")
    };
    assert!(report.contains("refused: node 1 is not empty"), "{report}");
    let busy = imports::run(args(path, "Root", false), None)
        .await
        .err()
        .unwrap();
    assert_eq!(busy.code, "import_conflict", "{}", busy.message);
    assert!(busy.message.contains("not empty"), "{}", busy.message);
}
