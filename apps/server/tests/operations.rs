//! Operations and recovery (milestone 11): backup while running and restore into a clean
//! directory, restore refusals, retention dry runs, diagnostics and `instance.operations`.

mod common;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::Harness;
use common::fakes::Discord;
use serde_json::{Value, json};
use tempfile::TempDir;
use v1per_server::config::SecretsArgs;
use v1per_server::db::now_ms;
use v1per_server::identity::secrets::{SECRETS_KEY_FILE, SecretKey};
use v1per_server::lock::DataLock;
use v1per_server::maintenance::backup::{self, Source};
use v1per_server::maintenance::commands::{self, DiagnosticsArgs, RetentionArgs};
use v1per_server::maintenance::{restore, retention};
use v1per_storage::{BatchId, Heartbeat, MonitorId, ProbeId, Status, Storage};

const DAY_MS: i64 = 86_400_000;

async fn eventually<F, Fut>(what: &str, holds: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if holds().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

fn beats(monitor: i64, time_ms: i64) -> Vec<Heartbeat> {
    (0..3)
        .map(|n| Heartbeat {
            monitor: MonitorId(monitor),
            probe: Some(ProbeId(1)),
            time_ms: time_ms + n,
            status: Status::Up,
            latency_ms: Some(10),
            message: "200 - OK".into(),
            retries: 0,
            http_status: Some(200),
            cert_expiry_ms: None,
        })
        .collect()
}

fn batch(id: &str) -> Option<BatchId> {
    Some(BatchId {
        probe: ProbeId(1),
        id: id.into(),
    })
}

async fn raw_rows(storage: &Storage) -> i64 {
    storage
        .read(|s| {
            s.connection()
                .query_row("SELECT count(*) FROM heartbeat", [], |r| r.get(0))
        })
        .await
        .unwrap()
        .value
}

fn key_of(dir: &Path) -> SecretKey {
    SecretKey::load(dir, None, None).unwrap()
}

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

/// A backup taken while results keep arriving restores on a clean directory: every heartbeat
/// the backup accepted is there once, an agent resending a batch the backup holds is
/// recognized, the pending delivery goes out with the restored key and the page is there.
#[tokio::test(flavor = "multi_thread")]
async fn a_backup_while_running_restores_without_duplicates() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let hook = Discord::start().await;
    let settings = json!({ "type": "discord", "webhook_url": { "value": hook.url } });
    let channel = harness
        .ok(
            &admin,
            "channels.create",
            json!({ "node": root, "name": "chat", "settings": settings }),
        )
        .await["channel"]["id"]
        .clone();
    harness
        .ok(
            &admin,
            "channels.test",
            json!({ "node": root, "channel": channel, "name": "Restored", "settings": { "type": "discord", "webhook_url": {} } }),
        )
        .await;
    harness
        .ok(
            &admin,
            "pages.create",
            json!({ "node": root, "slug": "status", "title": "Status" }),
        )
        .await;

    let storage = harness.state.storage.clone();
    let first = ("before".to_owned(), beats(1, now_ms()));
    let before = storage.admit(batch(&first.0), &first.1).await.unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= before.sequence)
        .await
        .unwrap();
    // Probe results keep arriving while the backup runs.
    let running = Arc::new(AtomicBool::new(true));
    let sender = tokio::spawn({
        let (storage, running) = (storage.clone(), running.clone());
        async move {
            let mut sent = Vec::new();
            while running.load(Ordering::SeqCst) {
                let id = format!("live-{}", sent.len());
                let heartbeats = beats(2, now_ms());
                storage.admit(batch(&id), &heartbeats).await.unwrap();
                sent.push((id, heartbeats));
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            sent
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let out = TempDir::new().unwrap();
    let archive = out.path().join("v1per.tar");
    let summary = backup::take(
        Source::Running(&harness.state),
        &harness.state.secrets,
        &archive,
        false,
    )
    .await
    .unwrap();
    running.store(false, Ordering::SeqCst);
    let sent = sender.await.unwrap();
    assert!(!summary.key_included);
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        1,
        "no staging files stay behind"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // A clean machine: a new directory and the key file the instance kept.
    let restored = TempDir::new().unwrap();
    let done = restore::restore(&archive, restored.path(), Some(key_of(harness.data_dir())))
        .await
        .unwrap();
    assert_eq!(done.state, "ready");
    assert_eq!(
        done.records_applied,
        Some(summary.positions.pending_records)
    );
    assert!(restored.path().join(SECRETS_KEY_FILE).exists());
    drop(harness);

    let harness = Harness::over(restored).await;
    let storage = &harness.state.storage;
    storage.drain().await;
    let count = raw_rows(storage).await;
    let mut held = 0;
    for (id, heartbeats) in std::iter::once(first).chain(sent) {
        if storage
            .admit(batch(&id), &heartbeats)
            .await
            .unwrap()
            .repeated
        {
            held += 1;
        }
    }
    assert!(held >= 2, "the backup holds batches sent during it");
    assert_eq!(count, 3 * held, "every held batch once, none twice");

    let background = harness.start();
    eventually("the pending delivery", || async { hook.posts().len() == 1 }).await;
    background.stop().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let pages = harness
        .query(Some(&admin), "pages.list", &format!("node={root}"))
        .await
        .json();
    assert_eq!(pages["pages"][0]["slug"], "status");
}

/// Restore refuses a backup without its key or with another key, a used directory and a
/// damaged archive, and leaves the directory as it was. A backup holding its key needs none.
#[tokio::test(flavor = "multi_thread")]
async fn restore_refuses_what_it_cannot_restore_whole() {
    let harness = Harness::new().await;
    let out = TempDir::new().unwrap();
    let take = |name: &str, include_key: bool| {
        let path = out.path().join(name);
        let harness = &harness;
        async move {
            backup::take(
                Source::Running(&harness.state),
                &harness.state.secrets,
                &path,
                include_key,
            )
            .await
            .unwrap();
            path
        }
    };
    let plain = take("plain.tar", false).await;
    let with_key = take("with-key.tar", true).await;
    let again = backup::take(
        Source::Running(&harness.state),
        &harness.state.secrets,
        &plain,
        false,
    )
    .await;
    assert_eq!(again.unwrap_err().code, "backup_exists");

    let code = |from: PathBuf, dir: PathBuf, key: Option<SecretKey>| async move {
        restore::restore(&from, &dir, key).await.unwrap_err().code
    };
    let target = TempDir::new().unwrap();
    let dir = target.path().to_path_buf();
    assert_eq!(
        code(plain.clone(), dir.clone(), None).await,
        "secrets_key_required"
    );
    let other = TempDir::new().unwrap();
    let foreign = SecretKey::load_or_create(other.path(), None, None).unwrap();
    assert_eq!(
        code(plain.clone(), dir.clone(), Some(foreign)).await,
        "secrets_key_mismatch"
    );
    assert!(is_empty(&dir));

    let mut bytes = std::fs::read(&plain).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xff;
    let damaged = out.path().join("damaged.tar");
    std::fs::write(&damaged, bytes).unwrap();
    assert_eq!(
        code(damaged, dir.clone(), Some(key_of(harness.data_dir()))).await,
        "backup_corrupt"
    );
    assert!(is_empty(&dir), "nothing of a damaged backup stays");

    let restored = restore::restore(&with_key, &dir, None).await.unwrap();
    assert_eq!(restored.state, "ready");
    assert_eq!(
        code(plain, dir, Some(key_of(harness.data_dir()))).await,
        "already_initialized"
    );
    let used = TempDir::new().unwrap();
    std::fs::write(used.path().join("notes.txt"), "mine").unwrap();
    assert_eq!(
        code(with_key, used.path().to_path_buf(), None).await,
        "directory_not_empty"
    );
}

/// A dry run counts what retention removes and removes nothing; the real run removes the
/// same, and a second dry run finds nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_retention_dry_run_counts_what_the_run_removes() {
    let harness = Harness::new().await;
    let state = &harness.state;
    let now = now_ms();
    let old = state
        .storage
        .admit(None, &beats(1, now - 95 * DAY_MS))
        .await
        .unwrap();
    state
        .storage
        .watermark()
        .wait_for(|w| w.sequence >= old.sequence)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, created_at, expires_at, last_seen_at)
         VALUES ('ended', ?1, ?2, ?2, ?2), ('live', ?1, ?2, ?3, ?4)",
    )
    .bind(harness.admin.as_str())
    .bind(now - 2 * DAY_MS)
    .bind(now + DAY_MS)
    .bind(now)
    .execute(&state.db.writer)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notification_outbox (node_id, event, payload, summary, state, created_at, finished_at)
         SELECT id, 'test', '{}', '', 'delivered', ?1, ?1 FROM nodes WHERE parent_id IS NULL",
    )
    .bind(now - 31 * DAY_MS)
    .execute(&state.db.writer)
    .await
    .unwrap();

    let planned = retention::run(state, true).await.unwrap();
    assert_eq!(
        (planned.heartbeats, planned.sessions, planned.deliveries),
        (3, 1, 1)
    );
    assert_eq!(raw_rows(&state.storage).await, 3);
    let done = retention::run(state, false).await.unwrap();
    assert_eq!(done, planned);
    assert_eq!(raw_rows(&state.storage).await, 0);
    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE id = 'live'")
        .fetch_one(&state.db.reader)
        .await
        .unwrap();
    assert_eq!(live, 1);
    assert_eq!(
        retention::run(state, true).await.unwrap(),
        Default::default()
    );
}

/// With a server holding the data directory, the commands queue their job for it. The
/// diagnostics JSON and `instance.operations` carry the same state, the last retention run
/// included; people without instance rights are refused.
#[tokio::test(flavor = "multi_thread")]
async fn commands_run_on_the_server_and_diagnostics_report_its_state() {
    let harness = Harness::new().await;
    let _serving = DataLock::acquire(harness.data_dir()).unwrap();
    std::fs::write(harness.data_dir().join("v1per.initialized"), b"").unwrap();
    let background = harness.start();
    let secrets = || SecretsArgs {
        secrets_key_file: None,
    };
    let data_dir = harness.data_dir().to_path_buf();

    let dry = commands::retention(
        RetentionArgs {
            data_dir: data_dir.clone(),
            dry_run: true,
            secrets: secrets(),
        },
        None,
    )
    .await
    .unwrap();
    assert!(
        dry.starts_with("dry run, would remove: 0 heartbeats"),
        "{dry}"
    );
    let run = commands::retention(
        RetentionArgs {
            data_dir: data_dir.clone(),
            dry_run: false,
            secrets: secrets(),
        },
        None,
    )
    .await
    .unwrap();
    assert!(run.starts_with("removed: "), "{run}");

    let json = commands::diagnostics(
        DiagnosticsArgs {
            data_dir: data_dir.clone(),
            json: true,
            secrets: secrets(),
        },
        None,
    )
    .await
    .unwrap();
    let report: Value = serde_json::from_str(&json).unwrap();
    let mut keys: Vec<&str> = report
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "generated_at",
            "heartbeats",
            "imports",
            "inbox",
            "jobs",
            "outbox",
            "probes",
            "retention",
            "schema",
            "storage",
            "version"
        ]
    );
    assert_eq!(report["retention"]["raw_days"], 90);
    assert!(report["retention"].get("minute_rollup_days").is_none());
    assert!(report["retention"]["last_run"]["finished_at"].is_i64());
    assert!(
        report["storage"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["name"] == "control.sqlite")
    );
    assert_eq!(report["probes"][0]["builtin"], true);
    let kinds: Vec<&str> = report["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["diagnostics", "retention", "retention_dry_run"]);

    let text = commands::diagnostics(
        DiagnosticsArgs {
            data_dir,
            json: false,
            secrets: secrets(),
        },
        None,
    )
    .await
    .unwrap();
    assert!(text.contains("\nDisk: "), "{text}");
    assert!(text.contains("Retention: raw heartbeats 90 days, minute rollups forever"));

    let admin = harness.session(&harness.admin).await;
    let operations = harness
        .query(Some(&admin), "instance.operations", "")
        .await
        .json();
    assert_eq!(operations["retention"], report["retention"]);
    assert_eq!(operations["schema"], report["schema"]);
    let member = harness.session(&harness.member).await;
    let refused = harness
        .query(Some(&member), "instance.operations", "")
        .await;
    assert_eq!(refused.code(), "forbidden");
    background.stop().await;
}
