//! `GET /api/subscribe`: topic access, catch-up from a cursor, live delivery and the end of a
//! stream when its session ends.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{Frames, HOST, Harness};
use serde_json::json;
use tower::ServiceExt;
use v1per_protocol::envelope::Topic;

#[tokio::test]
async fn subscribing_needs_a_session_and_access_to_the_topic() {
    let harness = Harness::new().await;
    let member = harness.session(&harness.member).await;
    let refused = |response: axum::response::Response| response.status();

    assert_eq!(
        refused(harness.subscribe(None, "topic=instance").await),
        StatusCode::UNAUTHORIZED
    );
    let others = format!("topic=user%3A{}", harness.admin);
    assert_eq!(
        refused(harness.subscribe(Some(&member), &others).await),
        StatusCode::NOT_FOUND
    );
    for query in [
        "topic=node%3An1",
        "topic=instance&cursor=user%3Ax%401",
        "topic=instance&cursor=instance%40-1",
    ] {
        assert_eq!(
            refused(harness.subscribe(Some(&member), query).await),
            StatusCode::BAD_REQUEST,
            "{query}"
        );
    }

    let foreign = Request::get("/api/subscribe?topic=instance")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "https://evil.example")
        .header(header::COOKIE, &member)
        .body(Body::empty())
        .unwrap();
    let response = harness.router().oneshot(foreign).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// A cursor behind the head gets the missed events at once, then events as they commit.
#[tokio::test]
async fn a_stream_catches_up_from_its_cursor_then_follows_the_topic() {
    let harness = Harness::new().await;
    let member = harness.session(&harness.member).await;
    let topic = Topic::user(&harness.member);
    harness.record(&topic, "account.updated").await;
    harness.record(&topic, "account.updated").await;

    let query = format!("topic=user%3A{0}&cursor=user%3A{0}%400", harness.member);
    let response = harness.subscribe(Some(&member), &query).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    let mut frames = Frames::new(response);
    let cursor = |seq: i64| format!("user:{}@{seq}", harness.member);
    let name = topic.as_str();
    assert_eq!(
        frames.next(2).await,
        [
            json!({ "topic": name, "kind": "subscribed", "head": cursor(2), "cursor": cursor(0) }),
            json!({ "topic": name, "kind": "events", "cursor": cursor(2), "events": [
                { "seq": 1, "kind": "account.updated" },
                { "seq": 2, "kind": "account.updated" },
            ] }),
        ]
    );

    harness.record(&Topic::instance(), "instance.renamed").await;
    harness.record(&topic, "account.renamed").await;
    assert_eq!(
        frames.next(1).await,
        [
            json!({ "topic": name, "kind": "events", "cursor": cursor(3), "events": [
            { "seq": 3, "kind": "account.renamed" },
        ] })
        ]
    );
}

/// A cursor the log never reached, as after a restore, needs a fresh snapshot.
#[tokio::test]
async fn a_cursor_ahead_of_the_head_is_expired() {
    let harness = Harness::new().await;
    let member = harness.session(&harness.member).await;
    let response = harness
        .subscribe(Some(&member), "topic=instance&cursor=instance%409")
        .await;
    let mut frames = Frames::new(response);
    assert_eq!(
        frames.next(2).await,
        [
            json!({ "topic": "instance", "kind": "subscribed", "head": "instance@0", "cursor": "instance@9" }),
            json!({ "topic": "instance", "kind": "expired", "head": "instance@0" }),
        ]
    );
    assert!(frames.ends().await);
}

/// The frame after a sign-out is not delivered; the stream ends with the last cursor.
#[tokio::test]
async fn an_ended_session_ends_the_stream_with_resync() {
    let harness = Harness::new().await;
    let member = harness.session(&harness.member).await;
    let mut frames = Frames::new(harness.subscribe(Some(&member), "topic=instance").await);
    assert_eq!(frames.next(1).await[0]["kind"], "subscribed");

    harness.record(&Topic::instance(), "instance.renamed").await;
    assert_eq!(frames.next(1).await[0]["cursor"], "instance@1");

    let logout = harness
        .command(
            Some(&member),
            "auth.logout",
            json!({ "idempotency_key": "out" }),
        )
        .await;
    assert_eq!(logout.status, StatusCode::OK);
    harness.record(&Topic::instance(), "instance.renamed").await;
    assert_eq!(
        frames.next(1).await,
        [
            json!({ "topic": "instance", "kind": "resync", "cursor": "instance@1", "reason": "unavailable" })
        ]
    );
    assert!(frames.ends().await);
}

/// One stream carries several topics, each from its own cursor; an expired topic ends alone.
#[tokio::test]
async fn one_stream_follows_several_topics() {
    let harness = Harness::new().await;
    // The admin owns the root node, `node:1`.
    let admin = harness.session(&harness.admin).await;
    let user = Topic::user(&harness.admin);
    harness.record(&user, "account.updated").await;
    harness.record(&Topic::instance(), "instance.renamed").await;
    harness.record(&Topic::instance(), "instance.renamed").await;

    let name = user.as_str().to_owned();
    let query = format!(
        "topic={}%400&topic=instance%401&topic=node%3A1%409",
        name.replace(':', "%3A")
    );
    let response = harness.subscribe(Some(&admin), &query).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut frames = Frames::new(response);
    let mut opened = frames.next(5).await;
    let caught_up = opened.split_off(3);
    assert_eq!(
        opened,
        [
            json!({ "topic": name, "kind": "subscribed", "head": format!("{name}@1"), "cursor": format!("{name}@0") }),
            json!({ "topic": "instance", "kind": "subscribed", "head": "instance@2", "cursor": "instance@1" }),
            json!({ "topic": "node:1", "kind": "subscribed", "head": "node:1@0", "cursor": "node:1@9" }),
        ]
    );
    // The expired topic ends first; catch-up of the other two follows.
    assert_eq!(
        caught_up[0],
        json!({ "topic": "node:1", "kind": "expired", "head": "node:1@0" })
    );
    assert_eq!(caught_up[1]["cursor"], format!("{name}@1"));
    assert_eq!(
        frames.next(1).await,
        [
            json!({ "topic": "instance", "kind": "events", "cursor": "instance@2", "events": [
            { "seq": 2, "kind": "instance.renamed" },
        ] })
        ]
    );

    harness.record(&Topic::instance(), "instance.renamed").await;
    assert_eq!(frames.next(1).await[0]["cursor"], "instance@3");
    harness.record(&user, "account.renamed").await;
    assert_eq!(frames.next(1).await[0]["cursor"], format!("{name}@2"));
}

/// A topic the caller cannot read refuses the whole request, like an unknown one.
#[tokio::test]
async fn one_unreadable_topic_refuses_the_stream() {
    let harness = Harness::new().await;
    let member = harness.session(&harness.member).await;
    let others = format!("topic=instance&topic=user%3A{}", harness.admin);
    let response = harness.subscribe(Some(&member), &others).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
