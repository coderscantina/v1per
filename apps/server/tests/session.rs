//! Sign-in, sign-out, the current session and the account and instance it reads.

mod common;

use axum::http::StatusCode;
use common::{Harness, PASSWORD};
use serde_json::{Value, json};
use v1per_server::db::now_ms;

#[tokio::test]
async fn login_sets_a_session_and_logout_ends_it() {
    let harness = Harness::new().await;
    let login = harness.login("l1", "Admin@Example.test", PASSWORD).await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.json());
    assert_eq!(login.json()["status"], "signed_in");
    assert_eq!(login.json()["account"]["instance_admin"], true);
    let set_cookie = login.header("set-cookie").to_owned();
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax"));
    let cookie = set_cookie.split(';').next().unwrap().to_owned();

    let session = harness.query(Some(&cookie), "auth.session", "").await;
    assert_eq!(session.json()["account"]["email"], "admin@example.test");
    let account = harness.query(Some(&cookie), "account.get", "").await;
    assert_eq!(account.status, StatusCode::OK);
    assert_eq!(account.json()["account"]["id"], harness.admin.as_str());

    let logout = harness
        .command(
            Some(&cookie),
            "auth.logout",
            json!({ "idempotency_key": "l2" }),
        )
        .await;
    assert_eq!(logout.status, StatusCode::OK);
    assert!(logout.header("set-cookie").contains("Max-Age=0"));
    let after = harness.query(Some(&cookie), "auth.session", "").await;
    assert_eq!(after.status, StatusCode::OK);
    assert_eq!(after.json()["account"], Value::Null);
    assert_eq!(
        harness.query(Some(&cookie), "account.get", "").await.code(),
        "unauthenticated"
    );
}

/// An outsider cannot tell a wrong password from an unknown account.
#[tokio::test]
async fn wrong_password_and_unknown_email_are_indistinguishable() {
    let harness = Harness::new().await;
    let wrong = harness
        .login("w1", "member@example.test", "wrong password")
        .await;
    let unknown = harness
        .login("w2", "nobody@example.test", "wrong password")
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_credentials");
    assert_eq!(wrong.bytes, unknown.bytes);
    assert!(wrong.header("set-cookie").is_empty());
}

#[tokio::test]
async fn logins_are_rate_limited_per_address() {
    let harness = Harness::new().await;
    for attempt in 0..10 {
        let answer = harness
            .login(
                &format!("r{attempt}"),
                "member@example.test",
                "wrong password",
            )
            .await;
        assert_eq!(answer.code(), "invalid_credentials");
    }
    let limited = harness.login("r10", "member@example.test", PASSWORD).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.code(), "rate_limited");
}

/// A session unused for 7 days ends.
#[tokio::test]
async fn an_idle_session_ends() {
    let harness = Harness::new().await;
    let session = harness.session(&harness.member).await;
    sqlx::query("UPDATE sessions SET last_seen_at = ?")
        .bind(now_ms() - 8 * 86_400_000)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    assert_eq!(
        harness
            .query(Some(&session), "account.get", "")
            .await
            .code(),
        "unauthenticated"
    );
}

#[tokio::test]
async fn instance_info_needs_a_session_and_carries_its_cursor() {
    let harness = Harness::new().await;
    assert_eq!(
        harness.query(None, "instance.info", "").await.code(),
        "unauthenticated"
    );

    let cookie = harness.session(&harness.member).await;
    let info = harness.query(Some(&cookie), "instance.info", "").await;
    assert_eq!(info.status, StatusCode::OK);
    assert_eq!(info.json()["name"], "Test instance");
    assert_eq!(info.json()["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info.json()["revision"], 0);
    assert_eq!(info.json()["cursor"], "instance@0");
    assert!(info.json().get("public_url").is_none());

    let account = harness.query(Some(&cookie), "account.get", "").await;
    assert_eq!(account.json()["account"]["instance_admin"], false);
    assert_eq!(
        account.json()["cursor"],
        format!("user:{}@0", harness.member)
    );
}
