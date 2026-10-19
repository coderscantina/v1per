//! A full disk during a backup or retention fails that run only: the server keeps storing
//! results, nothing half-written stays, and the next run finishes the work. The disk-full
//! failpoints are process-wide, so these tests have their own binary.

mod common;

use common::Harness;
use tempfile::TempDir;
use v1per_server::db::now_ms;
use v1per_server::maintenance::backup::{self, Source};
use v1per_server::maintenance::retention;
use v1per_storage::{Heartbeat, MonitorId, Status, fail_disk};

const DAY_MS: i64 = 86_400_000;

fn beat(time_ms: i64) -> Heartbeat {
    Heartbeat {
        monitor: MonitorId(1),
        probe: None,
        time_ms,
        status: Status::Up,
        latency_ms: Some(10),
        message: String::new(),
        retries: 0,
        http_status: None,
        cert_expiry_ms: None,
    }
}

async fn admit_visible(harness: &Harness, heartbeats: &[Heartbeat]) {
    let storage = &harness.state.storage;
    let admitted = storage.admit(None, heartbeats).await.unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= admitted.sequence)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_on_a_full_disk_fails_cleanly_and_the_next_one_works() {
    let harness = Harness::new().await;
    admit_visible(&harness, &[beat(now_ms())]).await;
    let out = TempDir::new().unwrap();
    let archive = out.path().join("v1per.tar");
    let take = || {
        backup::take(
            Source::Running(&harness.state),
            &harness.state.secrets,
            &archive,
            false,
        )
    };

    fail_disk("backup.copy", 1);
    assert_eq!(take().await.unwrap_err().code, "disk_full");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    // The pause ended with the failure: results still reach DuckDB.
    admit_visible(&harness, &[beat(now_ms())]).await;

    let summary = take().await.unwrap();
    assert_eq!(summary.positions.applied_records, 2);
    backup::verify(&archive).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_on_a_full_disk_is_finished_by_the_next_run() {
    let harness = Harness::new().await;
    let now = now_ms();
    let old: Vec<_> = (91..94).map(|days| beat(now - days * DAY_MS)).collect();
    admit_visible(&harness, &old).await;
    admit_visible(&harness, &[beat(now)]).await;

    fail_disk("retention.step", 2);
    let failed = retention::run(&harness.state, false).await.unwrap_err();
    assert_eq!(failed.code, "disk_full");
    let planned = retention::run(&harness.state, true).await.unwrap();
    assert!((1..3).contains(&planned.heartbeats), "{planned:?}");
    let done = retention::run(&harness.state, false).await.unwrap();
    assert_eq!(done.heartbeats, planned.heartbeats);
    assert_eq!(
        retention::run(&harness.state, true)
            .await
            .unwrap()
            .heartbeats,
        0
    );
}
