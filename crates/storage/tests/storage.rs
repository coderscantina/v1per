//! Admission, transfer, rollups and retention through the real SQLite and DuckDB engines.

use std::time::Duration;

use v1per_storage::{
    Admitted, BatchId, Budgets, Expired, Heartbeat, MonitorId, MonitorStatus, ProbeId, Refusal,
    Retention, Series, Status, Storage, UptimeWindow,
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

fn beat(monitor: i64, time_ms: i64, status: Status, latency_ms: Option<u32>) -> Heartbeat {
    Heartbeat {
        monitor: MonitorId(monitor),
        probe: Some(ProbeId(7)),
        time_ms,
        status,
        latency_ms,
        message: "200 - OK".into(),
        retries: 0,
        http_status: Some(200),
        cert_expiry_ms: Some(NOW_MS + 30 * DAY_MS),
    }
}

fn batch(id: &str) -> Option<BatchId> {
    Some(BatchId {
        probe: ProbeId(7),
        id: id.into(),
    })
}

/// Admits and waits until the heartbeats are visible.
async fn admit_visible(storage: &Storage, heartbeats: &[Heartbeat]) -> Admitted {
    let admitted = storage.admit(None, heartbeats).await.unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= admitted.sequence)
        .await
        .unwrap();
    admitted
}

async fn query<T: duckdb::types::FromSql + Send + 'static>(
    storage: &Storage,
    sql: &'static str,
) -> T {
    storage
        .read(move |s| s.connection().query_row(sql, [], |r| r.get(0)))
        .await
        .unwrap()
        .value
}

#[tokio::test(flavor = "multi_thread")]
async fn admitted_heartbeats_become_visible_with_a_covering_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let heartbeats = [
        beat(1, NOW_MS, Status::Up, Some(120)),
        beat(1, NOW_MS + 20_000, Status::Down, None),
        beat(2, NOW_MS, Status::Maintenance, Some(80)),
    ];
    let admitted = admit_visible(&storage, &heartbeats).await;
    assert!(!admitted.repeated);

    let latest = storage
        .latest(&[MonitorId(1), MonitorId(2)], NOW_MS)
        .await
        .unwrap();
    assert!(latest.watermark.sequence >= admitted.sequence);
    assert_eq!(
        latest.value,
        vec![heartbeats[1].clone(), heartbeats[2].clone()]
    );
    assert_eq!(
        query::<i64>(&storage, "SELECT count(*) FROM rollup_minute").await,
        2
    );
    // Retirement follows the watermark.
    storage.drain().await;
    assert_eq!(storage.health().inbox_rows, 0);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_batch_ids_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let heartbeats = [beat(1, NOW_MS, Status::Up, Some(10))];

    let first = storage.admit(batch("b-1"), &heartbeats).await.unwrap();
    let again = storage.admit(batch("b-1"), &heartbeats).await.unwrap();
    assert_eq!(
        again,
        Admitted {
            sequence: first.sequence,
            repeated: true
        }
    );
    let changed = [beat(1, NOW_MS, Status::Down, None)];
    assert_eq!(
        storage.admit(batch("b-1"), &changed).await,
        Err(Refusal::BatchConflict)
    );
    let other_probe = BatchId {
        probe: ProbeId(8),
        id: "b-1".into(),
    };
    let other = storage.admit(Some(other_probe), &heartbeats).await.unwrap();
    assert!(!other.repeated && other.sequence > first.sequence);

    // A retry after transfer retired the batch still finds it.
    storage.drain().await;
    assert_eq!(
        storage.admit(batch("b-1"), &heartbeats).await.unwrap(),
        again
    );
    assert_eq!(
        query::<i64>(&storage, "SELECT count(*) FROM heartbeat").await,
        2
    );

    // Pruned IDs are forgotten: the same batch is admitted anew.
    assert_eq!(
        storage
            .prune_agent_batches(Duration::from_secs(3600), i64::MAX / 2)
            .await
            .unwrap(),
        2
    );
    assert!(
        !storage
            .admit(batch("b-1"), &heartbeats)
            .await
            .unwrap()
            .repeated
    );
    storage.close().await.unwrap();
}

/// Heartbeats and their monitor-level statuses share one inbox row; a resent batch is a repeat
/// even when the server derives other statuses for it.
#[tokio::test(flavor = "multi_thread")]
async fn evaluated_batches_admit_heartbeats_and_statuses_together() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let heartbeats = [beat(1, NOW_MS, Status::Down, None)];
    let status = |status| MonitorStatus {
        monitor: MonitorId(1),
        time_ms: NOW_MS,
        status,
        latency_ms: None,
    };
    let id = batch("b-1").unwrap();
    assert_eq!(storage.admitted(&id).await.unwrap(), None);
    let first = storage
        .admit_evaluated(Some(id.clone()), &heartbeats, &[status(Status::Down)])
        .await
        .unwrap();
    assert_eq!(storage.admitted(&id).await.unwrap(), Some(first.sequence));
    let again = storage
        .admit_evaluated(Some(id), &heartbeats, &[status(Status::Up)])
        .await
        .unwrap();
    assert_eq!((again.sequence, again.repeated), (first.sequence, true));
    assert_eq!(storage.health().inbox_rows, 1);
    assert_eq!(storage.health().inbox_entries, 2);

    storage.drain().await;
    let counts = |series| {
        let storage = &storage;
        async move {
            storage
                .uptime(&[MonitorId(1)], series, UptimeWindow::Day, NOW_MS)
                .await
                .unwrap()
                .value[&MonitorId(1)]
        }
    };
    assert_eq!(counts(Series::Monitor).await.down, 1);
    assert_eq!(counts(Series::Probe(ProbeId(7))).await.down, 1);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn rollups_merge_across_transfer_batches() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    // Waiting for each to become visible puts them in separate transfer batches.
    admit_visible(&storage, &[beat(1, NOW_MS + 1_000, Status::Up, Some(100))]).await;
    admit_visible(
        &storage,
        &[
            beat(1, NOW_MS + 21_000, Status::Up, Some(300)),
            beat(1, NOW_MS + 41_000, Status::Pending, Some(900)),
        ],
    )
    .await;
    admit_visible(&storage, &[beat(1, NOW_MS + 61_000, Status::Down, None)]).await;

    let rows = storage
        .read(|s| {
            let mut statement = s.connection().prepare(
                "SELECT 'minute', count(*), sum(up), sum(down), sum(pending), min(latency_min), max(latency_max), sum(latency_sum), sum(latency_count) FROM rollup_minute
                 UNION ALL SELECT 'hour', count(*), sum(up), sum(down), sum(pending), min(latency_min), max(latency_max), sum(latency_sum), sum(latency_count) FROM rollup_hour
                 UNION ALL SELECT 'day', count(*), sum(up), sum(down), sum(pending), min(latency_min), max(latency_max), sum(latency_sum), sum(latency_count) FROM rollup_day
                 ORDER BY 1",
            )?;
            statement
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        [
                            r.get::<_, i64>(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                            r.get(7)?,
                            r.get(8)?,
                        ],
                    ))
                })?
                .collect::<duckdb::Result<Vec<_>>>()
        })
        .await
        .unwrap()
        .value;
    // Two minute buckets, one hour, one day; latency of up beats only.
    assert_eq!(
        rows,
        vec![
            ("day".into(), [1, 2, 1, 1, 100, 300, 400, 2]),
            ("hour".into(), [1, 2, 1, 1, 100, 300, 400, 2]),
            ("minute".into(), [2, 2, 1, 1, 100, 300, 400, 2]),
        ]
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn admission_refuses_beyond_the_inbox_budget() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(
        dir.path(),
        Budgets {
            inbox_bytes: 16,
            ..budgets()
        },
    )
    .await
    .unwrap();
    let heartbeats: Vec<Heartbeat> = (0..10)
        .map(|i| beat(1, NOW_MS + i, Status::Up, Some(1)))
        .collect();
    assert_eq!(
        storage.admit(batch("big"), &heartbeats).await,
        Err(Refusal::Busy)
    );
    assert_eq!(storage.health().inbox_rows, 0);
    // A refused batch ID was never claimed.
    assert!(storage.admit(batch("big"), &heartbeats[..0]).await.is_ok());
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_deletes_old_raw_heartbeats_and_keeps_rollups() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    admit_visible(
        &storage,
        &[
            beat(1, NOW_MS - 100 * DAY_MS, Status::Up, Some(1)),
            beat(1, NOW_MS - 91 * DAY_MS, Status::Up, Some(1)),
            beat(1, NOW_MS - 89 * DAY_MS, Status::Up, Some(1)),
            beat(1, NOW_MS - DAY_MS, Status::Up, Some(1)),
        ],
    )
    .await;

    let expired = storage.expire(Retention::default(), NOW_MS).await.unwrap();
    assert_eq!(
        expired,
        Expired {
            heartbeats: 2,
            minute_rollups: 0
        }
    );
    assert_eq!(
        query::<i64>(&storage, "SELECT count(*) FROM heartbeat").await,
        2
    );
    assert_eq!(
        query::<i64>(&storage, "SELECT count(*) FROM rollup_minute").await,
        4
    );
    assert_eq!(
        query::<i64>(&storage, "SELECT sum(up) FROM rollup_day").await,
        4
    );

    let minute_retention = Retention {
        minute_rollups: Some(Duration::from_secs(30 * 86_400)),
        ..Retention::default()
    };
    let expired = storage.expire(minute_retention, NOW_MS).await.unwrap();
    assert_eq!(expired.minute_rollups, 3);
    assert_eq!(
        query::<i64>(&storage, "SELECT count(*) FROM rollup_hour").await,
        4
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_resumes_from_the_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    let first = admit_visible(&storage, &[beat(1, NOW_MS, Status::Up, Some(5))]).await;
    let watermark = *storage.watermark().borrow();
    storage.close().await.unwrap();

    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    assert_eq!(*storage.watermark().borrow(), watermark);
    let second = admit_visible(&storage, &[beat(1, NOW_MS + 1, Status::Up, Some(5))]).await;
    assert!(second.sequence > first.sequence);
    assert!(storage.watermark().borrow().batch > watermark.batch);
    assert_eq!(
        query::<i64>(&storage, "SELECT count(*) FROM heartbeat").await,
        2
    );
    storage.close().await.unwrap();
}
