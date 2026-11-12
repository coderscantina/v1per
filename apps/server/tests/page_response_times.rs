//! `<base>/response-times.json`: series per probe or probe location by audience, percentiles,
//! steps per range, incident and maintenance marks, and the 404 and 400 answers.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::DateTime;
use common::{Answer, HOST, Harness};
use serde_json::{Value, json};
use v1per_storage::{Heartbeat, MonitorId, MonitorStatus, ProbeId, Status};

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;

async fn get(harness: &Harness, path: &str, cookie: Option<&str>) -> Answer {
    let mut request = Request::get(path).header(header::HOST, HOST);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    harness.send(request.body(Body::empty()).unwrap()).await
}

fn wire_date(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .unwrap()
        .format("%Y-%m-%dT%H:%M")
        .to_string()
}

/// A public page showing `api`, checked by four probes, a static component and an internal
/// monitor, with three hours of checks, a published and a draft incident and a past window.
struct Setup {
    harness: Harness,
    admin: String,
    page: Value,
    api: String,
    email: String,
    worker: String,
}

impl Setup {
    async fn new() -> Self {
        let harness = Harness::manual().await;
        let admin = harness.session(&harness.admin).await;
        let root = harness.root().await;
        let mut probes = Vec::new();
        for (name, location) in [
            (
                "vie-a",
                json!({ "country": "AT", "city": "Vienna", "provider": "Hetzner" }),
            ),
            ("vie-b", json!({ "country": "AT", "city": "Vienna" })),
            ("fra", json!({ "country": "DE" })),
            ("lab", json!({ "city": "Graz" })),
        ] {
            let created = harness
                .ok(
                    &admin,
                    "probes.create",
                    json!({ "node": root, "name": name, "location": location }),
                )
                .await;
            probes.push(created["probe"]["id"].as_str().unwrap().to_owned());
        }
        let monitor = |name: &str, settings: Value, probes: &[String]| {
            let body = json!({ "node": root, "name": name, "interval": 60, "settings": settings, "probes": probes });
            let (harness, admin) = (&harness, &admin);
            async move {
                harness.ok(admin, "monitors.create", body).await["monitor"]["id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            }
        };
        let http = json!({ "type": "http", "url": "https://api-secret.invalid/health" });
        let api = monitor("api", http.clone(), &probes).await;
        let worker = monitor("worker", http, &probes[..1]).await;
        let component = harness
            .ok(
                &admin,
                "components.create",
                json!({ "node": root, "name": "Email" }),
            )
            .await["component"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let created = harness
            .ok(
                &admin,
                "pages.create",
                json!({ "node": root, "slug": "acme", "title": "Acme" }),
            )
            .await;
        let page = harness
            .ok(
                &admin,
                "pages.updateLayout",
                json!({
                    "page": created["page"]["id"],
                    "expected_revision": created["page"]["revision"],
                    "groups": [{ "name": "Services", "components": [
                        { "target": { "type": "monitor", "monitor": api }, "display_name": "API" },
                        { "target": { "type": "component", "component": component } },
                        { "target": { "type": "monitor", "monitor": worker }, "internal": true },
                    ] }],
                }),
            )
            .await;
        let page_id = page["page"]["id"].clone();
        harness
            .ok(
                &admin,
                "pages.updateAccess",
                json!({ "page": page_id, "public": true }),
            )
            .await;
        let ids: Vec<String> = page["groups"][0]["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|component| component["id"].as_str().unwrap().to_owned())
            .collect();

        // Every ten minutes for three hours: vie-a 10 ms, vie-b 30, fra 50, lab 70, and the
        // monitor series 40.
        let now = harness.state.clock.now_ms();
        let monitor_id = MonitorId(api.parse().unwrap());
        let mut beats = Vec::new();
        let mut statuses = Vec::new();
        for step in 1..=18 {
            let at = now - step * 10 * MINUTE_MS;
            for (index, probe) in probes.iter().enumerate() {
                beats.push(Heartbeat {
                    monitor: monitor_id,
                    probe: Some(ProbeId(probe.parse().unwrap())),
                    time_ms: at,
                    status: Status::Up,
                    latency_ms: Some(10 + 20 * index as u32),
                    message: String::new(),
                    retries: 0,
                    http_status: Some(200),
                    cert_expiry_ms: None,
                });
            }
            statuses.push(MonitorStatus {
                monitor: monitor_id,
                time_ms: at,
                status: Status::Up,
                latency_ms: Some(40),
            });
        }
        let storage = &harness.state.storage;
        storage
            .admit_evaluated(None, &beats, &statuses)
            .await
            .unwrap();
        storage.drain().await;

        let target = json!({ "type": "monitor", "monitor": api });
        // Manual incidents are published to the pages showing their targets.
        harness
            .ok(
                &admin,
                "incidents.create",
                json!({ "node": root, "title": "API slow", "message": "Looking into it",
                        "impacts": [{ "target": target, "impact": "degraded" }] }),
            )
            .await;
        let draft = harness
            .ok(
                &admin,
                "incidents.create",
                json!({ "node": root, "title": "Secret draft",
                        "impacts": [{ "target": target, "impact": "major_outage" }] }),
            )
            .await["incident"]["id"]
            .clone();
        harness
            .ok(
                &admin,
                "incidents.unpublish",
                json!({ "incident": draft, "page": page_id }),
            )
            .await;
        harness
            .ok(
                &admin,
                "maintenance.create",
                json!({ "node": root, "title": "Database upgrade", "monitors": [api],
                        "schedule": { "strategy": "single", "timezone": "UTC",
                                      "start_date": wire_date(now - 2 * HOUR_MS),
                                      "end_date": wire_date(now - HOUR_MS) } }),
            )
            .await;
        Self {
            harness,
            admin,
            page,
            api: ids[0].clone(),
            email: ids[1].clone(),
            worker: ids[2].clone(),
        }
    }

    async fn settings(&self, response_times: bool, probe_locations: bool) {
        self.update(
            json!({ "response_times": response_times, "probe_locations": probe_locations }),
        )
        .await;
    }

    async fn update(&self, settings: Value) {
        let mut args = json!({ "page": self.page["page"]["id"], "slug": "acme", "title": "Acme" });
        args.as_object_mut()
            .unwrap()
            .extend(settings.as_object().unwrap().clone());
        self.harness.ok(&self.admin, "pages.update", args).await;
    }

    async fn read(&self, component: &str, range: &str, internal: bool) -> Answer {
        let path = format!("/s/acme/response-times.json?component={component}&range={range}");
        get(
            &self.harness,
            &path,
            internal.then_some(self.admin.as_str()),
        )
        .await
    }
}

fn labels(answer: &Value) -> Vec<&str> {
    answer["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|series| series["label"].as_str().unwrap())
        .collect()
}

/// Grant holders get a series per probe by name; visitors only per location, merged by city
/// and country, without probes that have no country, and only when the page shows both.
#[tokio::test]
async fn series_follow_the_audience_and_merge_locations() {
    let setup = Setup::new().await;
    let internal = setup.read(&setup.api, "24h", true).await.json();
    assert_eq!(labels(&internal), ["vie-a", "vie-b", "fra", "lab"]);
    let places: Vec<&Value> = internal["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|series| &series["place"])
        .collect();
    assert_eq!(
        places,
        [
            &json!("Vienna, AT"),
            &json!("Vienna, AT"),
            &json!("DE"),
            &json!("Graz")
        ]
    );
    assert_eq!(internal["overall"]["avg_ms"], 40.0);
    assert!(internal["overall"].get("label").is_none());

    assert_eq!(
        setup.read(&setup.api, "24h", false).await.status,
        StatusCode::NOT_FOUND
    );
    setup.settings(true, false).await;
    let public = setup.read(&setup.api, "24h", false).await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(public.json()["series"], json!([]));

    setup.settings(true, true).await;
    let public = setup.read(&setup.api, "24h", false).await;
    let body = String::from_utf8_lossy(&public.bytes).into_owned();
    let public = public.json();
    assert_eq!(labels(&public), ["DE", "Vienna, AT"]);
    let vienna = &public["series"][1];
    assert_eq!(vienna["country"], "AT");
    assert_eq!(vienna["avg_ms"], 20.0);
    assert_eq!(vienna["p50_ms"], 20.0);
    assert_eq!(vienna["points"][0]["min_ms"], 10.0);
    assert_eq!(vienna["points"][0]["max_ms"], 30.0);
    // The probe without a country still counts overall: 10, 30, 50 and 70 ms.
    assert_eq!(public["overall"]["avg_ms"], 40.0);
    assert_eq!(public["overall"]["p50_ms"], 40.0);
    for leak in [
        "vie-a",
        "vie-b",
        "fra",
        "Graz",
        "Hetzner",
        "api-secret",
        "\"probe",
    ] {
        assert!(!body.contains(leak), "a public answer contains {leak}");
    }
}

/// Each range has its own step, so a series stays under about 200 points.
#[tokio::test]
async fn ranges_pick_their_step() {
    let setup = Setup::new().await;
    for (range, step, span) in [
        ("24h", HOUR_MS, 24 * HOUR_MS),
        ("7d", HOUR_MS, 7 * 24 * HOUR_MS),
        ("30d", 4 * HOUR_MS, 30 * 24 * HOUR_MS),
        ("90d", 24 * HOUR_MS, 90 * 24 * HOUR_MS),
    ] {
        let answer = setup.read(&setup.api, range, true).await.json();
        assert_eq!(answer["range"], range);
        assert_eq!(answer["step"], step, "{range}");
        let (from, to) = (
            answer["from"].as_i64().unwrap(),
            answer["to"].as_i64().unwrap(),
        );
        assert_eq!(to - from, span, "{range}");
        assert_eq!(to % step, 0, "{range}");
        let points = answer["overall"]["points"].as_array().unwrap();
        assert!(!points.is_empty() && points.len() as i64 <= span / step);
    }
}

/// Marks are the published incidents on the component and the maintenance windows that
/// covered it, never a draft.
#[tokio::test]
async fn marks_show_published_incidents_and_maintenance() {
    let setup = Setup::new().await;
    setup.settings(true, false).await;
    let answer = setup.read(&setup.api, "24h", false).await.json();
    let marks = answer["marks"].as_array().unwrap();
    let now = setup.harness.state.clock.now_ms();
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert_eq!(marks[0]["kind"], "maintenance");
    assert_eq!(marks[0]["title"], "Database upgrade");
    assert_eq!(
        marks[0]["to"].as_i64().unwrap() - marks[0]["from"].as_i64().unwrap(),
        HOUR_MS
    );
    assert!(marks[0].get("incident").is_none());
    assert_eq!(marks[1]["kind"], "incident");
    assert_eq!(marks[1]["title"], "API slow");
    assert_eq!(marks[1]["impact"], "degraded");
    assert!(marks[1]["incident"].is_string());
    assert!(marks[1].get("to").is_none());
    assert!(marks[1]["from"].as_i64().unwrap() <= now);
}

/// Unknown, static and hidden components are `404`; a bad range or no component is
/// `invalid_arguments`.
#[tokio::test]
async fn refuses_what_the_audience_does_not_get() {
    let setup = Setup::new().await;
    setup.settings(true, true).await;
    for component in [setup.email.as_str(), setup.worker.as_str(), "999999", "x"] {
        let answer = setup.read(component, "7d", false).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{component}");
    }
    assert_eq!(
        setup.read(&setup.worker, "7d", true).await.status,
        StatusCode::OK
    );
    let bad = setup.read(&setup.api, "1y", false).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.code(), "invalid_arguments");
    let missing = get(&setup.harness, "/s/acme/response-times.json?range=7d", None).await;
    assert_eq!(missing.code(), "invalid_arguments");
}

/// Visitors get no range longer than the page's `public_range`; grant holders always get 90
/// days, and the snapshot's bars stop where the visitor's range does.
#[tokio::test]
async fn visitors_stop_at_the_public_range() {
    let setup = Setup::new().await;
    setup
        .update(json!({ "response_times": true, "public_range": "7d" }))
        .await;
    for (range, status) in [
        ("24h", StatusCode::OK),
        ("7d", StatusCode::OK),
        ("30d", StatusCode::NOT_FOUND),
        ("90d", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            setup.read(&setup.api, range, false).await.status,
            status,
            "{range}"
        );
    }
    assert_eq!(
        setup.read(&setup.api, "90d", true).await.status,
        StatusCode::OK
    );

    let public = get(&setup.harness, "/s/acme/snapshot.json", None)
        .await
        .json();
    assert_eq!(public["range_max"], "7d");
    let api = &public["groups"][0]["components"][0];
    assert_eq!(api["bars"].as_array().unwrap().len(), 7);
    assert_eq!(api["hours"].as_array().unwrap().len(), 24);
    // Three hours of checks end in the current hour.
    assert!(api["hours"][23]["up"].as_u64().is_some() || api["hours"][22]["up"].as_u64().is_some());

    let internal = get(&setup.harness, "/s/acme/snapshot.json", Some(&setup.admin))
        .await
        .json();
    assert_eq!(internal["range_max"], "90d");
    assert_eq!(
        internal["groups"][0]["components"][0]["bars"]
            .as_array()
            .unwrap()
            .len(),
        90
    );
}
