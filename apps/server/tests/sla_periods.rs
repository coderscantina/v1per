//! SLA periods (milestone 14): availability against the page uptime, the `any_down` tally,
//! frozen periods and recompute, and the incident clocks.

mod common;

use chrono::{DateTime, NaiveDate, Utc};
use common::Harness;
use serde_json::{Value, json};
use v1per_server::clock::Clock;
use v1per_storage::{Heartbeat, MonitorId, Retention, Series, Status};

/// Friday 2026-10-23 14:00 in Vienna.
const NOW: &str = "2026-10-23T12:00:00Z";
const MINUTE_MS: i64 = 60_000;

fn ms(at: &str) -> i64 {
    at.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}

/// Staff with Admin on the agency, a client Admin on the client below it, and two manual
/// monitors on the client, on a clock stopped at [`NOW`].
struct Fixture {
    harness: Harness,
    staff: String,
    client_admin: String,
    client: String,
    monitors: [String; 2],
}

impl Fixture {
    async fn new() -> Self {
        let harness = Harness::with_clock(Clock::manual(ms(NOW))).await;
        let admin = harness.session(&harness.admin).await;
        let root = harness.root().await;
        let agency = harness.node(&admin, &root, "agency").await;
        let client = harness.node(&admin, &agency, "client").await;
        harness.grant(&harness.member, &agency, "admin").await;
        let client_admin = harness.person("client").await;
        harness.grant(&client_admin, &client, "admin").await;
        let mut monitors = Vec::new();
        for name in ["api", "web"] {
            let created = harness
                .ok(
                    &admin,
                    "monitors.create",
                    json!({ "node": client, "name": name, "interval": 60, "settings": { "type": "manual" } }),
                )
                .await;
            monitors.push(created["monitor"]["id"].as_str().unwrap().to_owned());
        }
        Self {
            staff: harness.session(&harness.member).await,
            client_admin: harness.session(&client_admin).await,
            harness,
            client,
            monitors: monitors.try_into().unwrap(),
        }
    }

    fn storage_id(&self, index: usize) -> MonitorId {
        MonitorId(self.monitors[index].parse().unwrap())
    }

    async fn sla(&self, terms: Value) -> String {
        let mut body = json!({
            "node": self.client,
            "name": "Acme hosting",
            "timezone": "Europe/Vienna",
            "period": "month",
            "start_date": "2026-09-01",
            "combine": "each",
            "monitors": self.monitors,
        });
        body.as_object_mut()
            .unwrap()
            .extend(terms.as_object().unwrap().clone());
        let created = self.harness.ok(&self.staff, "slas.create", body).await;
        created["sla"]["id"].as_str().unwrap().to_owned()
    }

    /// Heartbeats of monitor `index`, visible to reads on return.
    async fn beats(&self, index: usize, beats: impl IntoIterator<Item = (i64, Status)>) {
        let beats: Vec<Heartbeat> = beats
            .into_iter()
            .map(|(time_ms, status)| Heartbeat {
                monitor: self.storage_id(index),
                probe: None,
                time_ms,
                status,
                latency_ms: (status == Status::Up).then_some(40),
                message: String::new(),
                retries: 0,
                http_status: None,
                cert_expiry_ms: None,
            })
            .collect();
        let storage = &self.harness.state.storage;
        let admitted = storage.admit(None, &beats).await.unwrap();
        storage
            .watermark()
            .wait_for(|w| w.sequence >= admitted.sequence)
            .await
            .unwrap();
    }

    async fn period(&self, sla: &str, start: Option<&str>) -> Value {
        let query = match start {
            Some(start) => format!("sla={sla}&start={start}"),
            None => format!("sla={sla}"),
        };
        let answer = self
            .harness
            .query(Some(&self.staff), "slas.period", &query)
            .await;
        assert_eq!(answer.status, 200, "{}", answer.json());
        answer.json()["period"].clone()
    }

    async fn catch_up(&self) {
        let more = v1per_server::slas::catch_up(&self.harness.state, ms(NOW))
            .await
            .unwrap();
        assert!(!more);
    }

    /// An incident on the first monitor whose first update sets a major outage on it, by a
    /// person when `answered_at` is set and by the server otherwise.
    async fn incident(&self, started: &str, answered_at: Option<&str>, resolved: Option<&str>) {
        let db = &self.harness.state.db.writer;
        let (auto, monitor) = (answered_at.is_none(), &self.monitors[0]);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO incidents (node_id, title, state, auto, auto_monitor_id, started_at,
                 resolved_at, revision, updated_at)
             VALUES (?, 'api is down', ?, ?, ?, ?, ?, 1, ?) RETURNING id",
        )
        .bind(self.client.parse::<i64>().unwrap())
        .bind(if resolved.is_some() {
            "resolved"
        } else {
            "investigating"
        })
        .bind(auto)
        .bind(auto.then(|| monitor.parse::<i64>().unwrap()))
        .bind(ms(started))
        .bind(resolved.map(ms))
        .bind(ms(started))
        .fetch_one(db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO incident_targets (incident_id, monitor_id) VALUES (?, ?)")
            .bind(id)
            .bind(monitor)
            .execute(db)
            .await
            .unwrap();
        let author = answered_at.map(|_| self.harness.admin.as_str().to_owned());
        let update: i64 = sqlx::query_scalar(
            "INSERT INTO incident_updates (incident_id, state, message, author_id, created_at)
             VALUES (?, 'investigating', 'looking', ?, ?) RETURNING id",
        )
        .bind(id)
        .bind(author)
        .bind(ms(answered_at.unwrap_or(started)))
        .fetch_one(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO incident_update_impacts (update_id, monitor_id, impact)
             VALUES (?, ?, 'major_outage')",
        )
        .bind(update)
        .bind(monitor)
        .execute(db)
        .await
        .unwrap();
    }
}

/// In `each` mode a monitor's availability for September in Vienna is the uptime of the status
/// page's day bars for the same days: pending counts as down, maintenance is left out, and a
/// beat just outside the local month does not count.
#[tokio::test]
async fn each_mode_availability_equals_the_page_uptime() {
    let f = Fixture::new().await;
    let sla = f.sla(json!({ "availability": 99.0 })).await;
    let mut beats = Vec::new();
    // Hourly beats through the local month, which starts 2026-08-31T22:00Z.
    let start = ms("2026-08-31T22:00:00Z");
    for hour in 0..30 * 24 {
        let status = match hour % 50 {
            0 => Status::Down,
            1 => Status::Pending,
            2 => Status::Maintenance,
            _ => Status::Up,
        };
        beats.push((start + hour * 60 * MINUTE_MS, status));
    }
    // An hour before the local month: the UTC month, but not the SLA's.
    beats.push((start - 60 * MINUTE_MS, Status::Down));
    f.beats(0, beats).await;

    let period = f.period(&sla, Some("2026-09-01")).await;
    assert_eq!(period["starts_at"], start);
    let bars = f
        .harness
        .state
        .storage
        .day_bars(
            &[f.storage_id(0)],
            Series::Monitor,
            chrono_tz::Europe::Vienna,
            NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
            30,
        )
        .await
        .unwrap()
        .value;
    let (mut up, mut down) = (0, 0);
    for bar in &bars[&f.storage_id(0)] {
        up += bar.counts.up;
        down += bar.counts.down + bar.counts.pending;
    }
    let page = up as f64 / (up + down) as f64 * 100.0;
    let api = &period["monitors"][0];
    assert_eq!(api["monitor"], f.monitors[0].as_str());
    assert!((api["availability"].as_f64().unwrap() - page).abs() < 1e-9);
    assert!(page < 99.0);
    assert_eq!(api["availability_outcome"], "missed");
    assert_eq!(period["availability"]["outcome"], "missed");
    // The second monitor has no beats: no data, not a miss.
    assert_eq!(period["monitors"][1]["availability_outcome"], "no_data");
}

/// The `any_down` tally of a closed day counts each down minute once, and neither it nor the
/// period changes once the minute rollups of that day are deleted.
#[tokio::test]
async fn a_tally_outlives_the_minute_rollups_of_its_day() {
    let f = Fixture::new().await;
    let sla = f
        .sla(json!({ "combine": "any_down", "availability": 99.9, "start_date": "2026-10-01" }))
        .await;
    // 2026-10-20 in Vienna starts at 2026-10-19T22:00Z. Minute data reaches back before it.
    let day = ms("2026-10-19T22:00:00Z");
    let at = |minute: i64| day + minute * MINUTE_MS;
    let outage = |from: i64, to: i64, minute: i64| {
        if (from..to).contains(&minute) {
            Status::Down
        } else {
            Status::Up
        }
    };
    f.beats(0, (-60..120).map(|m| (at(m), outage(10, 20, m))))
        .await;
    f.beats(1, (0..120).map(|m| (at(m), outage(15, 30, m))))
        .await;

    f.catch_up().await;
    let tally = || async {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT counted_minutes, down_minutes FROM sla_tallies WHERE sla_id = ? AND day = ?",
        )
        .bind(sla.parse::<i64>().unwrap())
        .bind("2026-10-20")
        .fetch_one(&f.harness.state.db.reader)
        .await
        .unwrap()
    };
    assert_eq!(tally().await, (120, 20));
    let before = f.period(&sla, None).await;
    assert!(before["availability"]["actual"].as_f64().is_some());

    let retention = Retention {
        minute_rollups: Some(std::time::Duration::from_secs(86_400)),
        ..Retention::default()
    };
    let expired = f
        .harness
        .state
        .storage
        .expire(retention, ms(NOW))
        .await
        .unwrap();
    assert!(expired.minute_rollups > 0);
    f.catch_up().await;
    assert_eq!(tally().await, (120, 20));
    assert_eq!(f.period(&sla, None).await, before);
}

/// A frozen period keeps its result when the targets change and a covered monitor is deleted.
/// Only staff above the client recompute it, which applies the current terms and data.
#[tokio::test]
async fn a_frozen_period_changes_only_through_recompute() {
    let f = Fixture::new().await;
    let sla = f.sla(json!({ "availability": 99.0 })).await;
    let start = ms("2026-08-31T22:00:00Z");
    f.beats(
        0,
        (0..100).map(|h| (start + h * 60 * MINUTE_MS, Status::Up)),
    )
    .await;
    f.beats(
        1,
        (0..100).map(|h| {
            let status = if h < 10 { Status::Down } else { Status::Up };
            (start + h * 60 * MINUTE_MS, status)
        }),
    )
    .await;

    f.catch_up().await;
    let frozen = f.period(&sla, Some("2026-09-01")).await;
    assert_eq!(frozen["frozen_at"], ms(NOW));
    assert_eq!(frozen["availability"]["outcome"], "missed");
    assert_eq!(frozen["monitors"].as_array().unwrap().len(), 2);

    let h = &f.harness;
    let update = json!({
        "sla": sla,
        "name": "Acme hosting",
        "timezone": "Europe/Vienna",
        "period": "month",
        "start_date": "2026-09-01",
        "combine": "each",
        "availability": 50.0,
        "monitors": f.monitors,
    });
    h.ok(&f.staff, "slas.update", update).await;
    let admin = h.session(&h.admin).await;
    h.ok(
        &admin,
        "monitors.delete",
        json!({ "monitor": f.monitors[1] }),
    )
    .await;
    f.catch_up().await;
    assert_eq!(f.period(&sla, Some("2026-09-01")).await, frozen);

    let args = json!({ "sla": sla, "start": "2026-09-01" });
    let refused = h
        .run(Some(&f.client_admin), "slas.recompute", args.clone())
        .await;
    assert_eq!(refused.code(), "forbidden");
    let open = h
        .run(
            Some(&f.staff),
            "slas.recompute",
            json!({ "sla": sla, "start": "2026-10-01" }),
        )
        .await;
    assert_eq!(open.code(), "invalid_arguments");
    let recomputed = h.ok(&f.staff, "slas.recompute", args).await["period"].clone();
    assert_eq!(recomputed["terms"]["availability"], 50.0);
    assert_eq!(recomputed["availability"]["outcome"], "met");
    assert_eq!(recomputed["monitors"].as_array().unwrap().len(), 1);
    assert_eq!(f.period(&sla, Some("2026-09-01")).await, recomputed);
}

/// A running period is at risk once 75 % of the downtime budget is used and not yet exceeded:
/// 99 % over October at one beat a minute allows 446 down beats.
#[tokio::test]
async fn a_running_period_is_at_risk_from_three_quarters_of_the_budget() {
    let f = Fixture::new().await;
    let sla = f
        .sla(json!({ "start_date": "2026-10-01", "availability": 99.0, "monitors": [f.monitors[0]] }))
        .await;
    let start = ms("2026-10-05T00:00:00Z");
    let down = |from: i64, count: i64| {
        (from..from + count).map(move |i| (start + i * MINUTE_MS, Status::Down))
    };
    f.beats(0, down(0, 300)).await;
    let period = f.period(&sla, None).await;
    assert_eq!(period["monitors"][0]["availability_outcome"], "on_track");
    assert_eq!(period["availability"]["outcome"], "on_track");

    f.beats(0, down(300, 100)).await;
    let period = f.period(&sla, None).await;
    assert_eq!(period["monitors"][0]["availability_outcome"], "at_risk");
    assert_eq!(period["availability"]["outcome"], "at_risk");

    f.beats(0, down(400, 100)).await;
    let period = f.period(&sla, None).await;
    assert_eq!(period["availability"]["outcome"], "missed");
}

/// Latency is judged missed only when the period closes, so a running average above the
/// target is at risk instead.
#[tokio::test]
async fn a_running_period_with_slow_latency_is_at_risk() {
    let f = Fixture::new().await;
    let admin = f.harness.session(&f.harness.admin).await;
    let created = f
        .harness
        .ok(
            &admin,
            "monitors.create",
            json!({ "node": f.client, "name": "site", "interval": 60,
                    "settings": { "type": "http", "url": "http://site.test" } }),
        )
        .await;
    let site = created["monitor"]["id"].as_str().unwrap().to_owned();
    let sla = f
        .sla(json!({ "start_date": "2026-10-01", "latency_ms": 250, "monitors": [site] }))
        .await;
    let beat = |time_ms, latency_ms| Heartbeat {
        monitor: MonitorId(site.parse().unwrap()),
        probe: None,
        time_ms,
        status: Status::Up,
        latency_ms: Some(latency_ms),
        message: String::new(),
        retries: 0,
        http_status: None,
        cert_expiry_ms: None,
    };
    let storage = &f.harness.state.storage;
    let start = ms("2026-10-05T00:00:00Z");
    let admitted = storage
        .admit(None, &[beat(start, 819), beat(start + MINUTE_MS, 819)])
        .await
        .unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= admitted.sequence)
        .await
        .unwrap();

    let period = f.period(&sla, None).await;
    assert_eq!(period["latency"], "at_risk");
    assert_eq!(period["monitors"][0]["latency_outcome"], "at_risk");
}

/// A closed period before any monitor reported has nothing to judge, even without incidents.
/// With data and no incident the clocks are met.
#[tokio::test]
async fn a_period_without_data_has_no_outcome() {
    let f = Fixture::new().await;
    let sla = f
        .sla(json!({ "availability": 99.0, "times": { "major_outage": { "response_minutes": 30, "resolution_minutes": 240 } } }))
        .await;
    let empty = f.period(&sla, Some("2026-09-01")).await;
    assert_eq!(empty["availability"]["outcome"], "no_data");
    assert_eq!(empty["response"]["outcome"], "no_data");
    assert_eq!(empty["resolution"]["outcome"], "no_data");

    let start = ms("2026-08-31T22:00:00Z");
    f.beats(0, (0..10).map(|i| (start + i * MINUTE_MS, Status::Up)))
        .await;
    let seen = f.period(&sla, Some("2026-09-01")).await;
    assert_eq!(seen["response"]["outcome"], "met");
    assert_eq!(seen["resolution"]["outcome"], "met");
}

/// An auto-incident that resolves before its response deadline without a human update needs
/// no response: it is neither met nor missed. One still open at the deadline missed it.
#[tokio::test]
async fn a_self_healed_incident_is_not_a_missed_response() {
    let f = Fixture::new().await;
    let sla = f
        .sla(json!({ "start_date": "2026-10-01", "times": { "major_outage": { "response_minutes": 30, "resolution_minutes": 240 } } }))
        .await;
    f.incident("2026-10-05T10:00:00Z", None, Some("2026-10-05T10:45:00Z"))
        .await;

    let period = f.period(&sla, None).await;
    let incident = &period["incidents"][0];
    assert_eq!(incident["impact"], "major_outage");
    assert_eq!(incident["response"]["outcome"], "missed");
    // Resolved within the response target instead: not counted.
    f.incident("2026-10-06T10:00:00Z", None, Some("2026-10-06T10:12:00Z"))
        .await;
    let period = f.period(&sla, None).await;
    let healed = &period["incidents"][1];
    assert_eq!(healed["response"]["outcome"], "not_needed");
    assert_eq!(healed["resolution"]["outcome"], "met");
    assert_eq!(period["response"]["counted"], 1);
    assert_eq!(period["response"]["met"], 0);

    let resolution_only = f
        .sla(json!({ "start_date": "2026-10-01", "times": { "major_outage": { "resolution_minutes": 240 } } }))
        .await;
    let period = f.period(&resolution_only, None).await;
    assert_eq!(period["incidents"][1]["response"]["outcome"], "no_target");
}

/// Business hours stop the clocks over a weekend: Friday 16:00 to Monday 10:00 in Vienna is
/// two business hours.
#[tokio::test]
async fn business_hours_skip_a_weekend() {
    let f = Fixture::new().await;
    let weekdays: Vec<Value> = (1..=5)
        .map(|weekday| json!({ "weekday": weekday, "start": "09:00", "end": "17:00" }))
        .collect();
    let sla = f
        .sla(json!({
            "start_date": "2026-10-01",
            "times": { "major_outage": { "response_minutes": 30, "resolution_minutes": 120 } },
            "hours": weekdays,
        }))
        .await;
    f.incident(
        "2026-10-16T14:00:00Z",
        Some("2026-10-16T14:20:00Z"),
        Some("2026-10-19T08:00:00Z"),
    )
    .await;

    let period = f.period(&sla, None).await;
    let incident = &period["incidents"][0];
    assert_eq!(incident["response"]["outcome"], "met");
    assert_eq!(incident["response"]["elapsed_ms"], 20 * MINUTE_MS);
    assert_eq!(incident["resolution"]["outcome"], "met");
    assert_eq!(incident["resolution"]["elapsed_ms"], 120 * MINUTE_MS);
    assert_eq!(
        incident["resolution"]["deadline"],
        ms("2026-10-19T08:00:00Z")
    );
}
