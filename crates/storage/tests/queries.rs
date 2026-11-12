//! Release-gate reads: latest status, uptime, daily bars and latency, with imported Kuma rows.

use std::time::Duration;

use chrono::NaiveDate;
use v1per_storage::{
    Budgets, Counts, Heartbeat, HeartbeatCursor, KumaStat, Latency, LatencyPoint, MonitorId,
    MonitorStatus, ProbeId, Resolution, Series, Status, Storage, TrendBucket, UptimeWindow,
};

const NOW_MS: i64 = 1_790_812_800_000; // 2026-10-01T00:00:00Z
const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

async fn open(dir: &tempfile::TempDir) -> Storage {
    let budgets = Budgets {
        transfer_delay: Duration::ZERO,
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    };
    Storage::open(dir.path(), budgets).await.unwrap()
}

/// A server-decided heartbeat (push): it is the monitor-level entry itself.
fn beat(monitor: i64, time_ms: i64, status: Status, latency_ms: Option<u32>) -> Heartbeat {
    probe_beat(None, monitor, time_ms, status, latency_ms)
}

fn probe_beat(
    probe: Option<ProbeId>,
    monitor: i64,
    time_ms: i64,
    status: Status,
    latency_ms: Option<u32>,
) -> Heartbeat {
    Heartbeat {
        monitor: MonitorId(monitor),
        probe,
        time_ms,
        status,
        latency_ms,
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

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn latest_status_per_monitor_within_the_bound() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    admit_visible(
        &storage,
        &[
            beat(1, NOW_MS - MINUTE_MS, Status::Up, Some(5)),
            beat(1, NOW_MS, Status::Down, None),
            beat(2, NOW_MS - 2 * DAY_MS, Status::Up, Some(5)),
        ],
    )
    .await;
    let latest = storage
        .latest(&[MonitorId(1), MonitorId(2)], NOW_MS - DAY_MS)
        .await
        .unwrap()
        .value;
    assert_eq!(latest, vec![beat(1, NOW_MS, Status::Down, None)]);
    storage.close().await.unwrap();
}

/// Pages run newest first, by time then probe, and continue exactly where the cursor left off
/// when several probes share a millisecond.
#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_pages_follow_their_cursor_and_probe_filter() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let (a, b) = (Some(ProbeId(1)), Some(ProbeId(2)));
    admit_visible(
        &storage,
        &[
            probe_beat(a, 1, NOW_MS - MINUTE_MS, Status::Up, Some(5)),
            probe_beat(a, 1, NOW_MS, Status::Up, Some(5)),
            probe_beat(b, 1, NOW_MS, Status::Down, None),
            probe_beat(None, 1, NOW_MS, Status::Up, None),
            probe_beat(b, 2, NOW_MS, Status::Up, Some(5)),
        ],
    )
    .await;
    let page = |probe, limit, after| {
        let storage = &storage;
        async move {
            storage
                .heartbeats(MonitorId(1), probe, limit, after)
                .await
                .unwrap()
                .value
        }
    };
    let key = |beat: &Heartbeat| (beat.time_ms, beat.probe);
    let first = page(None, 2, None).await;
    assert_eq!(
        first.heartbeats.iter().map(key).collect::<Vec<_>>(),
        [(NOW_MS, b), (NOW_MS, a)]
    );
    let cursor = first.next.unwrap();
    assert_eq!(
        cursor,
        HeartbeatCursor {
            time_ms: NOW_MS,
            probe: a
        }
    );
    let second = page(None, 2, Some(cursor)).await;
    assert_eq!(
        second.heartbeats.iter().map(key).collect::<Vec<_>>(),
        [(NOW_MS, None), (NOW_MS - MINUTE_MS, a)]
    );
    let last = page(None, 2, second.next).await;
    assert!(last.heartbeats.is_empty() && last.next.is_none());

    let only_a = page(a, 10, None).await;
    assert_eq!(only_a.heartbeats.len(), 2);
    assert!(only_a.next.is_none());
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn uptime_counts_pending_as_down_and_leaves_maintenance_out() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    admit_visible(
        &storage,
        &[
            beat(1, NOW_MS - 25 * HOUR_MS, Status::Up, Some(10)),
            beat(1, NOW_MS - 10 * MINUTE_MS, Status::Up, Some(10)),
            beat(1, NOW_MS - 5 * MINUTE_MS, Status::Pending, Some(10)),
            beat(1, NOW_MS - 2 * MINUTE_MS, Status::Maintenance, None),
            beat(1, NOW_MS - MINUTE_MS, Status::Down, None),
        ],
    )
    .await;
    let uptime = |window| {
        let storage = &storage;
        async move {
            storage
                .uptime(&[MonitorId(1)], Series::Monitor, window, NOW_MS)
                .await
                .unwrap()
                .value[&MonitorId(1)]
        }
    };
    let day = uptime(UptimeWindow::Day).await;
    assert_eq!(
        (day.up, day.pending, day.down, day.maintenance),
        (1, 1, 1, 1)
    );
    assert_eq!(day.uptime(), Some(1.0 / 3.0));
    for window in [UptimeWindow::Week, UptimeWindow::Month, UptimeWindow::Year] {
        assert_eq!(uptime(window).await.uptime(), Some(2.0 / 4.0));
    }
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn uptime_uses_imported_kuma_rows_for_imported_periods() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let daily: Vec<KumaStat> = (5..=10)
        .map(|days_ago| KumaStat {
            bucket_ms: NOW_MS - days_ago * DAY_MS,
            ping: Some(20.0),
            ping_min: Some(10.0),
            ping_max: Some(40.0),
            up: 90,
            down: 10,
            extras: (days_ago == 5).then(|| r#"{"maintenance":4}"#.to_owned()),
        })
        .collect();
    storage
        .import_kuma_stats(MonitorId(2), Resolution::Day, daily.clone())
        .await
        .unwrap();
    // A retried chunk replaces its rows instead of adding to them.
    storage
        .import_kuma_stats(MonitorId(2), Resolution::Day, daily)
        .await
        .unwrap();
    admit_visible(
        &storage,
        &[
            // Inside the imported period: Kuma's numbers win.
            beat(2, NOW_MS - 6 * DAY_MS + HOUR_MS, Status::Down, None),
            beat(2, NOW_MS - HOUR_MS, Status::Up, Some(30)),
        ],
    )
    .await;
    let year = storage
        .uptime(&[MonitorId(2)], Series::Monitor, UptimeWindow::Year, NOW_MS)
        .await
        .unwrap()
        .value[&MonitorId(2)];
    assert_eq!((year.up, year.down, year.maintenance), (541, 60, 4));
    let latency = year.latency.unwrap();
    assert_eq!((latency.min_ms, latency.max_ms), (10.0, 40.0));
    assert_eq!(latency.avg_ms, (540.0 * 20.0 + 30.0) / 541.0);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn day_bars_follow_the_page_timezone() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    admit_visible(
        &storage,
        &[
            // 2026-09-30 23:30 in Vienna (CEST)
            beat(1, NOW_MS - 150 * MINUTE_MS, Status::Down, None),
            // 2026-10-01 00:30 in Vienna
            beat(1, NOW_MS - 90 * MINUTE_MS, Status::Up, Some(10)),
            // 2026-10-01 00:15 in Kolkata, in the UTC hour starting 2026-09-30 23:30 there
            beat(2, NOW_MS - 315 * MINUTE_MS, Status::Up, Some(10)),
            // 2026-09-30 23:45 in St. John's (NDT, -02:30), 00:15 with the minutes dropped
            beat(3, NOW_MS + 135 * MINUTE_MS, Status::Up, Some(10)),
        ],
    )
    .await;
    let bars = |timezone| {
        let storage = &storage;
        async move {
            storage
                .day_bars(
                    &[MonitorId(1), MonitorId(2), MonitorId(3)],
                    Series::Monitor,
                    timezone,
                    date(2026, 10, 1),
                    90,
                )
                .await
                .unwrap()
                .value
        }
    };
    let vienna = bars(chrono_tz::Europe::Vienna).await;
    let days: Vec<_> = vienna[&MonitorId(1)]
        .iter()
        .map(|bar| (bar.day, bar.counts.up, bar.counts.down))
        .collect();
    assert_eq!(
        days,
        vec![(date(2026, 9, 30), 0, 1), (date(2026, 10, 1), 1, 0)]
    );

    // Non-hour offsets drop their minutes: +05:30 counts as +05:00, -02:30 as -02:00.
    let kolkata = bars(chrono_tz::Asia::Kolkata).await;
    assert_eq!(kolkata[&MonitorId(2)][0].day, date(2026, 9, 30));
    let st_johns = bars(chrono_tz::America::St_Johns).await;
    assert_eq!(st_johns[&MonitorId(3)][0].day, date(2026, 10, 1));
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn latency_series_per_bucket() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    admit_visible(
        &storage,
        &[
            beat(3, NOW_MS + MINUTE_MS, Status::Up, Some(100)),
            beat(3, NOW_MS + 2 * MINUTE_MS, Status::Up, Some(300)),
            beat(3, NOW_MS + 3 * MINUTE_MS, Status::Down, Some(9_000)),
            beat(3, NOW_MS + 61 * MINUTE_MS, Status::Up, Some(50)),
        ],
    )
    .await;
    let series = storage
        .latency(
            MonitorId(3),
            Series::Monitor,
            Resolution::Hour,
            NOW_MS,
            NOW_MS + 2 * HOUR_MS,
        )
        .await
        .unwrap()
        .value;
    let point = |bucket_ms, min_ms, max_ms, avg_ms| LatencyPoint {
        bucket_ms,
        latency: Latency {
            min_ms,
            max_ms,
            avg_ms,
        },
    };
    assert_eq!(
        series,
        vec![
            point(NOW_MS, 100.0, 300.0, 200.0),
            point(NOW_MS + HOUR_MS, 50.0, 50.0, 50.0),
        ]
    );
    assert_eq!(
        storage
            .uptime(&[MonitorId(9)], Series::Monitor, UptimeWindow::Day, NOW_MS)
            .await
            .unwrap()
            .value
            .get(&MonitorId(9)),
        None::<&Counts>
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_results_and_monitor_status_are_separate_series() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let (a, b) = (ProbeId(1), ProbeId(2));
    admit_visible(
        &storage,
        &[
            probe_beat(Some(a), 4, NOW_MS - 3 * MINUTE_MS, Status::Up, Some(100)),
            probe_beat(Some(b), 4, NOW_MS - 3 * MINUTE_MS, Status::Down, None),
            probe_beat(Some(a), 4, NOW_MS - 2 * MINUTE_MS, Status::Up, Some(300)),
            probe_beat(Some(b), 4, NOW_MS - 2 * MINUTE_MS, Status::Up, Some(500)),
        ],
    )
    .await;
    let recorded = storage
        .record_statuses(&[
            MonitorStatus {
                monitor: MonitorId(4),
                time_ms: NOW_MS - 3 * MINUTE_MS,
                status: Status::Up,
                latency_ms: Some(100),
            },
            MonitorStatus {
                monitor: MonitorId(4),
                time_ms: NOW_MS - 2 * MINUTE_MS,
                status: Status::Up,
                latency_ms: Some(400),
            },
        ])
        .await
        .unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= recorded.sequence)
        .await
        .unwrap();

    let uptime = |series| {
        let storage = &storage;
        async move {
            storage
                .uptime(&[MonitorId(4)], series, UptimeWindow::Day, NOW_MS)
                .await
                .unwrap()
                .value[&MonitorId(4)]
        }
    };
    let monitor = uptime(Series::Monitor).await;
    assert_eq!((monitor.up, monitor.down), (2, 0));
    assert_eq!(monitor.latency.unwrap().avg_ms, 250.0);
    let probe_b = uptime(Series::Probe(b)).await;
    assert_eq!((probe_b.up, probe_b.down), (1, 1));
    assert_eq!(probe_b.latency.unwrap().avg_ms, 500.0);

    // Monitor entries write no raw row: raw heartbeats stay per probe.
    let raw: i64 = storage
        .read(|s| {
            s.connection()
                .query_row("SELECT count(*) FROM heartbeat", [], |r| r.get(0))
        })
        .await
        .unwrap()
        .value;
    assert_eq!(raw, 4);
    let latency = storage
        .latency(
            MonitorId(4),
            Series::Probe(a),
            Resolution::Minute,
            NOW_MS - HOUR_MS,
            NOW_MS,
        )
        .await
        .unwrap()
        .value;
    assert_eq!(latency.len(), 2);
    assert_eq!(latency[1].latency.avg_ms, 300.0);
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn day_bars_fall_back_to_kuma_days_before_hourly_data() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let kuma = |resolution, bucket_ms, up| {
        let storage = &storage;
        async move {
            let row = KumaStat {
                bucket_ms,
                ping: Some(20.0),
                ping_min: Some(20.0),
                ping_max: Some(20.0),
                up,
                down: 0,
                extras: None,
            };
            storage
                .import_kuma_stats(MonitorId(5), resolution, vec![row])
                .await
                .unwrap();
        }
    };
    // Kuma's hourly rows start mid-day on 2026-09-29; its daily rows reach back further.
    kuma(Resolution::Day, NOW_MS - 3 * DAY_MS, 1440).await;
    kuma(Resolution::Day, NOW_MS - 2 * DAY_MS, 1440).await;
    kuma(Resolution::Hour, NOW_MS - 2 * DAY_MS + 12 * HOUR_MS, 60).await;
    kuma(Resolution::Hour, NOW_MS - DAY_MS + HOUR_MS, 60).await;

    let bars = storage
        .day_bars(
            &[MonitorId(5)],
            Series::Monitor,
            chrono_tz::UTC,
            date(2026, 9, 30),
            90,
        )
        .await
        .unwrap()
        .value;
    let days: Vec<_> = bars[&MonitorId(5)]
        .iter()
        .map(|bar| (bar.day, bar.counts.up, bar.utc_approximate))
        .collect();
    assert_eq!(
        days,
        vec![
            (date(2026, 9, 28), 1440, true),
            // Partly covered by hourly rows: the whole Kuma day is used instead.
            (date(2026, 9, 29), 1440, true),
            (date(2026, 9, 30), 60, false),
        ]
    );
    storage.close().await.unwrap();
}

/// A monitor Kuma started on the day of the import has hourly rows from that day only, so the
/// day comes from Kuma's daily row; checks after the import still count in it.
#[tokio::test(flavor = "multi_thread")]
async fn the_import_day_adds_native_hours_to_the_kuma_day() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let kuma = |resolution, bucket_ms, up| {
        let storage = &storage;
        async move {
            let row = KumaStat {
                bucket_ms,
                ping: None,
                ping_min: None,
                ping_max: None,
                up,
                down: 0,
                extras: None,
            };
            storage
                .import_kuma_stats(MonitorId(6), resolution, vec![row])
                .await
                .unwrap();
        }
    };
    // Imported at 2026-10-01 03:30 UTC; Kuma's day and hour rows end there.
    kuma(Resolution::Day, NOW_MS, 200).await;
    kuma(Resolution::Hour, NOW_MS + 2 * HOUR_MS, 50).await;
    kuma(Resolution::Hour, NOW_MS + 3 * HOUR_MS, 30).await;
    admit_visible(
        &storage,
        &[
            beat(6, NOW_MS + 4 * HOUR_MS, Status::Up, None),
            beat(6, NOW_MS + 5 * HOUR_MS, Status::Down, None),
        ],
    )
    .await;
    let bars = storage
        .day_bars(
            &[MonitorId(6)],
            Series::Monitor,
            chrono_tz::UTC,
            date(2026, 10, 1),
            90,
        )
        .await
        .unwrap()
        .value;
    let today = &bars[&MonitorId(6)][0];
    assert_eq!(
        (
            today.day,
            today.counts.up,
            today.counts.down,
            today.utc_approximate
        ),
        (date(2026, 10, 1), 201, 1, true)
    );
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn trend_counts_each_monitor_by_its_worst_beat_per_hour() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let earlier = NOW_MS - 2 * HOUR_MS;
    admit_visible(
        &storage,
        &[
            // Before the window.
            beat(1, NOW_MS - DAY_MS, Status::Down, None),
            beat(1, earlier + MINUTE_MS, Status::Up, Some(100)),
            beat(1, earlier + 2 * MINUTE_MS, Status::Down, None),
            beat(2, earlier + MINUTE_MS, Status::Up, Some(200)),
            beat(2, earlier + 2 * MINUTE_MS, Status::Maintenance, None),
            beat(3, earlier + MINUTE_MS, Status::Pending, None),
            beat(1, NOW_MS + MINUTE_MS, Status::Up, Some(50)),
            beat(2, NOW_MS + MINUTE_MS, Status::Maintenance, None),
            // Not asked for.
            beat(4, NOW_MS + MINUTE_MS, Status::Down, None),
        ],
    )
    .await;
    let monitors = [MonitorId(1), MonitorId(2), MonitorId(3)];
    let trend = storage
        .trend(&monitors, NOW_MS + 30 * MINUTE_MS, 24)
        .await
        .unwrap()
        .value;

    assert_eq!(trend.len(), 24);
    assert_eq!(trend[0].bucket_ms, NOW_MS - 23 * HOUR_MS);
    let mix = |b: &TrendBucket| (b.healthy, b.down, b.pending, b.maintenance);
    let past = &trend[21];
    assert_eq!(past.bucket_ms, earlier);
    assert_eq!(mix(past), (1, 1, 1, 1));
    assert_eq!(past.counts.uptime(), Some(0.5));
    assert_eq!(past.counts.latency.map(|l| l.avg_ms), Some(150.0));
    let current = &trend[23];
    assert_eq!(current.bucket_ms, NOW_MS);
    assert_eq!(mix(current), (2, 0, 0, 1));
    assert_eq!(current.counts.uptime(), Some(1.0));
    let empty = &trend[22];
    assert_eq!(mix(empty), (0, 0, 0, 0));
    assert_eq!(empty.counts, Counts::default());
    storage.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hourly_keeps_each_monitor_by_hour_with_empty_hours() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let earlier = NOW_MS - 2 * HOUR_MS;
    admit_visible(
        &storage,
        &[
            beat(1, earlier + MINUTE_MS, Status::Up, Some(100)),
            beat(1, earlier + 2 * MINUTE_MS, Status::Down, None),
            beat(1, NOW_MS + MINUTE_MS, Status::Up, Some(50)),
            beat(2, NOW_MS + MINUTE_MS, Status::Maintenance, None),
            // Not asked for.
            beat(4, NOW_MS + MINUTE_MS, Status::Down, None),
        ],
    )
    .await;
    let hourly = storage
        .hourly(
            &[MonitorId(1), MonitorId(2), MonitorId(3)],
            NOW_MS + 30 * MINUTE_MS,
            24,
        )
        .await
        .unwrap()
        .value;

    assert_eq!(hourly.from_ms, NOW_MS - 23 * HOUR_MS);
    assert_eq!(
        hourly.counts.keys().copied().collect::<Vec<_>>(),
        [MonitorId(1), MonitorId(2), MonitorId(3)]
    );
    let first = &hourly.counts[&MonitorId(1)];
    assert_eq!(first.len(), 24);
    assert_eq!((first[21].up, first[21].down), (1, 1));
    assert_eq!(first[22], Counts::default());
    assert_eq!(first[23].latency.map(|l| l.avg_ms), Some(50.0));
    assert_eq!(hourly.counts[&MonitorId(2)][23].maintenance, 1);
    assert!(
        hourly.counts[&MonitorId(3)]
            .iter()
            .all(|hour| *hour == Counts::default())
    );
    storage.close().await.unwrap();
}

/// Period totals are the numbers the page's 30-day uptime shows for the same days, for
/// native and imported monitors, and a range edge inside a Kuma-only day uses that day whole.
#[tokio::test(flavor = "multi_thread")]
async fn period_totals_equal_the_page_uptime() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let daily: Vec<KumaStat> = (5..=10)
        .map(|days_ago| KumaStat {
            bucket_ms: NOW_MS - days_ago * DAY_MS,
            ping: Some(20.0),
            ping_min: Some(10.0),
            ping_max: Some(40.0),
            up: 90,
            down: 10,
            extras: (days_ago == 5).then(|| r#"{"maintenance":4}"#.to_owned()),
        })
        .collect();
    storage
        .import_kuma_stats(MonitorId(2), Resolution::Day, daily)
        .await
        .unwrap();
    admit_visible(
        &storage,
        &[
            beat(1, NOW_MS - 20 * DAY_MS, Status::Up, Some(10)),
            beat(1, NOW_MS - 3 * HOUR_MS, Status::Up, Some(30)),
            beat(1, NOW_MS - 2 * HOUR_MS, Status::Pending, None),
            beat(1, NOW_MS - HOUR_MS, Status::Maintenance, None),
            beat(1, NOW_MS, Status::Down, None),
            beat(2, NOW_MS - HOUR_MS, Status::Up, Some(30)),
        ],
    )
    .await;
    let monitors = [MonitorId(1), MonitorId(2), MonitorId(3)];
    // The 30 UTC days ending with the day that holds NOW_MS.
    let to = NOW_MS + DAY_MS;
    let totals = storage
        .period_totals(&monitors, to - 30 * DAY_MS, to)
        .await
        .unwrap()
        .value;
    let page = storage
        .uptime(&monitors, Series::Monitor, UptimeWindow::Month, NOW_MS)
        .await
        .unwrap()
        .value;
    for monitor in [MonitorId(1), MonitorId(2)] {
        let (total, counts) = (totals[&monitor], page[&monitor]);
        assert_eq!(total.up, counts.up);
        assert_eq!(total.down, counts.down + counts.pending);
        assert_eq!(total.pending, counts.pending);
        assert_eq!(total.maintenance, counts.maintenance);
        assert_eq!(total.uptime(), counts.uptime());
        assert_eq!(
            total.latency_sum / total.latency_count as f64,
            counts.latency.unwrap().avg_ms
        );
        assert!(total.has_data && !total.utc_approximate);
    }
    assert_eq!(
        (totals[&MonitorId(1)].up, totals[&MonitorId(1)].down),
        (2, 2)
    );
    assert_eq!(totals[&MonitorId(2)].up, 541);
    assert!(!totals[&MonitorId(3)].has_data);

    // The range starts mid-day 10 days ago and ends mid-day 6 days ago: the Kuma days that
    // start inside count whole, the one that starts before the range does not.
    let cut = storage
        .period_totals(
            &[MonitorId(2)],
            NOW_MS - 8 * DAY_MS + 6 * HOUR_MS,
            NOW_MS - 6 * DAY_MS + 6 * HOUR_MS,
        )
        .await
        .unwrap()
        .value[&MonitorId(2)];
    assert_eq!((cut.up, cut.down), (180, 20));
    assert!(cut.utc_approximate);
    storage.close().await.unwrap();
}

/// Each minute counts once however many monitors beat in it; maintenance-only minutes are
/// not counted, and a range the minute rollups do not reach back to is reported as uncovered.
#[tokio::test(flavor = "multi_thread")]
async fn down_minutes_count_each_minute_once() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let at = |minute: i64| NOW_MS + minute * MINUTE_MS;
    let mut beats = Vec::new();
    for minute in 0..30 {
        let first = if (10..20).contains(&minute) {
            Status::Down
        } else {
            Status::Up
        };
        beats.push(beat(1, at(minute), first, None));
        if minute < 25 {
            let second = if minute >= 15 {
                Status::Down
            } else {
                Status::Up
            };
            beats.push(beat(2, at(minute), second, None));
        }
    }
    beats.extend([
        // Disjoint outages, one of them pending.
        beat(1, at(50), Status::Down, None),
        beat(2, at(52), Status::Pending, None),
        // Counted by nobody.
        beat(1, at(40), Status::Maintenance, None),
    ]);
    admit_visible(&storage, &beats).await;
    let monitors = [MonitorId(1), MonitorId(2)];
    let minutes = storage
        .down_minutes(&monitors, NOW_MS, NOW_MS + HOUR_MS)
        .await
        .unwrap()
        .value;
    // Minutes 0..30 and 50 and 52 are counted; 10..25, 50 and 52 are down (15 overlapping).
    assert_eq!((minutes.counted, minutes.down), (32, 17));
    assert!(minutes.covered);

    let earlier = storage
        .down_minutes(&monitors, NOW_MS - DAY_MS, NOW_MS)
        .await
        .unwrap()
        .value;
    assert_eq!(
        (earlier.counted, earlier.down, earlier.covered),
        (0, 0, false)
    );
    storage.close().await.unwrap();
}

/// Probe groups merge their rollups per step, and every series gets the average and
/// percentiles of its own up checks from raw heartbeats.
#[tokio::test(flavor = "multi_thread")]
async fn response_times_merge_probe_groups_with_percentiles() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let mut beats = Vec::new();
    let mut statuses = Vec::new();
    for hour in 0..8 {
        let at = NOW_MS + hour * HOUR_MS;
        for probe in 1..=3 {
            let latency = (10 * probe + hour) as u32;
            beats.push(probe_beat(
                Some(ProbeId(probe)),
                1,
                at + probe * MINUTE_MS,
                Status::Up,
                Some(latency),
            ));
        }
        beats.push(probe_beat(
            Some(ProbeId(1)),
            1,
            at + 30 * MINUTE_MS,
            Status::Down,
            Some(900),
        ));
        statuses.push(MonitorStatus {
            monitor: MonitorId(1),
            time_ms: at,
            status: Status::Up,
            latency_ms: Some(100 + hour as u32),
        });
    }
    let admitted = storage
        .admit_evaluated(None, &beats, &statuses)
        .await
        .unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= admitted.sequence)
        .await
        .unwrap();

    let groups = vec![vec![ProbeId(1)], vec![ProbeId(2), ProbeId(3)]];
    let times = storage
        .response_times(
            MonitorId(1),
            groups,
            Resolution::Hour,
            4 * HOUR_MS,
            NOW_MS,
            NOW_MS + 8 * HOUR_MS,
        )
        .await
        .unwrap()
        .value;
    let averages = |points: &[LatencyPoint]| -> Vec<(i64, f64)> {
        points
            .iter()
            .map(|p| (p.bucket_ms - NOW_MS, p.latency.avg_ms))
            .collect()
    };
    let step = 4 * HOUR_MS;
    assert_eq!(averages(&times.overall.points), [(0, 101.5), (step, 105.5)]);
    assert_eq!(averages(&times.groups[0].points), [(0, 11.5), (step, 15.5)]);
    assert_eq!(averages(&times.groups[1].points), [(0, 26.5), (step, 30.5)]);
    assert_eq!(times.groups[1].points[0].latency.min_ms, 20.0);
    assert_eq!(times.groups[1].points[0].latency.max_ms, 33.0);

    // Probe 1 checked 10 to 17 ms; its down beats do not count.
    let vienna = times.groups[0].summary.unwrap();
    assert_eq!(vienna.checks, 8);
    assert_eq!(vienna.avg_ms, 13.5);
    assert_eq!(vienna.p50_ms, 13.5);
    assert!((vienna.p95_ms - 16.65).abs() < 1e-9);
    assert!((vienna.p99_ms - 16.93).abs() < 1e-9);
    assert_eq!(times.groups[1].summary.unwrap().checks, 16);
    let overall = times.overall.summary.unwrap();
    assert_eq!(
        (overall.checks, overall.avg_ms, overall.p50_ms),
        (24, 23.5, 23.5)
    );

    let empty = storage
        .response_times(
            MonitorId(1),
            vec![vec![ProbeId(9)]],
            Resolution::Hour,
            HOUR_MS,
            NOW_MS - DAY_MS,
            NOW_MS,
        )
        .await
        .unwrap()
        .value;
    assert!(empty.overall.points.is_empty() && empty.overall.summary.is_none());
    assert_eq!(empty.groups[0], Default::default());
    storage.close().await.unwrap();
}

/// Compaction rewrites each closed day once and keeps every heartbeat.
#[tokio::test(flavor = "multi_thread")]
async fn compaction_rewrites_closed_days_once() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(&dir).await;
    let mut beats = Vec::new();
    for minute in 0..(3 * 24 * 6) {
        let at = NOW_MS + minute * 10 * MINUTE_MS;
        for monitor in 1..=3 {
            beats.push(probe_beat(
                Some(ProbeId(1)),
                monitor,
                at + monitor,
                Status::Up,
                Some((minute % 50) as u32),
            ));
        }
    }
    admit_visible(&storage, &beats).await;
    let now = NOW_MS + 2 * DAY_MS + HOUR_MS;
    let everything = || async {
        let mut all = Vec::new();
        for monitor in 1..=3 {
            let page = storage
                .heartbeats(MonitorId(monitor), None, 10_000, None)
                .await
                .unwrap()
                .value;
            all.extend(page.heartbeats);
        }
        all
    };
    let before = everything().await;
    let compacted = storage
        .compact(v1per_storage::Retention::default(), now)
        .await
        .unwrap();
    // Every closed day in the 90 days, empty ones too, and not today.
    assert_eq!(compacted, 90);
    assert_eq!(
        storage
            .compact(v1per_storage::Retention::default(), now)
            .await
            .unwrap(),
        0
    );
    assert_eq!(everything().await, before);
    storage.close().await.unwrap();
}
