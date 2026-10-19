//! Incidents end to end: the monitoring contract's impact rules, access by role, and
//! auto-incidents from monitor transitions.

mod common;

use std::future::Future;
use std::time::Duration;

use common::Harness;
use serde_json::{Value, json};

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

fn monitor_target(id: &str) -> Value {
    json!({ "type": "monitor", "monitor": id })
}

fn component_target(id: &str) -> Value {
    json!({ "type": "component", "component": id })
}

async fn detail(harness: &Harness, cookie: &str, incident: &str) -> Value {
    harness
        .query(
            Some(cookie),
            "incidents.get",
            &format!("incident={incident}"),
        )
        .await
        .json()
}

/// Impacts of the affected targets, in order.
fn impacts(detail: &Value) -> Vec<Value> {
    detail["affected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|target| target["impact"].clone())
        .collect()
}

/// The latest update that names a target sets its impact, resolving makes everything
/// operational, members run incidents in their subtree and viewers only read them.
#[tokio::test]
async fn impact_follows_the_latest_update_naming_each_target() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let team = harness.node(&admin, &root, "team").await;
    let other = harness.node(&admin, &root, "other").await;
    harness.grant(&harness.member, &team, "member").await;
    let member = harness.session(&harness.member).await;
    let viewer_id = harness.person("viewer").await;
    harness.grant(&viewer_id, &team, "viewer").await;
    let viewer = harness.session(&viewer_id).await;

    let created = harness
        .ok(
            &member,
            "monitors.create",
            json!({ "node": team, "name": "api", "interval": 60, "settings": { "type": "manual" } }),
        )
        .await;
    let monitor = created["monitor"]["id"].as_str().unwrap().to_owned();
    let refused = harness
        .run(
            Some(&member),
            "components.create",
            json!({ "node": team, "name": "Email" }),
        )
        .await;
    assert_eq!(refused.code(), "forbidden");
    let created = harness
        .ok(
            &admin,
            "components.create",
            json!({ "node": team, "name": "Email delivery" }),
        )
        .await;
    let component = created["component"]["id"].as_str().unwrap().to_owned();
    let foreign = harness
        .ok(
            &admin,
            "components.create",
            json!({ "node": other, "name": "Elsewhere" }),
        )
        .await;

    let create = json!({
        "node": team, "title": "Slow mail",
        "message": "Looking into it",
        "impacts": [
            { "target": monitor_target(&monitor), "impact": "major_outage" },
            { "target": component_target(&component), "impact": "degraded" },
        ],
    });
    let denied = harness
        .run(Some(&viewer), "incidents.create", create.clone())
        .await;
    assert_eq!(denied.code(), "forbidden");
    let mut outside = create.clone();
    outside["impacts"][1]["target"] =
        component_target(foreign["component"]["id"].as_str().unwrap());
    let invalid = harness
        .run(Some(&member), "incidents.create", outside)
        .await;
    assert_eq!(
        invalid.json()["errors"]["impacts"][0]["code"],
        "invalid_value"
    );
    let created = harness.ok(&member, "incidents.create", create).await;
    let incident = created["incident"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["incident"]["impact"], "major_outage");
    assert_eq!(created["incident"]["origin"], "manual");

    let update = |state: &str, impacts: Value| json!({ "incident": incident, "state": state, "message": "news", "impacts": impacts });
    harness
        .ok(
            &member,
            "incidents.addUpdate",
            update(
                "identified",
                json!([{ "target": component_target(&component), "impact": "partial_outage" }]),
            ),
        )
        .await;
    let seen = detail(&harness, &viewer, &incident).await;
    assert_eq!(
        impacts(&seen),
        [json!("major_outage"), json!("partial_outage")]
    );
    let after = harness
        .ok(
            &member,
            "incidents.addUpdate",
            update(
                "monitoring",
                json!([{ "target": monitor_target(&monitor), "impact": "degraded" }]),
            ),
        )
        .await;
    assert_eq!(after["incident"]["impact"], "partial_outage");
    assert_eq!(after["incident"]["state"], "monitoring");

    let resolved = harness
        .ok(
            &member,
            "incidents.addUpdate",
            update("resolved", json!([])),
        )
        .await;
    assert!(resolved["incident"]["resolved_at"].is_i64());
    let seen = detail(&harness, &viewer, &incident).await;
    assert_eq!(impacts(&seen), [json!("operational"), json!("operational")]);
    let states: Vec<&str> = seen["updates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        ["investigating", "identified", "monitoring", "resolved"]
    );
    assert_eq!(seen["updates"][0]["author"], "Member");
    assert_eq!(seen["affected"][1]["name"], "Email delivery");

    let open = harness
        .query(
            Some(&viewer),
            "incidents.list",
            &format!("node={root}&subtree=true&status=open"),
        )
        .await;
    assert_eq!(open.code(), "not_found");
    let listed = harness
        .query(
            Some(&viewer),
            "incidents.list",
            &format!("node={team}&status=resolved"),
        )
        .await
        .json();
    assert_eq!(listed["incidents"][0]["id"], json!(incident));
    let tree = harness
        .query(
            Some(&admin),
            "incidents.list",
            &format!("node={root}&subtree=true&monitor={monitor}"),
        )
        .await
        .json();
    assert_eq!(tree["incidents"].as_array().unwrap().len(), 1);
}

async fn set_manual(harness: &Harness, cookie: &str, monitor: &str, status: &str) {
    harness
        .ok(
            cookie,
            "monitors.setManualStatus",
            json!({ "monitor": monitor, "status": status }),
        )
        .await;
}

/// Open auto-incidents of `monitor`: (id, state, edited).
async fn auto_incidents(harness: &Harness, cookie: &str, monitor: &str) -> Vec<Value> {
    let root = harness.root().await;
    let listed = harness
        .query(
            Some(cookie),
            "incidents.list",
            &format!("node={root}&subtree=true&origin=auto&monitor={monitor}"),
        )
        .await
        .json();
    listed["incidents"].as_array().unwrap().clone()
}

async fn status(harness: &Harness, monitor: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT status FROM monitor_state WHERE monitor_id = ?")
        .bind(monitor.parse::<i64>().unwrap())
        .fetch_optional(&harness.state.db.reader)
        .await
        .unwrap()
}

/// A down transition opens one auto-incident per monitor; recovery resolves it unless a person
/// changed it, and repeated failures while it is open do not open another.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_incidents_open_once_and_resolve_unless_edited() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let manual = |name: &str| json!({ "node": root, "name": name, "interval": 60, "settings": { "type": "manual" } });
    let quiet = harness.ok(&admin, "monitors.create", manual("quiet")).await["monitor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let handled = harness
        .ok(&admin, "monitors.create", manual("handled"))
        .await["monitor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    for monitor in [&quiet, &handled] {
        set_manual(&harness, &admin, monitor, "down").await;
        eventually("the down beat", || async {
            status(&harness, monitor).await == Some(0)
        })
        .await;
    }
    let opened = auto_incidents(&harness, &admin, &quiet).await;
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0]["title"], "quiet is down");
    assert_eq!(opened[0]["impact"], "major_outage");
    assert_eq!(opened[0]["state"], "investigating");
    let handled_incident = auto_incidents(&harness, &admin, &handled).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    harness
        .ok(
            &admin,
            "incidents.addUpdate",
            json!({ "incident": handled_incident, "state": "identified", "message": "disk full" }),
        )
        .await;

    for monitor in [&quiet, &handled] {
        set_manual(&harness, &admin, monitor, "up").await;
        eventually("the up beat", || async {
            status(&harness, monitor).await == Some(1)
        })
        .await;
    }
    let quiet_incident = &auto_incidents(&harness, &admin, &quiet).await[0];
    assert_eq!(quiet_incident["state"], "resolved");
    let handled_now = &auto_incidents(&harness, &admin, &handled).await[0];
    assert_eq!(handled_now["state"], "identified");
    assert_eq!(handled_now["edited"], true);

    // Failing again while the edited incident is still open adds nothing.
    set_manual(&harness, &admin, &handled, "down").await;
    eventually("the second down beat", || async {
        status(&harness, &handled).await == Some(0)
    })
    .await;
    assert_eq!(auto_incidents(&harness, &admin, &handled).await.len(), 1);
    // The resolved one does not reopen; a new failure opens a new incident.
    set_manual(&harness, &admin, &quiet, "down").await;
    eventually("a new auto-incident", || async {
        auto_incidents(&harness, &admin, &quiet).await.len() == 2
    })
    .await;
    let reopen = harness
        .run(
            Some(&admin),
            "incidents.addUpdate",
            json!({ "incident": quiet_incident["id"], "state": "investigating" }),
        )
        .await;
    assert_eq!(reopen.json()["errors"]["state"][0]["code"], "invalid_value");
    background.stop().await;
}
