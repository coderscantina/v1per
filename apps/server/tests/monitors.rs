//! Monitors end to end: definitions and access through the API, the built-in probe against a
//! local target on a manual clock, evaluation per decision 0008, and Kuma's push URL.

mod common;

use std::future::Future;
use std::time::Duration;

use axum::http::StatusCode;
use common::{Frames, Harness, Target};
use serde_json::{Value, json};
use v1per_protocol::ids::MonitorId;
use v1per_server::monitors::{self, Assignment};

/// Waits in real time until `holds` is true.
async fn eventually<F, Fut>(what: &str, holds: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..1_000 {
        if holds().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

/// The monitor-level state: Kuma status code, retries, time of the latest beat, message.
async fn state(harness: &Harness, monitor: &str) -> Option<(i64, i64, i64, String)> {
    sqlx::query_as(
        "SELECT status, retries, beat_at, message FROM monitor_state WHERE monitor_id = ?",
    )
    .bind(monitor.parse::<i64>().unwrap())
    .fetch_optional(&harness.state.db.reader)
    .await
    .unwrap()
}

/// Waits until the monitor's latest beat is the one at `at`, and returns its state.
async fn beat_at(harness: &Harness, monitor: &str, at: i64) -> (i64, i64, String) {
    eventually(&format!("monitor {monitor} to beat at {at}"), || async {
        state(harness, monitor)
            .await
            .is_some_and(|state| state.2 == at)
    })
    .await;
    let (status, retries, _, message) = state(harness, monitor).await.unwrap();
    (status, retries, message)
}

/// The journal: (previous, status, important, notify, resend), oldest first.
async fn journal(harness: &Harness, monitor: &str) -> Vec<(Option<i64>, i64, bool, bool, bool)> {
    sqlx::query_as(
        "SELECT previous, status, important, notify, resend FROM monitor_transitions
         WHERE monitor_id = ? ORDER BY id",
    )
    .bind(monitor.parse::<i64>().unwrap())
    .fetch_all(&harness.state.db.reader)
    .await
    .unwrap()
}

fn advance(harness: &Harness, seconds: u64) {
    harness.state.clock.advance(Duration::from_secs(seconds));
}

fn now(harness: &Harness) -> i64 {
    harness.state.clock.now_ms()
}

fn http(name: &str, url: &str) -> Value {
    json!({
        "name": name,
        "interval": 20,
        "settings": { "type": "http", "url": url },
    })
}

fn with(mut base: Value, extra: Value) -> Value {
    for (key, value) in extra.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

async fn create(harness: &Harness, cookie: &str, node: &str, monitor: Value) -> String {
    let created = harness
        .ok(
            cookie,
            "monitors.create",
            with(monitor, json!({ "node": node })),
        )
        .await;
    created["monitor"]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn secrets_are_sealed_and_never_answered() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let monitor = http("api", "https://api.example.test/health");
    let monitor = with(
        monitor,
        json!({ "settings": {
            "type": "http",
            "url": "https://api.example.test/health",
            "headers": [
                { "name": "Accept", "value": "text/plain" },
                { "name": "X-Api-Key", "secret": { "value": "key-sekrit-1" } },
            ],
            "auth": {
                "auth_method": "basic",
                "basic_auth_user": "probe",
                "basic_auth_pass": { "value": "pass-sekrit-2" },
            },
        }}),
    );
    let created = harness
        .run(
            Some(&admin),
            "monitors.create",
            with(monitor.clone(), json!({ "node": root })),
        )
        .await;
    assert_eq!(created.status, StatusCode::OK, "{}", created.json());
    let id = created.json()["monitor"]["id"].as_str().unwrap().to_owned();
    let settings = &created.json()["monitor"]["settings"];
    assert_eq!(settings["auth"]["basic_auth_pass"], json!({ "set": true }));
    assert_eq!(settings["headers"][1]["secret"], json!({ "set": true }));
    assert_eq!(settings["headers"][0]["value"], "text/plain");

    let answers = [
        created.bytes,
        harness
            .query(Some(&admin), "monitors.get", &format!("monitor={id}"))
            .await
            .bytes,
        harness
            .query(Some(&admin), "monitors.list", &format!("node={root}"))
            .await
            .bytes,
    ];
    let (settings_column, sealed): (String, Vec<Vec<u8>>) = (
        sqlx::query_scalar("SELECT settings FROM monitors")
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap(),
        sqlx::query_scalar("SELECT sealed FROM monitor_secrets")
            .fetch_all(&harness.state.db.reader)
            .await
            .unwrap(),
    );
    for bytes in answers
        .iter()
        .chain(sealed.iter())
        .chain([settings_column.as_bytes().to_vec()].iter())
    {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("sekrit"), "a secret leaked: {text}");
    }

    // Leaving the values out keeps them; the probe still gets them.
    let mut kept = monitor.clone();
    kept["settings"]["headers"][1]["secret"] = json!({});
    kept["settings"]["auth"]["basic_auth_pass"] = json!({});
    harness
        .ok(
            &admin,
            "monitors.update",
            with(kept, json!({ "monitor": id })),
        )
        .await;
    let mut conn = harness.state.db.reader.acquire().await.unwrap();
    let assignment = monitors::assignment(
        &mut conn,
        &harness.state.secrets,
        MonitorId::new(id.parse().unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    let Assignment::Check(assignment) = assignment else {
        panic!("an http monitor runs a check");
    };
    let v1per_checks::Check::Http(request) = &assignment.definition.check else {
        panic!("an http check");
    };
    assert_eq!(request.headers["X-Api-Key"].expose(), "key-sekrit-1");
    let v1per_checks::HttpAuth::Basic {
        basic_auth_pass, ..
    } = &request.auth
    else {
        panic!("basic auth");
    };
    assert_eq!(basic_auth_pass.expose(), "pass-sekrit-2");

    // A method switch needs the new method's secret.
    let mut bearer = monitor.clone();
    bearer["settings"]["auth"] = json!({ "auth_method": "bearer", "bearer_token": {} });
    let refused = harness
        .run(
            Some(&admin),
            "monitors.update",
            with(bearer, json!({ "monitor": id })),
        )
        .await;
    assert_eq!(refused.code(), "invalid_arguments");
    assert_eq!(
        refused.json()["errors"]["settings.auth.bearer_token"][0]["code"],
        "required"
    );

    // Clearing removes the stored value; the check then sends an empty one.
    let mut cleared = monitor;
    cleared["settings"]["headers"][1]["secret"] = json!({});
    cleared["settings"]["auth"]["basic_auth_pass"] = json!({ "clear": true });
    let answer = harness
        .ok(
            &admin,
            "monitors.update",
            with(cleared.clone(), json!({ "monitor": id })),
        )
        .await;
    let settings = &answer["monitor"]["settings"];
    assert_eq!(settings["auth"]["basic_auth_pass"], json!({ "set": false }));
    assert_eq!(settings["headers"][1]["secret"], json!({ "set": true }));
    let fields: Vec<String> = sqlx::query_scalar("SELECT field FROM monitor_secrets")
        .fetch_all(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(fields, ["header:x-api-key"]);
    let mut conn = harness.state.db.reader.acquire().await.unwrap();
    let Some(Assignment::Check(assignment)) = monitors::assignment(
        &mut conn,
        &harness.state.secrets,
        MonitorId::new(id.parse().unwrap()),
    )
    .await
    .unwrap() else {
        panic!("an http monitor runs a check");
    };
    let v1per_checks::Check::Http(request) = &assignment.definition.check else {
        panic!("an http check");
    };
    let v1per_checks::HttpAuth::Basic {
        basic_auth_pass, ..
    } = &request.auth
    else {
        panic!("basic auth");
    };
    assert_eq!(basic_auth_pass.expose(), "");
    let mut both = cleared;
    both["settings"]["auth"]["basic_auth_pass"] = json!({ "value": "x", "clear": true });
    let contradictory = harness
        .run(
            Some(&admin),
            "monitors.update",
            with(both, json!({ "monitor": id })),
        )
        .await;
    assert_eq!(
        contradictory.json()["errors"]["settings.auth.basic_auth_pass"][0]["code"],
        "invalid_value"
    );
}

#[tokio::test]
async fn invalid_settings_name_each_field() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let refused = harness
        .run(
            Some(&admin),
            "monitors.create",
            json!({
                "node": root,
                "name": " ",
                "interval": 5,
                "timeout": 30,
                "settings": {
                    "type": "http",
                    "url": "ftp://example.test",
                    "accepted_statuscodes": ["299-200"],
                    "auth": { "auth_method": "basic", "basic_auth_user": "u", "basic_auth_pass": {} },
                },
            }),
        )
        .await;
    assert_eq!(refused.code(), "invalid_arguments");
    let errors = &refused.json()["errors"];
    assert_eq!(errors["name"][0]["code"], "required");
    assert_eq!(
        errors["interval"][0],
        json!({ "code": "invalid_value", "limit": 20 })
    );
    assert_eq!(errors["timeout"][0]["code"], "invalid_value");
    assert_eq!(errors["settings.url"][0]["code"], "invalid_value");
    assert_eq!(
        errors["settings.accepted_statuscodes"][0]["code"],
        "invalid_value"
    );
    assert_eq!(
        errors["settings.auth.basic_auth_pass"][0]["code"],
        "required"
    );
}

/// Viewers read, members manage, and nothing outside the caller's subtree exists for them.
#[tokio::test]
async fn monitors_follow_node_scope_and_capabilities() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let a = harness.node(&admin, &root, "a").await;
    let b = harness.node(&admin, &root, "b").await;
    let viewer = harness.person("viewer").await;
    harness.grant(&harness.member, &a, "member").await;
    harness.grant(&viewer, &a, "viewer").await;
    let member = harness.session(&harness.member).await;
    let viewer = harness.session(&viewer).await;

    let push = json!({ "name": "cron", "interval": 60, "settings": { "type": "push", "push_token": "cron-token-1" } });
    let on_a = create(&harness, &member, &a, push).await;
    let on_b = create(&harness, &admin, &b, http("b", "http://b.test")).await;
    let group_on_root = create(
        &harness,
        &admin,
        &root,
        json!({ "name": "all", "interval": 20, "settings": { "type": "group" } }),
    )
    .await;

    let listed = harness
        .query(Some(&viewer), "monitors.list", &format!("node={a}"))
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json()["monitors"][0]["type"], "push");
    let seen_by_viewer = harness
        .query(Some(&viewer), "monitors.get", &format!("monitor={on_a}"))
        .await
        .json();
    assert!(seen_by_viewer["monitor"]["settings"]["push_token"].is_null());
    let seen_by_member = harness
        .query(Some(&member), "monitors.get", &format!("monitor={on_a}"))
        .await
        .json();
    assert_eq!(
        seen_by_member["monitor"]["settings"]["push_token"],
        "cron-token-1"
    );
    let refused = harness
        .run(
            Some(&viewer),
            "monitors.create",
            with(http("x", "http://x.test"), json!({ "node": a })),
        )
        .await;
    assert_eq!(refused.code(), "forbidden");
    let refused = harness
        .run(Some(&viewer), "monitors.pause", json!({ "monitor": on_a }))
        .await;
    assert_eq!(refused.code(), "forbidden");

    // Another subtree and unknown IDs look the same.
    for (name, query) in [
        ("monitors.list", format!("node={b}")),
        ("monitors.get", format!("monitor={on_b}")),
        ("monitors.get", "monitor=999999".to_owned()),
        ("monitors.uptime", format!("monitor={on_b}")),
    ] {
        let hidden = harness.query(Some(&member), name, &query).await;
        assert_eq!(hidden.code(), "not_found", "{name} {query}");
    }
    for (name, body) in [
        ("monitors.delete", json!({ "monitor": on_b })),
        ("monitors.delete", json!({ "monitor": "999999" })),
        (
            "monitors.create",
            with(http("x", "http://x.test"), json!({ "node": b })),
        ),
    ] {
        let hidden = harness.run(Some(&member), name, body).await;
        assert_eq!(hidden.code(), "not_found", "{name}");
    }
    // A group above the member's highest grant is no group to them.
    let grouped = harness
        .run(
            Some(&member),
            "monitors.create",
            with(
                http("x", "http://x.test"),
                json!({ "node": a, "group": group_on_root }),
            ),
        )
        .await;
    assert_eq!(
        grouped.json()["errors"]["group"][0]["code"],
        "invalid_value"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checks_run_on_their_interval_without_drift() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let target = Target::start().await;
    let start = now(&harness);
    let id = create(&harness, &admin, &root, http("site", &target.url)).await;

    // Created while running: checked at once, then every 20 seconds after that.
    assert_eq!(beat_at(&harness, &id, start).await.0, 1);
    for round in 1..=5 {
        advance(&harness, 19);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(target.hits(), round, "no check before the slot");
        advance(&harness, 1);
        beat_at(&harness, &id, start + round as i64 * 20_000).await;
        assert_eq!(target.hits(), round + 1);
    }
    let (started, late) = background.check_lag();
    assert_eq!((started, late), (6, 0));
    background.stop().await;
}

/// Decision 0008's retry rule and journal: two retries, then down, a repeated down alert
/// every second non-important down beat, then recovery.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failures_retry_go_down_and_recover() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let target = Target::start().await;
    target.answer(500);
    let start = now(&harness);
    let id = create(
        &harness,
        &admin,
        &root,
        with(
            http("site", &target.url),
            json!({ "interval": 60, "retry_interval": 20, "max_retries": 2, "resend_interval": 2 }),
        ),
    )
    .await;
    let (down, up, pending) = (0, 1, 2);

    let (status, retries, message) = beat_at(&harness, &id, start).await;
    assert_eq!((status, retries), (pending, 1));
    assert_eq!(message, "Request failed with status code 500");
    advance(&harness, 20);
    assert_eq!(beat_at(&harness, &id, start + 20_000).await.0, pending);
    advance(&harness, 20);
    assert_eq!(beat_at(&harness, &id, start + 40_000).await.0, down);
    // Down waits a full interval again.
    advance(&harness, 20);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(target.hits(), 3);
    advance(&harness, 40);
    assert_eq!(beat_at(&harness, &id, start + 100_000).await.0, down);
    advance(&harness, 60);
    assert_eq!(beat_at(&harness, &id, start + 160_000).await.0, down);
    target.answer(200);
    advance(&harness, 60);
    let (status, retries, _) = beat_at(&harness, &id, start + 220_000).await;
    assert_eq!((status, retries), (up, 0));

    assert_eq!(
        journal(&harness, &id).await,
        [
            (None, pending, true, false, false),
            (Some(pending), down, true, true, false),
            (Some(down), down, false, true, true),
            (Some(down), up, true, true, false),
        ]
    );
    let changes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM topic_events WHERE topic = ? AND kind = 'monitor.status_changed'",
    )
    .bind(format!("monitors:{root}"))
    .fetch_one(&harness.state.db.reader)
    .await
    .unwrap();
    assert_eq!(changes, 3);

    // History: raw beats from the built-in probe, uptime with pending counted as down.
    harness.state.storage.drain().await;
    let page = harness
        .query(
            Some(&admin),
            "monitors.heartbeats",
            &format!("monitor={id}&limit=4"),
        )
        .await
        .json();
    let statuses: Vec<&str> = page["heartbeats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|beat| beat["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["up", "down", "down", "down"]);
    assert!(page["heartbeats"][0]["probe"].is_string());
    let next = page["next_cursor"].as_str().unwrap();
    let rest = harness
        .query(
            Some(&admin),
            "monitors.heartbeats",
            &format!("monitor={id}&cursor={next}"),
        )
        .await
        .json();
    assert_eq!(rest["heartbeats"].as_array().unwrap().len(), 2);
    assert!(rest["next_cursor"].is_null());
    let uptime = harness
        .query(Some(&admin), "monitors.uptime", &format!("monitor={id}"))
        .await
        .json();
    assert!((uptime["uptime_24h"].as_f64().unwrap() - 1.0 / 6.0).abs() < 1e-9);
    let listed = harness
        .query(Some(&admin), "monitors.list", &format!("node={root}"))
        .await
        .json();
    assert_eq!(listed["monitors"][0]["status"], "up");
    assert!(listed["monitors"][0]["latency_ms"].is_number());
    background.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upside_down_monitors_are_up_while_the_target_fails() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let target = Target::start().await;
    target.answer(503);
    let start = now(&harness);
    let id = create(
        &harness,
        &admin,
        &root,
        with(
            http("inverted", &target.url),
            json!({ "upside_down": true }),
        ),
    )
    .await;
    assert_eq!(beat_at(&harness, &id, start).await.0, 1);
    target.answer(200);
    advance(&harness, 20);
    let (status, _, message) = beat_at(&harness, &id, start + 20_000).await;
    assert_eq!((status, message.as_str()), (0, "Flip UP to DOWN"));
    background.stop().await;
}

async fn push(harness: &Harness, path: &str) -> (StatusCode, Value) {
    let answer = harness
        .send(
            axum::http::Request::post(path)
                .header(axum::http::header::HOST, common::HOST)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
    (answer.status, answer.json())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_push_url_answers_like_kuma() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let id = create(
        &harness,
        &admin,
        &root,
        json!({ "name": "cron", "interval": 60, "settings": { "type": "push", "push_token": "tok3n-push" } }),
    )
    .await;

    let not_found = json!({ "ok": false, "msg": "Monitor not found or not active." });
    assert_eq!(
        push(&harness, "/api/push/unknown").await,
        (StatusCode::NOT_FOUND, not_found.clone())
    );
    assert_eq!(
        push(
            &harness,
            "/api/push/tok3n-push?status=up&msg=all%20good&ping=12.6"
        )
        .await,
        (StatusCode::OK, json!({ "ok": true }))
    );
    let pushed_at = now(&harness);
    let (status, _, message) = beat_at(&harness, &id, pushed_at).await;
    assert_eq!((status, message.as_str()), (1, "all good"));
    let latency: Option<i64> =
        sqlx::query_scalar("SELECT latency_ms FROM monitor_state WHERE monitor_id = ?")
            .bind(id.parse::<i64>().unwrap())
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap();
    assert_eq!(latency, Some(13));
    assert_eq!(
        push(&harness, "/api/push/tok3n-push?ping=-1").await,
        (
            StatusCode::NOT_FOUND,
            json!({ "ok": false, "msg": "Invalid ping value. Must be between 0 and 100000000000 ms." })
        )
    );
    advance(&harness, 1);
    assert_eq!(
        push(&harness, "/api/push/tok3n-push?status=down").await.0,
        StatusCode::OK
    );
    let (status, _, message) = beat_at(&harness, &id, now(&harness)).await;
    assert_eq!((status, message.as_str()), (0, "OK"));

    harness
        .ok(&admin, "monitors.pause", json!({ "monitor": id }))
        .await;
    assert_eq!(
        push(&harness, "/api/push/tok3n-push").await,
        (StatusCode::NOT_FOUND, not_found)
    );
    // Guessing tokens is limited per address.
    for _ in 0..30 {
        push(&harness, "/api/push/guess").await;
    }
    assert_eq!(
        push(&harness, "/api/push/guess").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    background.stop().await;
}

/// Kuma's push deadline: the latest beat plus the interval plus one second, then pending every
/// retry interval, then down every interval until a push arrives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_pushes_go_pending_then_down_on_time() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let start = now(&harness);
    let id = create(
        &harness,
        &admin,
        &root,
        json!({
            "name": "cron", "interval": 60, "retry_interval": 20, "max_retries": 1,
            "settings": { "type": "push", "push_token": "deadline-1" },
        }),
    )
    .await;
    advance(&harness, 10);
    push(&harness, "/api/push/deadline-1").await;
    beat_at(&harness, &id, start + 10_000).await;

    // The first round, one interval after creation, finds the push in time.
    advance(&harness, 50);
    advance(&harness, 11);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(state(&harness, &id).await.unwrap().2, start + 10_000);
    advance(&harness, 1);
    let (status, retries, message) = beat_at(&harness, &id, start + 72_000).await;
    assert_eq!(
        (status, retries, message.as_str()),
        (2, 1, "No heartbeat in the time window")
    );
    advance(&harness, 20);
    assert_eq!(beat_at(&harness, &id, start + 92_000).await.0, 0);
    advance(&harness, 60);
    assert_eq!(beat_at(&harness, &id, start + 152_000).await.0, 0);
    advance(&harness, 8);
    push(&harness, "/api/push/deadline-1").await;
    assert_eq!(beat_at(&harness, &id, start + 160_000).await.0, 1);
    background.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn groups_follow_their_direct_members() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let child = harness.node(&admin, &root, "client").await;
    let start = now(&harness);
    let group = create(
        &harness,
        &admin,
        &root,
        json!({ "name": "all", "interval": 20, "settings": { "type": "group" } }),
    )
    .await;
    let (status, _, message) = beat_at(&harness, &group, start).await;
    assert_eq!((status, message.as_str()), (2, "Group empty"));

    let manual = |name: &str| json!({ "name": name, "interval": 20, "group": group, "settings": { "type": "manual" } });
    let a = create(&harness, &admin, &child, manual("a")).await;
    let b = create(&harness, &admin, &root, manual("b")).await;
    assert_eq!(
        beat_at(&harness, &a, start).await,
        (2, 0, "Manual monitoring - No status set".to_owned())
    );
    for (monitor, status, message) in [(&a, "up", "Up"), (&b, "down", "Down")] {
        harness
            .ok(
                &admin,
                "monitors.setManualStatus",
                json!({ "monitor": monitor, "status": status }),
            )
            .await;
        // A new status restarts the monitor's rounds, so it is recorded at once.
        eventually(message, || async {
            state(&harness, monitor)
                .await
                .is_some_and(|state| state.3 == message)
        })
        .await;
    }
    advance(&harness, 20);
    let (status, _, message) = beat_at(&harness, &group, start + 20_000).await;
    assert_eq!((status, message.as_str()), (0, "Child monitors down: b"));

    harness
        .ok(&admin, "monitors.pause", json!({ "monitor": b }))
        .await;
    advance(&harness, 20);
    let (status, _, message) = beat_at(&harness, &group, start + 40_000).await;
    assert_eq!(
        (status, message.as_str()),
        (1, "All children up and running")
    );
    background.stop().await;
}

/// A restarted server keeps each monitor's state and history and checks it again; deleting a
/// node stops its monitors.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn schedules_resume_after_a_restart_and_stop_with_their_node() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let node = harness.node(&admin, &root, "team").await;
    let target = Target::start().await;
    let start = now(&harness);
    let id = create(&harness, &admin, &node, http("site", &target.url)).await;
    beat_at(&harness, &id, start).await;

    advance(&harness, 5);
    let harness = harness.restart(background).await;
    let background = harness.start();
    assert_eq!(state(&harness, &id).await.unwrap().2, start);
    eventually("a check after the restart", || async {
        advance(&harness, 1);
        tokio::time::sleep(Duration::from_millis(20)).await;
        state(&harness, &id).await.unwrap().2 != start
    })
    .await;
    assert_eq!(target.hits(), 2);
    harness.state.storage.drain().await;
    let page = harness
        .query(
            Some(&admin),
            "monitors.heartbeats",
            &format!("monitor={id}"),
        )
        .await
        .json();
    assert_eq!(page["heartbeats"].as_array().unwrap().len(), 2);

    let admin = harness.session(&harness.admin).await;
    harness
        .ok(&admin, "nodes.delete", json!({ "node": node }))
        .await;
    eventually("the monitor's task to stop", || async {
        advance(&harness, 20);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let before = target.hits();
        advance(&harness, 20);
        tokio::time::sleep(Duration::from_millis(100)).await;
        target.hits() == before
    })
    .await;
    background.stop().await;
}

/// A subtree list covers every node below and follows the `monitor-tree` topic; events name
/// their monitor and arrive once the beat behind them is readable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subtree_lists_and_events_name_their_monitor() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let team = harness.node(&admin, &root, "team").await;
    let client = harness.node(&admin, &team, "client").await;
    let push = |token: &str| json!({ "name": token, "interval": 60, "settings": { "type": "push", "push_token": token } });
    let on_root = create(&harness, &admin, &root, push("root-token")).await;
    let on_client = create(&harness, &admin, &client, push("client-token")).await;

    let listed = harness
        .query(
            Some(&admin),
            "monitors.list",
            &format!("node={team}&subtree=true"),
        )
        .await
        .json();
    assert_eq!(listed["monitors"].as_array().unwrap().len(), 1);
    assert_eq!(listed["monitors"][0]["id"], on_client);
    assert_eq!(listed["monitors"][0]["node"], client);
    let whole = harness
        .query(
            Some(&admin),
            "monitors.list",
            &format!("node={root}&subtree=true"),
        )
        .await
        .json();
    assert_eq!(whole["monitors"].as_array().unwrap().len(), 2);
    let only_root = harness
        .query(Some(&admin), "monitors.list", &format!("node={root}"))
        .await
        .json();
    assert_eq!(only_root["monitors"][0]["id"], on_root);
    assert_eq!(only_root["monitors"].as_array().unwrap().len(), 1);

    let cursor = listed["cursor"].as_str().unwrap();
    assert!(cursor.starts_with(&format!("monitor-tree:{team}@")));
    let mut frames = Frames::new(
        harness
            .subscribe(
                Some(&admin),
                &format!("topic=monitor-tree:{team}&cursor={cursor}"),
            )
            .await,
    );
    frames.next(1).await;
    // Read the stream as a browser does, so it waits for wakes instead of catching up late.
    let (received, mut next_frame) = tokio::sync::mpsc::unbounded_channel();
    let reader = tokio::spawn(async move {
        loop {
            let _ = received.send(frames.next(1).await.remove(0));
        }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    harness
        .send(
            axum::http::Request::post("/api/push/client-token")
                .header(axum::http::header::HOST, common::HOST)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
    let frame = next_frame.recv().await.unwrap();
    reader.abort();
    let event = &frame["events"][0];
    assert_eq!(
        (&event["kind"], &event["monitor"]),
        (&json!("monitor.status_changed"), &json!(on_client))
    );
    let page = harness
        .query(
            Some(&admin),
            "monitors.heartbeats",
            &format!("monitor={on_client}"),
        )
        .await
        .json();
    assert_eq!(page["heartbeats"].as_array().unwrap().len(), 1);
    background.stop().await;
}

/// A new push token needs 10 characters and must be free; both refusals look alike, and an
/// imported shorter token may be sent back unchanged.
#[tokio::test]
async fn push_tokens_are_long_unique_and_refused_alike() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let push = |token: &str| {
        json!({ "node": root, "name": "cron", "interval": 60,
                "settings": { "type": "push", "push_token": token } })
    };
    let id = create(&harness, &admin, &root, push("taken-token")).await;

    let mut refusals = Vec::new();
    for token in ["short-tok", "taken-token", "bad token!"] {
        let refused = harness
            .run(Some(&admin), "monitors.create", push(token))
            .await;
        refusals.push(refused.json());
    }
    assert_eq!(
        refusals[0],
        json!({ "error_code": "invalid_arguments", "message": refusals[0]["message"],
                "errors": { "settings.push_token": [{ "code": "invalid_value" }] } })
    );
    assert!(
        refusals.iter().all(|refusal| *refusal == refusals[0]),
        "{refusals:?}"
    );

    // As the import stores it: a 6-character token, kept through an edit that sends it back.
    sqlx::query("UPDATE monitors SET push_token_hash = ? WHERE id = ?")
        .bind(v1per_server::identity::token_digest("kuma42"))
        .bind(id.parse::<i64>().unwrap())
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    let mut kept = push("kuma42");
    kept["monitor"] = json!(id);
    kept.as_object_mut().unwrap().remove("node");
    harness.ok(&admin, "monitors.update", kept).await;
}
