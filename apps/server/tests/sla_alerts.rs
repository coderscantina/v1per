//! SLA alerts through the notification outbox and the SLA section of status page snapshots
//! (milestone 14).

mod common;

use std::collections::HashSet;

use axum::body::Body;
use axum::http::{Request, header};
use chrono::{DateTime, Utc};
use common::{HOST, Harness};
use serde_json::{Value, json};
use v1per_server::clock::Clock;
use v1per_storage::{Heartbeat, MonitorId, Status};

/// Friday 2026-10-23 14:00 in Vienna.
const NOW: &str = "2026-10-23T12:00:00Z";
/// An hour after October closed in Vienna, when it freezes.
const OCTOBER_FROZEN: &str = "2026-11-01T00:00:00Z";
const HOUR_MS: i64 = 3_600_000;
const INCIDENT_TITLE: &str = "Secret database outage";

fn ms(at: &str) -> i64 {
    at.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}

/// An admin, staff with Admin on the agency, a client below it with the monitors `api`
/// (manual) and `web` (with `web` settings), and a Discord channel on the client, on a clock
/// stopped at [`NOW`].
struct Fixture {
    harness: Harness,
    admin: String,
    staff: String,
    client: String,
    monitors: [String; 2],
    channel: String,
}

impl Fixture {
    async fn new(web: Value) -> Self {
        let harness = Harness::with_clock(Clock::manual(ms(NOW))).await;
        let admin = harness.session(&harness.admin).await;
        let root = harness.root().await;
        let agency = harness.node(&admin, &root, "agency").await;
        let client = harness.node(&admin, &agency, "client").await;
        harness.grant(&harness.member, &agency, "admin").await;
        let mut monitors = Vec::new();
        for (name, settings) in [("api", json!({ "type": "manual" })), ("web", web)] {
            let created = harness
                .ok(
                    &admin,
                    "monitors.create",
                    json!({ "node": client, "name": name, "interval": 60, "settings": settings }),
                )
                .await;
            monitors.push(created["monitor"]["id"].as_str().unwrap().to_owned());
        }
        let channel = harness
            .ok(
                &admin,
                "channels.create",
                json!({
                    "node": client, "name": "ops",
                    "settings": { "type": "discord", "webhook_url": { "value": "http://127.0.0.1:9/hook" } },
                }),
            )
            .await["channel"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            staff: harness.session(&harness.member).await,
            harness,
            admin,
            client,
            monitors: monitors.try_into().unwrap(),
            channel,
        }
    }

    async fn manual() -> Self {
        Self::new(json!({ "type": "manual" })).await
    }

    async fn sla(&self, name: &str, terms: Value) -> String {
        let mut body = json!({
            "node": self.client,
            "name": name,
            "timezone": "Europe/Vienna",
            "period": "month",
            "start_date": "2026-09-01",
            "combine": "each",
            "monitors": self.monitors,
            "channels": [self.channel],
        });
        body.as_object_mut()
            .unwrap()
            .extend(terms.as_object().unwrap().clone());
        let created = self.harness.ok(&self.staff, "slas.create", body).await;
        created["sla"]["id"].as_str().unwrap().to_owned()
    }

    /// Availability 99.9 %, a 30 minute response and a 4 hour resolution for major outages.
    async fn contract(&self) -> String {
        self.sla(
            "Acme hosting",
            json!({
                "availability": 99.9,
                "times": { "major_outage": { "response_minutes": 30, "resolution_minutes": 240 } },
            }),
        )
        .await
    }

    /// Hourly beats of monitor `index` through October up to now, every fifth one down.
    async fn october(&self, index: usize) {
        let start = ms("2026-09-30T22:00:00Z");
        let beats: Vec<Heartbeat> = (0..)
            .map(|hour| start + hour * HOUR_MS)
            .take_while(|at| *at < ms(NOW))
            .enumerate()
            .map(|(hour, time_ms)| {
                let status = if hour % 5 == 0 {
                    Status::Down
                } else {
                    Status::Up
                };
                Heartbeat {
                    monitor: MonitorId(self.monitors[index].parse().unwrap()),
                    probe: None,
                    time_ms,
                    status,
                    latency_ms: (status == Status::Up).then_some(40),
                    message: String::new(),
                    retries: 0,
                    http_status: None,
                    cert_expiry_ms: None,
                }
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

    /// An auto-incident on `api` since 07:00 UTC, open, without a human update and published
    /// nowhere, so a draft.
    async fn draft_incident(&self) -> i64 {
        self.incident("2026-10-23T07:00:00Z", None).await
    }

    /// An auto-incident on `api` from `started` to `resolved`, without a human update.
    async fn incident(&self, started: &str, resolved: Option<&str>) -> i64 {
        let db = &self.harness.state.db.writer;
        let monitor: i64 = self.monitors[0].parse().unwrap();
        let started = ms(started);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO incidents (node_id, title, state, auto, auto_monitor_id, started_at,
                 resolved_at, revision, updated_at)
             VALUES (?, ?, ?, 1, ?, ?, ?, 1, ?) RETURNING id",
        )
        .bind(self.client.parse::<i64>().unwrap())
        .bind(INCIDENT_TITLE)
        .bind(if resolved.is_some() {
            "resolved"
        } else {
            "investigating"
        })
        .bind(monitor)
        .bind(started)
        .bind(resolved.map(ms))
        .bind(started)
        .fetch_one(db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO incident_targets (incident_id, monitor_id) VALUES (?, ?)")
            .bind(id)
            .bind(monitor)
            .execute(db)
            .await
            .unwrap();
        id
    }

    async fn catch_up(&self, at: &str) {
        let more = v1per_server::slas::catch_up(&self.harness.state, ms(at))
            .await
            .unwrap();
        assert!(!more);
    }

    /// `(event, monitor, sla, transition)` of every outbox row, oldest first.
    async fn outbox(&self) -> Vec<(String, Option<i64>, Option<i64>, Option<i64>)> {
        sqlx::query_as(
            "SELECT event, monitor_id, sla_id, transition_id FROM notification_outbox ORDER BY id",
        )
        .fetch_all(&self.harness.state.db.reader)
        .await
        .unwrap()
    }

    /// A public page `acme` on the client showing `api` and, marked internal, `web`, with
    /// `slas` selected.
    async fn page(&self, slas: &[&str]) {
        let created = self
            .harness
            .ok(
                &self.admin,
                "pages.create",
                json!({ "node": self.client, "slug": "acme", "title": "Acme status" }),
            )
            .await;
        let page = &created["page"]["id"];
        self.harness
            .ok(
                &self.admin,
                "pages.updateLayout",
                json!({
                    "page": page,
                    "expected_revision": created["page"]["revision"],
                    "groups": [{ "name": "Services", "components": [
                        { "target": { "type": "monitor", "monitor": self.monitors[0] }, "display_name": "API" },
                        { "target": { "type": "monitor", "monitor": self.monitors[1] }, "internal": true },
                    ] }],
                }),
            )
            .await;
        self.harness
            .ok(
                &self.admin,
                "pages.updateAccess",
                json!({ "page": page, "public": true }),
            )
            .await;
        self.harness
            .ok(
                &self.admin,
                "pages.updateSlas",
                json!({ "page": page, "slas": slas }),
            )
            .await;
    }

    /// The snapshot as text, for the public audience or with `cookie`.
    async fn snapshot(&self, cookie: Option<&str>) -> String {
        let mut request = Request::get("/s/acme/snapshot.json").header(header::HOST, HOST);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let answer = self
            .harness
            .send(request.body(Body::empty()).unwrap())
            .await;
        assert_eq!(answer.status, 200);
        String::from_utf8(answer.bytes).unwrap()
    }
}

/// Every object key below `value`.
fn keys(value: &Value, into: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                into.insert(key.clone());
                keys(item, into);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| keys(item, into)),
        _ => {}
    }
}

/// The budget warning, the availability breach of `api` and the missed response and
/// resolution of the incident are each queued once; a restarted server queues none again.
#[tokio::test]
async fn each_alert_is_queued_once_per_period_across_a_restart() {
    let f = Fixture::manual().await;
    f.contract().await;
    f.october(0).await;
    f.draft_incident().await;

    f.catch_up(NOW).await;
    let mut events: Vec<String> = f.outbox().await.into_iter().map(|row| row.0).collect();
    events.sort();
    assert_eq!(
        events,
        [
            "sla_availability_breach",
            "sla_budget_warning",
            "sla_resolution_missed",
            "sla_response_missed",
        ]
    );
    f.catch_up(NOW).await;
    assert_eq!(f.outbox().await.len(), 4);

    let Fixture { harness, .. } = f;
    let background = harness.start();
    let harness = harness.restart(background).await;
    v1per_server::slas::catch_up(&harness.state, ms(NOW))
        .await
        .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM notification_outbox")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(rows, 4);
}

/// A response deadline that falls after the period froze is still alerted once it passes, and
/// a restarted server does not alert it again.
#[tokio::test]
async fn a_response_deadline_after_the_freeze_is_alerted_once() {
    let f = Fixture::manual().await;
    f.sla(
        "Acme hosting",
        json!({ "times": { "major_outage": { "response_minutes": 120 } } }),
    )
    .await;
    f.incident("2026-10-31T22:30:00Z", None).await;

    f.catch_up(OCTOBER_FROZEN).await;
    assert!(f.outbox().await.is_empty());

    f.catch_up("2026-11-01T01:00:00Z").await;
    let events: Vec<String> = f.outbox().await.into_iter().map(|row| row.0).collect();
    assert_eq!(events, ["sla_response_missed"]);

    let Fixture { harness, .. } = f;
    let background = harness.start();
    let harness = harness.restart(background).await;
    v1per_server::slas::catch_up(&harness.state, ms("2026-11-01T02:00:00Z"))
        .await
        .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM notification_outbox")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

/// An incident of a frozen period that resolves within its target sends nothing.
#[tokio::test]
async fn an_incident_resolved_in_time_after_the_freeze_sends_nothing() {
    let f = Fixture::manual().await;
    f.sla(
        "Acme hosting",
        json!({ "times": { "major_outage": { "response_minutes": 120, "resolution_minutes": 240 } } }),
    )
    .await;
    f.incident("2026-10-31T22:30:00Z", Some("2026-11-01T00:20:00Z"))
        .await;

    f.catch_up(OCTOBER_FROZEN).await;
    f.catch_up("2026-11-01T06:00:00Z").await;
    assert!(f.outbox().await.is_empty());
}

/// A per-monitor breach names its monitor; every SLA row names the SLA and no transition.
#[tokio::test]
async fn a_per_monitor_breach_carries_its_monitor() {
    let f = Fixture::manual().await;
    let sla: i64 = f.contract().await.parse().unwrap();
    f.october(0).await;
    f.draft_incident().await;
    f.catch_up(NOW).await;

    let api: i64 = f.monitors[0].parse().unwrap();
    for (event, monitor, sla_id, transition) in f.outbox().await {
        let expected = (event == "sla_availability_breach").then_some(api);
        assert_eq!(monitor, expected, "{event}");
        assert_eq!(sla_id, Some(sla), "{event}");
        assert_eq!(transition, None, "{event}");
    }
}

/// A latency breach waits for the period to close and is then queued once.
#[tokio::test]
async fn a_latency_breach_waits_for_the_period_to_close() {
    let f = Fixture::new(json!({ "type": "http", "url": "http://127.0.0.1:9/" })).await;
    f.sla("Acme latency", json!({ "latency_ms": 10 })).await;
    f.october(1).await;

    f.catch_up(NOW).await;
    assert!(f.outbox().await.is_empty());
    f.catch_up(OCTOBER_FROZEN).await;
    f.catch_up(OCTOBER_FROZEN).await;
    let web: i64 = f.monitors[1].parse().unwrap();
    let rows = f.outbox().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "sla_latency_breach");
    assert_eq!(rows[0].1, Some(web));
}

/// The public snapshot carries the SLA's targets and aggregates for the running period and the
/// frozen September, and nothing else: no incident title, no draft, no internal field.
#[tokio::test]
async fn the_public_snapshot_shows_sla_aggregates_only() {
    let f = Fixture::manual().await;
    let sla = f.contract().await;
    f.october(0).await;
    f.draft_incident().await;
    f.catch_up(NOW).await;
    f.page(&[&sla]).await;

    let text = f.snapshot(None).await;
    assert!(!text.contains(INCIDENT_TITLE));
    let snapshot: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(snapshot["audience"], "public");
    assert_eq!(snapshot["incidents"], json!([]));
    let shown = &snapshot["slas"][0];
    assert_eq!(shown["name"], "Acme hosting");
    let current = &shown["current"];
    assert_eq!(current["start"], "2026-10-01");
    assert_eq!(current["outcome"], "missed");
    assert_eq!(current["availability"]["target"], 99.9);
    assert_eq!(current["availability"]["outcome"], "missed");
    assert!(current["availability"]["actual"].as_f64().unwrap() < 99.9);
    assert_eq!(current["response"]["targets"]["major_outage"], 30);
    assert_eq!(current["response"]["counted"], 1);
    assert_eq!(current["response"]["within_target"], 0.0);
    assert_eq!(current["resolution"]["targets"]["major_outage"], 240);
    assert_eq!(shown["history"][0]["start"], "2026-09-01");

    let mut found = HashSet::new();
    keys(&snapshot["slas"], &mut found);
    let allowed: HashSet<String> = [
        "name",
        "timezone",
        "period",
        "combine",
        "current",
        "history",
        "start",
        "starts_at",
        "ends_at",
        "outcome",
        "availability",
        "latency",
        "response",
        "resolution",
        "target",
        "actual",
        "budget_used",
        "targets",
        "major_outage",
        "partial_outage",
        "degraded",
        "counted",
        "met",
        "within_target",
        "average_ms",
        "target_ms",
        "actual_ms",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let extra: Vec<&String> = found.difference(&allowed).collect();
    assert!(extra.is_empty(), "unexpected fields {extra:?}");
}

/// Grant holders get per-monitor rows, by the page components that show the monitors.
#[tokio::test]
async fn the_internal_snapshot_has_per_monitor_rows() {
    let f = Fixture::manual().await;
    let sla = f.contract().await;
    f.october(0).await;
    f.page(&[&sla]).await;

    let snapshot: Value = serde_json::from_str(&f.snapshot(Some(&f.staff)).await).unwrap();
    assert_eq!(snapshot["audience"], "internal");
    let components: Vec<&Value> = snapshot["groups"][0]["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|component| &component["id"])
        .collect();
    let rows = snapshot["slas"][0]["current"]["monitors"]
        .as_array()
        .unwrap();
    let named: Vec<&Value> = rows.iter().map(|row| &row["component"]).collect();
    assert_eq!(named, components);
    assert_eq!(rows[0]["availability_outcome"], "missed");
    assert_eq!(rows[1]["availability_outcome"], "no_data");
}

/// Only the SLAs selected on the page appear.
#[tokio::test]
async fn an_sla_not_selected_on_the_page_is_absent() {
    let f = Fixture::manual().await;
    let shown = f.contract().await;
    f.sla("Hidden terms", json!({ "availability": 99.0 })).await;
    f.page(&[&shown]).await;

    let snapshot: Value = serde_json::from_str(&f.snapshot(Some(&f.staff)).await).unwrap();
    let names: Vec<&Value> = snapshot["slas"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sla| &sla["name"])
        .collect();
    assert_eq!(names, [&json!("Acme hosting")]);
}
