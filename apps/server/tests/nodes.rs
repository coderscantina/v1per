//! The node tree: ordering, depth, moves, deletion, refusals and who hears about changes.

mod common;

use axum::http::StatusCode;
use common::{Frames, Harness};
use serde_json::{Value, json};
use v1per_protocol::envelope::Topic;
use v1per_server::events;

/// Names of the readable nodes in `nodes.tree`, depth first.
async fn tree(harness: &Harness, cookie: &str) -> Vec<String> {
    harness.query(Some(cookie), "nodes.tree", "").await.json()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["name"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn nodes_are_created_renamed_and_ordered_among_siblings() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let a = harness.node(&admin, &root, "a").await;
    harness.node(&admin, &root, "b").await;
    let c = harness.node(&admin, &root, "c").await;

    harness
        .ok(&admin, "nodes.reorder", json!({ "node": c, "position": 0 }))
        .await;
    let renamed = harness
        .ok(
            &admin,
            "nodes.rename",
            json!({ "node": a, "name": "  alpha " }),
        )
        .await;
    assert_eq!(renamed["node"]["name"], "alpha");
    assert_eq!(renamed["node"]["position"], 1);
    assert_eq!(tree(&harness, &admin).await, ["Root", "c", "alpha", "b"]);

    let blank = harness
        .run(
            Some(&admin),
            "nodes.create",
            json!({ "parent": root, "name": " " }),
        )
        .await;
    assert_eq!(blank.code(), "invalid_arguments");
    assert_eq!(blank.json()["errors"]["name"][0]["code"], "required");
}

#[tokio::test]
async fn a_node_sits_at_most_eight_levels_below_the_root() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let mut parent = harness.root().await;
    for level in 1..=8 {
        parent = harness
            .node(&admin, &parent, &format!("level {level}"))
            .await;
    }
    let ninth = harness
        .run(
            Some(&admin),
            "nodes.create",
            json!({ "parent": parent, "name": "deep" }),
        )
        .await;
    assert_eq!(ninth.code(), "invalid_arguments");
    assert_eq!(
        ninth.json()["errors"]["parent"][0],
        json!({ "code": "too_many", "limit": 8 })
    );
}

/// Moving needs `node.move` on the node and `node.write` where it leaves and where it lands,
/// never lands below itself, and changes who sees the subtree.
#[tokio::test]
async fn moving_needs_rights_on_both_sides_and_never_goes_below_itself() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let a = harness.node(&admin, &root, "a").await;
    let a1 = harness.node(&admin, &a, "a1").await;
    let b = harness.node(&admin, &root, "b").await;
    let hidden = harness.node(&admin, &root, "hidden").await;

    let mover = harness.person("mover").await;
    harness.grant(&mover, &a1, "owner").await;
    harness.grant(&mover, &b, "admin").await;
    let watcher = harness.person("watcher").await;
    harness.grant(&watcher, &a, "viewer").await;
    let cookie = harness.session(&mover).await;
    let to = |parent: &str| json!({ "node": a1, "parent": parent });

    let leaving = harness.run(Some(&cookie), "nodes.move", to(&b)).await;
    assert_eq!(leaving.code(), "forbidden", "no node.write on a");
    harness.grant(&mover, &a, "admin").await;
    let invisible = harness.run(Some(&cookie), "nodes.move", to(&hidden)).await;
    assert_eq!(invisible.code(), "not_found");
    let below_itself = harness
        .run(
            Some(&admin),
            "nodes.move",
            json!({ "node": a, "parent": a1 }),
        )
        .await;
    assert_eq!(
        below_itself.json()["errors"]["parent"][0]["code"],
        "invalid_value"
    );
    let root_move = harness
        .run(
            Some(&admin),
            "nodes.move",
            json!({ "node": root, "parent": b }),
        )
        .await;
    assert_eq!(
        root_move.json()["errors"]["node"][0]["code"],
        "invalid_value"
    );

    let watcher_cookie = harness.session(&watcher).await;
    assert_eq!(tree(&harness, &watcher_cookie).await, ["a", "a1"]);
    let moved = harness.ok(&cookie, "nodes.move", to(&b)).await;
    assert_eq!(moved["node"]["parent"], b.as_str());
    assert_eq!(tree(&harness, &watcher_cookie).await, ["a"]);
}

/// A node with descendants goes only with their number confirmed, takes its subtree and the
/// grants in it along, and needs a fresh login. The root stays.
#[tokio::test]
async fn deleting_a_node_with_descendants_needs_their_number() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let a = harness.node(&admin, &root, "a").await;
    let a1 = harness.node(&admin, &a, "a1").await;
    harness.node(&admin, &a1, "a2").await;
    let inside = harness.person("inside").await;
    harness.grant(&inside, &a1, "viewer").await;

    let stale = harness.session_reauthenticated(&harness.admin, None).await;
    let refused = harness
        .run(
            Some(&stale),
            "nodes.delete",
            json!({ "node": a, "descendants": 2 }),
        )
        .await;
    assert_eq!(refused.code(), "reauthentication_required");
    for descendants in [Value::Null, json!(1)] {
        let unconfirmed = harness
            .run(
                Some(&admin),
                "nodes.delete",
                json!({ "node": a, "descendants": descendants }),
            )
            .await;
        assert_eq!(unconfirmed.status, StatusCode::CONFLICT);
        assert_eq!(unconfirmed.code(), "node_not_empty");
        assert_eq!(unconfirmed.json()["current"], 2);
    }
    let deleted = harness
        .ok(
            &admin,
            "nodes.delete",
            json!({ "node": a, "descendants": 2 }),
        )
        .await;
    assert_eq!(deleted["descendants"], 2);
    assert_eq!(
        harness
            .query(Some(&admin), "nodes.get", &format!("node={a1}"))
            .await
            .code(),
        "not_found"
    );
    assert!(
        tree(&harness, &harness.session(&inside).await)
            .await
            .is_empty()
    );
    let root_delete = harness
        .run(Some(&admin), "nodes.delete", json!({ "node": root }))
        .await;
    assert_eq!(
        root_delete.json()["errors"]["node"][0]["code"],
        "invalid_value"
    );
}

/// Commands on a hidden node answer byte for byte like commands on a missing one, before any
/// validation could tell them apart.
#[tokio::test]
async fn refusals_do_not_disclose_hidden_nodes() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let mine = harness.node(&admin, &root, "mine").await;
    let theirs = harness.node(&admin, &root, "theirs").await;
    harness.grant(&harness.member, &mine, "owner").await;
    let member = harness.session(&harness.member).await;

    for (name, body) in [
        ("nodes.rename", json!({ "name": "" })),
        ("nodes.create", json!({ "name": "x" })),
        ("nodes.delete", json!({})),
        ("members.invite", json!({ "role": "owner", "email": "x" })),
    ] {
        let field = if name == "nodes.create" {
            "parent"
        } else {
            "node"
        };
        let mut hidden = body.clone();
        hidden[field] = json!(theirs);
        let mut missing = body;
        missing[field] = json!("424242");
        let hidden = harness.run(Some(&member), name, hidden).await;
        let missing = harness.run(Some(&member), name, missing).await;
        assert_eq!(hidden.status, StatusCode::NOT_FOUND, "{name}");
        assert_eq!(hidden.bytes, missing.bytes, "{name}");
    }
    let hidden = harness
        .query(Some(&member), "members.list", &format!("node={theirs}"))
        .await;
    assert_eq!(hidden.code(), "not_found");
}

/// A change is recorded on the topics of the people who see it, and nobody else's.
#[tokio::test]
async fn tree_changes_reach_only_the_people_who_see_them() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let kaessbohrer = harness.node(&admin, &root, "kaessbohrer").await;
    let autohaus = harness.node(&admin, &root, "autohaus").await;
    let client = harness.person("client").await;
    harness.grant(&client, &kaessbohrer, "admin").await;
    harness.grant(&harness.member, &autohaus, "admin").await;
    let head = |user: &v1per_protocol::ids::UserId| {
        let topic = Topic::user(user);
        let db = harness.state.db.clone();
        async move {
            let mut conn = db.reader.acquire().await.unwrap();
            events::head(&mut conn, &topic).await.unwrap()
        }
    };
    let (client_before, member_before) = (head(&client).await, head(&harness.member).await);

    harness.node(&admin, &kaessbohrer, "website").await;
    assert!(head(&client).await > client_before);
    assert_eq!(head(&harness.member).await, member_before);
}

/// A node topic needs a scope on the node, and losing it ends the stream.
#[tokio::test]
async fn node_topics_follow_the_scope() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let mine = harness.node(&admin, &root, "mine").await;
    let theirs = harness.node(&admin, &root, "theirs").await;
    harness.grant(&harness.member, &mine, "viewer").await;
    let member = harness.session(&harness.member).await;

    let refused = harness
        .subscribe(Some(&member), &format!("topic=node%3A{theirs}"))
        .await;
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);
    let stream = harness
        .subscribe(Some(&member), &format!("topic=node%3A{mine}"))
        .await;
    assert_eq!(stream.status(), StatusCode::OK);
    let mut frames = Frames::new(stream);
    assert_eq!(frames.next(1).await[0]["kind"], "subscribed");

    harness
        .ok(
            &admin,
            "members.remove",
            json!({ "node": mine, "user": harness.member }),
        )
        .await;
    let ended = frames.next(1).await;
    assert_eq!(ended[0]["kind"], "resync");
    assert_eq!(ended[0]["reason"], "unavailable");
}
