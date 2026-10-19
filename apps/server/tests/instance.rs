//! Instance settings: for instance admins, with the server configuration winning.

mod common;

use axum::http::StatusCode;
use common::Harness;
use serde_json::json;

#[tokio::test]
async fn settings_are_for_instance_admins_and_configuration_wins() {
    let harness = Harness::new().await;
    let member = harness.session(&harness.member).await;
    let admin = harness.session(&harness.admin).await;
    let update = json!({ "name": "Rockhouse", "public_url": "https://status.example.com/admin/" });

    assert_eq!(
        harness
            .query(Some(&member), "instance.settings", "")
            .await
            .code(),
        "forbidden"
    );
    assert_eq!(
        harness
            .run(Some(&member), "instance.updateSettings", update.clone())
            .await
            .code(),
        "forbidden"
    );
    let bad = harness
        .run(
            Some(&admin),
            "instance.updateSettings",
            json!({ "name": "x", "public_url": "ftp://x" }),
        )
        .await;
    assert_eq!(
        bad.json()["errors"]["public_url"][0]["code"],
        "invalid_format"
    );

    let settings = harness.ok(&admin, "instance.updateSettings", update).await;
    assert_eq!(
        settings,
        json!({
            "name": "Test instance",
            "name_fixed": true,
            "public_url": "https://status.example.com",
            "public_url_fixed": false,
            "timezone": "UTC",
            "tls_expiry_notify_days": [7, 14, 21],
            "revision": 1,
        })
    );
    // The fixed name is neither stored nor needed.
    let stored: Option<String> = sqlx::query_scalar("SELECT name FROM instance_settings")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(stored, None);
    harness
        .ok(
            &admin,
            "instance.updateSettings",
            json!({ "public_url": "https://status.example.com" }),
        )
        .await;
    let info = harness.query(Some(&member), "instance.info", "").await;
    assert_eq!(info.status, StatusCode::OK);
    assert_eq!(info.json()["public_url"], "https://status.example.com");

    let invited = harness
        .ok(
            &admin,
            "members.invite",
            json!({ "node": harness.root().await, "role": "viewer", "email": "a@b.c" }),
        )
        .await;
    let code = invited["secret"]["code"].as_str().unwrap();
    assert_eq!(
        invited["secret"]["link"],
        format!("https://status.example.com/join?code={code}")
    );
}
