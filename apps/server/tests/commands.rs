//! Command protection, idempotency and request validation.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{HOST, Harness, ORIGIN, PASSWORD};
use serde_json::json;

async fn sessions(harness: &Harness) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap()
}

#[tokio::test]
async fn commands_need_the_request_marker_and_a_same_origin_origin() {
    let harness = Harness::new().await;
    let body =
        json!({ "idempotency_key": "k", "email": "member@example.test", "password": PASSWORD })
            .to_string();
    let request = |origin: Option<&str>, marker: bool| {
        let mut builder = Request::post("/api/commands/auth.login")
            .header(header::HOST, HOST)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        if marker {
            builder = builder.header("x-v1per-request", "1");
        }
        builder.body(Body::from(body.clone())).unwrap()
    };

    for (origin, marker) in [
        (Some(ORIGIN), false),
        (Some("https://evil.example"), true),
        (None, true),
    ] {
        let answer = harness.send(request(origin, marker)).await;
        assert_eq!(answer.status, StatusCode::FORBIDDEN);
        assert_eq!(answer.code(), "origin_rejected");
    }
    assert_eq!(sessions(&harness).await, 0);
    assert_eq!(
        harness.send(request(Some(ORIGIN), true)).await.status,
        StatusCode::OK
    );
}

/// The same key and arguments return the stored answer and start no second session.
#[tokio::test]
async fn same_key_and_arguments_replay_the_stored_answer() {
    let harness = Harness::new().await;
    let first = harness.login("K1", "member@example.test", PASSWORD).await;
    let second = harness.login("K1", "member@example.test", PASSWORD).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.json());
    assert_eq!(first.json()["revision"], 0);
    assert_eq!(second.bytes, first.bytes);
    assert!(second.header("set-cookie").is_empty());
    assert_eq!(sessions(&harness).await, 1);
}

#[tokio::test]
async fn reused_key_with_other_arguments_is_a_conflict() {
    let harness = Harness::new().await;
    harness.login("K2", "member@example.test", PASSWORD).await;
    let conflict = harness.login("K2", "admin@example.test", PASSWORD).await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
    assert_eq!(conflict.code(), "idempotency_conflict");
}

/// A failed command stores nothing, so a retry with the same key runs again.
#[tokio::test]
async fn failed_command_stores_nothing_under_its_key() {
    let harness = Harness::new().await;
    let failed = harness
        .login("K3", "member@example.test", "wrong password")
        .await;
    assert_eq!(failed.code(), "invalid_credentials");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM idempotency_keys")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(count, 0);

    let retried = harness
        .login("K3", "member@example.test", "wrong password")
        .await;
    assert_eq!(retried.code(), "invalid_credentials");
}

#[tokio::test]
async fn malformed_and_oversized_bodies_are_typed_errors() {
    let harness = Harness::new().await;

    let no_key = json!({ "email": "member@example.test", "password": PASSWORD });
    assert_eq!(
        harness.command(None, "auth.login", no_key).await.code(),
        "malformed_request"
    );

    let wrong_type = json!({ "idempotency_key": "k", "email": 7, "password": PASSWORD });
    assert_eq!(
        harness.command(None, "auth.login", wrong_type).await.code(),
        "malformed_request"
    );

    let huge =
        json!({ "idempotency_key": "k", "email": "x".repeat(70 * 1024), "password": PASSWORD });
    let answer = harness.command(None, "auth.login", huge).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.code(), "payload_too_large");
}

#[tokio::test]
async fn person_commands_refuse_anonymous_callers() {
    let harness = Harness::new().await;
    let answer = harness
        .command(None, "auth.logout", json!({ "idempotency_key": "k" }))
        .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "unauthenticated");
}
