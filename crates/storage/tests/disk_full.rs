//! A full disk during retention or a backup snapshot fails that run only; the next one
//! finishes the work. Run with `cargo test -p v1per-storage --features failpoints`.
#![cfg(feature = "failpoints")]

use std::time::Duration;

use v1per_storage::{
    Budgets, Heartbeat, MonitorId, ProbeId, Retention, Status, Storage, fail_disk,
};

const NOW_MS: i64 = 1_790_812_800_000; // 2026-10-01T00:00:00Z
const DAY_MS: i64 = 86_400_000;

fn budgets() -> Budgets {
    Budgets {
        transfer_delay: Duration::ZERO,
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

fn beat(time_ms: i64) -> Heartbeat {
    Heartbeat {
        monitor: MonitorId(1),
        probe: Some(ProbeId(7)),
        time_ms,
        status: Status::Up,
        latency_ms: Some(5),
        message: String::new(),
        retries: 0,
        http_status: None,
        cert_expiry_ms: None,
    }
}

async fn admit_visible(storage: &Storage, heartbeats: &[Heartbeat]) {
    let admitted = storage.admit(None, heartbeats).await.unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= admitted.sequence)
        .await
        .unwrap();
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

#[tokio::test(flavor = "multi_thread")]
async fn retention_resumes_after_a_full_disk() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let old: Vec<_> = (91..96).map(|days| beat(NOW_MS - days * DAY_MS)).collect();
    admit_visible(&storage, &old).await;
    admit_visible(&storage, &[beat(NOW_MS - DAY_MS)]).await;

    // The second day's delete hits a full disk: the first day stays deleted.
    fail_disk("retention.step", 2);
    let error = storage
        .expire(Retention::default(), NOW_MS)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no space left"), "{error}");
    assert_eq!(raw_rows(&storage).await, 5);

    let rest = storage.expire(Retention::default(), NOW_MS).await.unwrap();
    assert_eq!(rest.heartbeats, 4);
    assert_eq!(raw_rows(&storage).await, 1);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_disk_during_a_snapshot_releases_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    admit_visible(&storage, &[beat(NOW_MS)]).await;
    let copy = tempfile::tempdir().unwrap();

    fail_disk("backup.copy", 1);
    let error = storage.snapshot(copy.path()).await.unwrap_err();
    assert!(error.to_string().contains("no space left"), "{error}");
    // Transfer goes on after the failed copy.
    tokio::time::timeout(
        Duration::from_secs(10),
        admit_visible(&storage, &[beat(NOW_MS + 1)]),
    )
    .await
    .expect("transfer resumed");

    storage.snapshot(copy.path()).await.unwrap();
    storage.close().await.unwrap();
    let positions = v1per_storage::inspect(copy.path()).await.unwrap();
    assert_eq!(positions.applied_records, 2);
}
