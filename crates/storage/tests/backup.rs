//! Backup snapshots of the engine pair and retention dry runs.

use std::time::Duration;

use v1per_storage::{
    BatchId, Budgets, Heartbeat, MonitorId, Positions, ProbeId, Retention, Status, Storage, inspect,
};

const NOW_MS: i64 = 1_790_812_800_000; // 2026-10-01T00:00:00Z
const DAY_MS: i64 = 86_400_000;

fn budgets(transfer_delay: Duration) -> Budgets {
    Budgets {
        transfer_delay,
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

fn beat(monitor: i64, time_ms: i64) -> Heartbeat {
    Heartbeat {
        monitor: MonitorId(monitor),
        probe: Some(ProbeId(7)),
        time_ms,
        status: Status::Up,
        latency_ms: Some(5),
        message: "200 - OK".into(),
        retries: 0,
        http_status: Some(200),
        cert_expiry_ms: None,
    }
}

fn batch(id: &str) -> Option<BatchId> {
    Some(BatchId {
        probe: ProbeId(7),
        id: id.into(),
    })
}

async fn count(storage: &Storage, sql: &'static str) -> i64 {
    storage
        .read(move |s| s.connection().query_row(sql, [], |r| r.get(0)))
        .await
        .unwrap()
        .value
}

/// A snapshot taken while results wait in the inbox restores to every heartbeat exactly once,
/// and an agent resending a batch admitted before the snapshot is recognized.
#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_restores_every_heartbeat_once() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets(Duration::from_secs(2)))
        .await
        .unwrap();
    let applied = storage
        .admit(batch("a"), &[beat(1, NOW_MS), beat(2, NOW_MS)])
        .await
        .unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= applied.sequence)
        .await
        .unwrap();
    // Admitted, but transfer waits two seconds: still in the inbox at the snapshot.
    storage
        .admit(
            batch("b"),
            &[
                beat(1, NOW_MS + 1),
                beat(2, NOW_MS + 1),
                beat(3, NOW_MS + 1),
            ],
        )
        .await
        .unwrap();
    let copy = tempfile::tempdir().unwrap();
    storage.snapshot(copy.path()).await.unwrap();
    storage.close().await.unwrap();

    let positions = inspect(copy.path()).await.unwrap();
    assert_eq!(
        positions,
        Positions {
            applied_batch: 1,
            applied_sequence: applied.sequence,
            applied_records: 1,
            pending_records: 1,
        }
    );

    let restored = Storage::open(copy.path(), budgets(Duration::ZERO))
        .await
        .unwrap();
    restored.drain().await;
    assert_eq!(count(&restored, "SELECT count(*) FROM heartbeat").await, 5);
    let resent = restored
        .admit(
            batch("b"),
            &[
                beat(1, NOW_MS + 1),
                beat(2, NOW_MS + 1),
                beat(3, NOW_MS + 1),
            ],
        )
        .await
        .unwrap();
    assert!(resent.repeated);
    restored.drain().await;
    assert_eq!(count(&restored, "SELECT count(*) FROM heartbeat").await, 5);
    restored.close().await.unwrap();

    let after = inspect(copy.path()).await.unwrap();
    assert_eq!(after.applied_records, 2);
    assert_eq!(after.pending_records, 0);
}

/// The dry run counts what retention deletes, and deletes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_dry_run_counts_what_retention_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets(Duration::ZERO))
        .await
        .unwrap();
    let admitted = storage
        .admit(
            batch("old"),
            &[
                beat(1, NOW_MS - 95 * DAY_MS),
                beat(1, NOW_MS - 93 * DAY_MS),
                beat(1, NOW_MS - 40 * DAY_MS),
                beat(1, NOW_MS - DAY_MS),
            ],
        )
        .await
        .unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= admitted.sequence)
        .await
        .unwrap();
    let retention = Retention {
        minute_rollups: Some(Duration::from_secs(30 * 86_400)),
        ..Retention::default()
    };

    let planned = storage.expirable(retention, NOW_MS).await.unwrap();
    let keep = Duration::from_secs(3600);
    let later = wall_clock_ms() + 2 * 3_600_000;
    assert_eq!(
        storage.prunable_agent_batches(keep, later).await.unwrap(),
        1
    );
    assert_eq!(count(&storage, "SELECT count(*) FROM heartbeat").await, 4);

    let done = storage.expire(retention, NOW_MS).await.unwrap();
    assert_eq!(planned, done);
    assert_eq!((done.heartbeats, done.minute_rollups), (2, 3));
    assert_eq!(storage.prune_agent_batches(keep, later).await.unwrap(), 1);
    assert_eq!(
        storage.expirable(retention, NOW_MS).await.unwrap(),
        Default::default()
    );
    storage.close().await.unwrap();
}

fn wall_clock_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
