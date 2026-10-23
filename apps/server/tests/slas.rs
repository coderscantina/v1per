//! SLAs (milestone 14): terms by API, who may read and change them, the page selection, and
//! the upgrade of an existing install to the schema that holds them.

mod common;

use common::Harness;
use serde_json::{Value, json};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;
use v1per_server::db::{CONTROL_DB_FILE, MIGRATOR};
use v1per_server::identity::secrets::SecretKey;
use v1per_server::init::DONE_MARKER;
use v1per_server::maintenance::{upgrade, versions};
use v1per_storage::{Budgets, Storage};

/// Staff with Admin on the agency, a client Admin on the client below it, and a Viewer on a
/// sibling client. The client holds two monitors, the agency a channel.
struct Tree {
    harness: Harness,
    staff: String,
    client_admin: String,
    sibling_viewer: String,
    agency: String,
    client: String,
    sibling: String,
    monitors: [String; 2],
    channel: String,
}

async fn tree() -> Tree {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let agency = harness.node(&admin, &root, "agency").await;
    let client = harness.node(&admin, &agency, "client").await;
    let sibling = harness.node(&admin, &agency, "sibling").await;
    harness.grant(&harness.member, &agency, "admin").await;
    let client_admin = harness.person("client").await;
    harness.grant(&client_admin, &client, "admin").await;
    let sibling_viewer = harness.person("sibling").await;
    harness.grant(&sibling_viewer, &sibling, "viewer").await;
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
    let channel = harness
        .ok(
            &admin,
            "channels.create",
            json!({ "node": agency, "name": "ops", "settings": { "type": "discord", "webhook_url": { "value": "http://127.0.0.1:9/hook" } } }),
        )
        .await["channel"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    Tree {
        staff: harness.session(&harness.member).await,
        client_admin: harness.session(&client_admin).await,
        sibling_viewer: harness.session(&sibling_viewer).await,
        harness,
        agency,
        client,
        sibling,
        monitors: monitors.try_into().unwrap(),
        channel,
    }
}

fn terms(tree: &Tree) -> Value {
    json!({
        "node": tree.client,
        "name": "Acme hosting",
        "timezone": "Europe/Vienna",
        "period": "month",
        "start_date": "2026-01-01",
        "combine": "each",
        "availability": 99.9,
        "latency_ms": 800,
        "times": { "major_outage": { "response_minutes": 30, "resolution_minutes": 240 } },
        "hours": [{ "weekday": 1, "start": "08:00", "end": "18:00" }],
        "holidays": ["2026-12-25"],
        "monitors": tree.monitors,
        "channels": [tree.channel],
    })
}

/// Staff create, read, change and delete an SLA; a page on the client shows it until then.
#[tokio::test]
async fn staff_manage_an_sla_and_pages_show_it() {
    let tree = tree().await;
    let h = &tree.harness;
    let created = h.ok(&tree.staff, "slas.create", terms(&tree)).await;
    let sla = created["sla"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["sla"]["node"], tree.client.as_str());
    assert_eq!(created["sla"]["availability"], 99.9);
    assert_eq!(created["sla"]["hours"][0]["end"], "18:00");
    assert_eq!(created["sla"]["monitors"], json!(tree.monitors));
    assert_eq!(created["sla"]["revision"], 1);

    let listed = h
        .query(
            Some(&tree.staff),
            "slas.list",
            &format!("node={}&subtree=true", tree.agency),
        )
        .await
        .json();
    assert_eq!(listed["slas"][0]["id"], sla.as_str());

    let mut update = terms(&tree);
    update["sla"] = json!(sla);
    update["expected_revision"] = json!(1);
    update["combine"] = json!("any_down");
    update["hours"] = json!([{ "weekday": 0, "start": "00:00", "end": "24:00" }]);
    let updated = h.ok(&tree.staff, "slas.update", update.clone()).await;
    assert_eq!(updated["sla"]["combine"], "any_down");
    assert_eq!(updated["sla"]["hours"][0]["end"], "24:00");
    let stale = h.run(Some(&tree.staff), "slas.update", update).await;
    assert_eq!(stale.code(), "stale_revision");
    assert_eq!(stale.json()["current"], 2);

    // The client Admin runs their own page and picks the SLA for it.
    let page = h
        .ok(
            &tree.client_admin,
            "pages.create",
            json!({ "node": tree.client, "slug": "acme", "title": "Acme" }),
        )
        .await["page"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let picked = h
        .ok(
            &tree.client_admin,
            "pages.updateSlas",
            json!({ "page": page, "slas": [sla] }),
        )
        .await;
    assert_eq!(picked["slas"], json!([sla]));

    // A monitor that moves out of the client's subtree leaves the SLA.
    let admin = h.session(&h.admin).await;
    h.ok(
        &admin,
        "monitors.move",
        json!({ "monitor": tree.monitors[1], "node": tree.sibling, "position": 0 }),
    )
    .await;
    let detail = h
        .query(Some(&tree.staff), "slas.get", &format!("sla={sla}"))
        .await
        .json();
    assert_eq!(detail["sla"]["monitors"], json!([tree.monitors[0]]));
    assert_eq!(detail["sla"]["revision"], 3);

    h.ok(&tree.staff, "slas.delete", json!({ "sla": sla }))
        .await;
    let gone = h
        .query(Some(&tree.staff), "slas.get", &format!("sla={sla}"))
        .await;
    assert_eq!(gone.code(), "not_found");
    let page = h
        .query(
            Some(&tree.client_admin),
            "pages.get",
            &format!("page={page}"),
        )
        .await
        .json();
    assert_eq!(page["slas"], json!([]));
}

/// A client Admin reads the SLA on their node and changes nothing; a Viewer on a sibling client
/// gets the same answers as for an SLA that does not exist.
#[tokio::test]
async fn clients_read_their_sla_and_siblings_cannot_tell_it_exists() {
    let tree = tree().await;
    let h = &tree.harness;
    let refused = h
        .run(Some(&tree.client_admin), "slas.create", terms(&tree))
        .await;
    assert_eq!(refused.code(), "forbidden");
    let created = h.ok(&tree.staff, "slas.create", terms(&tree)).await;
    let sla = created["sla"]["id"].as_str().unwrap().to_owned();

    let read = h
        .query(Some(&tree.client_admin), "slas.get", &format!("sla={sla}"))
        .await;
    assert_eq!(read.json()["sla"]["name"], "Acme hosting");
    let mut update = terms(&tree);
    update["sla"] = json!(sla);
    for (name, body) in [
        ("slas.update", update.clone()),
        ("slas.delete", json!({ "sla": sla })),
        (
            "slas.recompute",
            json!({ "sla": sla, "start": "2026-01-01" }),
        ),
    ] {
        let answer = h.run(Some(&tree.client_admin), name, body).await;
        assert_eq!(answer.code(), "forbidden", "{name}");
    }

    let unknown = h
        .query(Some(&tree.sibling_viewer), "slas.get", "sla=999999")
        .await;
    let hidden = h
        .query(
            Some(&tree.sibling_viewer),
            "slas.get",
            &format!("sla={sla}"),
        )
        .await;
    assert_eq!(hidden.code(), "not_found");
    assert_eq!(hidden.bytes, unknown.bytes);
    update["sla"] = json!("999999");
    let unknown = h
        .run(Some(&tree.sibling_viewer), "slas.update", update.clone())
        .await;
    update["sla"] = json!(sla);
    let hidden = h
        .run(Some(&tree.sibling_viewer), "slas.update", update)
        .await;
    assert_eq!(hidden.status, unknown.status);
    assert_eq!(hidden.bytes, unknown.bytes);
    let listed = h
        .query(
            Some(&tree.sibling_viewer),
            "slas.list",
            &format!("node={}", tree.client),
        )
        .await;
    assert_eq!(listed.code(), "not_found");
    let own = h
        .query(
            Some(&tree.sibling_viewer),
            "slas.list",
            &format!("node={}&subtree=true", tree.sibling),
        )
        .await;
    assert_eq!(own.json()["slas"], json!([]));
}

/// Each invalid term is named; monitors outside the subtree and channels off the path too.
#[tokio::test]
async fn invalid_terms_are_refused_by_field() {
    let tree = tree().await;
    let h = &tree.harness;
    let admin = h.session(&h.admin).await;
    let outside = h
        .ok(
            &admin,
            "monitors.create",
            json!({ "node": tree.sibling, "name": "other", "interval": 60, "settings": { "type": "manual" } }),
        )
        .await["monitor"]["id"]
        .clone();
    let foreign_channel = h
        .ok(
            &admin,
            "channels.create",
            json!({ "node": tree.sibling, "name": "theirs", "settings": { "type": "discord", "webhook_url": { "value": "http://127.0.0.1:9/hook" } } }),
        )
        .await["channel"]["id"]
        .clone();

    let mut bad = terms(&tree);
    bad["timezone"] = json!("Mars/Olympus");
    bad["start_date"] = json!("2026-02-30");
    bad["availability"] = json!(99.9999);
    bad["times"]["degraded"] = json!({ "resolution_minutes": 0 });
    bad["hours"] = json!([
        { "weekday": 1, "start": "08:00", "end": "12:00" },
        { "weekday": 1, "start": "11:00", "end": "18:00" },
    ]);
    bad["holidays"] = json!(["25.12.2026"]);
    bad["monitors"] = json!([]);
    let refused = h.run(Some(&tree.staff), "slas.create", bad).await;
    assert_eq!(refused.code(), "invalid_arguments");
    let fields: Vec<String> = refused.json()["errors"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        fields,
        [
            "availability",
            "holidays",
            "hours",
            "monitors",
            "start_date",
            "times.degraded.resolution_minutes",
            "timezone",
        ]
    );

    let mut bad = terms(&tree);
    bad["monitors"] = json!([tree.monitors[0], outside]);
    bad["channels"] = json!([foreign_channel]);
    let refused = h.run(Some(&tree.staff), "slas.create", bad).await;
    let errors = &refused.json()["errors"];
    assert_eq!(errors["monitors"][0]["code"], "invalid_value");
    assert_eq!(errors["channels"][0]["code"], "invalid_value");
}

/// An install at control schema 13 with outbox rows upgrades to 14 unattended: every row and
/// delivery ID stays, the counter does not go back, and the outbox takes SLA events.
#[tokio::test]
async fn an_install_at_schema_13_upgrades_with_its_outbox_intact() {
    let data = TempDir::new().unwrap();
    let dir = data.path();
    SecretKey::load_or_create(dir, None, None).unwrap();
    Storage::open(dir, Budgets::default())
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(dir.join(CONTROL_DB_FILE))
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    MIGRATOR.run_to(13, &pool).await.unwrap();
    for statement in [
        "INSERT INTO monitors (node_id, name, description, type, settings, interval,
             retry_interval, max_retries, resend_interval, timeout, upside_down, quorum, active,
             tags, revision, created_at, updated_at)
         VALUES (1, 'api', '', 'http', '{}', 60, 60, 0, 0, 48, 0, 'majority', 1, '[]', 1, 1, 1)",
        "INSERT INTO monitor_transitions (monitor_id, time, previous, status, message, important,
             notify, resend)
         VALUES (1, 10, 1, 0, 'timeout', 1, 1, 0)",
        "INSERT INTO channels (node_id, name, provider, config, is_default, active, revision,
             created_at, updated_at)
         VALUES (1, 'ops', 'discord', x'00ff', 0, 1, 1, 1, 1)",
        "INSERT INTO notification_outbox (node_id, channel_id, event, monitor_id, transition_id,
             payload, summary, state, attempts, created_at, finished_at)
         VALUES (1, 1, 'monitor_down', 1, 1, '{\"down\":1}', 'api is down', 'delivered', 1, 10, 11)",
        "INSERT INTO notification_outbox (node_id, channel_id, event, monitor_id, payload,
             summary, state, attempts, next_attempt_at, created_at)
         VALUES (1, 1, 'certificate_expiry', 7, '{}', 'expires', 'pending', 2, 99, 12)",
        "INSERT INTO notification_outbox (node_id, event, payload, summary, config, state, error,
             detail, created_at, finished_at)
         VALUES (1, 'test', '{}', 'test', x'0102', 'failed', 'refused', 'HTTP 400', 13, 14)",
        "INSERT INTO notification_outbox (node_id, channel_id, event, payload, summary, state,
             created_at)
         VALUES (1, 1, 'probe_offline', '{}', 'gone', 'suppressed', 15)",
        // The newest row was cleaned up, so the counter is ahead of the last ID.
        "DELETE FROM notification_outbox WHERE id = 4",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    let rows = "SELECT json_group_array(json_array(id, node_id, channel_id, event, monitor_id,
                    transition_id, payload, summary, hex(config), state, attempts,
                    next_attempt_at, error, detail, created_at, finished_at))
                FROM (SELECT * FROM notification_outbox ORDER BY id)";
    let before: String = sqlx::query_scalar(rows).fetch_one(&pool).await.unwrap();
    pool.close().await;
    std::fs::write(dir.join(DONE_MARKER), "").unwrap();
    assert_eq!(versions::stored(dir).await.unwrap().control, 13);

    let backup = TempDir::new().unwrap();
    let upgraded = upgrade::upgrade(dir, Some(&backup.path().join("pre.tar")), || {
        Ok(SecretKey::load(dir, None, None).unwrap())
    })
    .await
    .unwrap();
    assert_eq!(upgraded.state, "upgraded");
    assert_eq!(upgraded.to.control, 14);
    assert_eq!(upgraded.to.heartbeats, upgraded.from.heartbeats);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(dir.join(CONTROL_DB_FILE))
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let after: String = sqlx::query_scalar(rows).fetch_one(&pool).await.unwrap();
    assert_eq!(after, before);
    let broken: Vec<(String,)> = sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(broken.is_empty(), "{broken:?}");
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'notification_outbox'
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        indexes,
        [
            "notification_outbox_channel",
            "notification_outbox_due",
            "notification_outbox_finished",
            "notification_outbox_monitor",
            "notification_outbox_once",
            "notification_outbox_queue",
            "notification_outbox_sla",
        ]
    );
    let next: i64 = sqlx::query_scalar(
        "INSERT INTO notification_outbox (node_id, channel_id, event, payload, summary, state,
             next_attempt_at, created_at, sla_id)
         VALUES (1, 1, 'sla_budget_warning', '{}', 'budget', 'pending', 20, 20, 1) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(next, 5);
    pool.close().await;
}
