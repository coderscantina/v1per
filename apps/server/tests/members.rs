//! Grants and invitations: redemption, the rank rule, leaving, and who is listed.

mod common;

use axum::http::StatusCode;
use common::{Harness, PASSWORD};
use serde_json::{Value, json};

/// A signed-out invitee gets an account with the invited email, a session and the role; the
/// link works once, and a replay of the invite never shows the code again.
#[tokio::test]
async fn invitations_are_redeemed_once_with_their_role() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let autohaus = harness
        .node(&admin, &harness.root().await, "autohaus")
        .await;
    let body = json!({
        "idempotency_key": "invite-1",
        "node": autohaus,
        "role": "viewer",
        "email": "Contact@Autohaus.example",
    });
    let created = harness
        .command(Some(&admin), "members.invite", body.clone())
        .await
        .json();
    let code = created["secret"]["code"].as_str().unwrap().to_owned();
    assert_eq!(created["secret"]["link"], format!("/join?code={code}"));
    assert_eq!(created["invitation"]["email"], "contact@autohaus.example");
    let replay = harness
        .command(Some(&admin), "members.invite", body)
        .await
        .json();
    assert_eq!(replay["invitation"]["id"], created["invitation"]["id"]);
    assert_eq!(
        (&replay["secret"], &replay["secret_withheld"]),
        (&Value::Null, &json!(true))
    );

    let preview = harness
        .query(None, "invitations.preview", &format!("code={code}"))
        .await
        .json();
    assert_eq!(
        (
            &preview["node_name"],
            &preview["role"],
            &preview["account_exists"]
        ),
        (&json!("autohaus"), &json!("viewer"), &json!(false))
    );

    let accept = json!({ "code": code, "account": { "name": "Contact", "password": PASSWORD } });
    let accepted = harness
        .run(None, "invitations.accept", accept.clone())
        .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.json());
    assert_eq!(
        accepted.json()["account"]["email"],
        "contact@autohaus.example"
    );
    let cookie = accepted
        .header("set-cookie")
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let tree = harness.query(Some(&cookie), "nodes.tree", "").await.json();
    assert_eq!(tree["nodes"][1]["name"], "autohaus");
    assert_eq!(tree["nodes"][1]["role"], "viewer");

    let again = harness.run(None, "invitations.accept", accept).await;
    let unknown = harness
        .run(
            None,
            "invitations.accept",
            json!({ "code": "nope", "account": { "name": "x", "password": PASSWORD } }),
        )
        .await;
    assert_eq!(again.code(), "not_found");
    assert_eq!(again.bytes, unknown.bytes);
    let listed = harness
        .query(
            Some(&admin),
            "members.invitations",
            &format!("node={autohaus}"),
        )
        .await
        .json();
    assert_eq!(listed["invitations"][0]["state"], "used");
}

/// R-RANK on effective roles: admins invite and change only roles below their own, owners may
/// make owners, raising to admin needs a fresh login, and nobody changes their own grant.
#[tokio::test]
async fn role_changes_follow_rank_and_reauthentication() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let node = harness.node(&admin, &harness.root().await, "client").await;
    let (owner, manager, member) = (
        harness.person("owner").await,
        harness.person("manager").await,
        harness.person("worker").await,
    );
    harness.grant(&owner, &node, "owner").await;
    harness.grant(&manager, &node, "admin").await;
    harness.grant(&member, &node, "member").await;
    let manager_cookie = harness.session(&manager).await;
    let owner_cookie = harness.session_reauthenticated(&owner, None).await;
    let change = |user: &v1per_protocol::ids::UserId, role: &str| json!({ "node": node, "user": user, "role": role });
    let code = |answer: common::Answer| {
        if answer.status == StatusCode::OK {
            "ok".to_owned()
        } else {
            answer.code()
        }
    };

    let steps = [
        (
            &manager_cookie,
            "members.invite",
            json!({ "node": node, "role": "admin", "email": "a@b.c" }),
            "forbidden",
        ),
        (
            &manager_cookie,
            "members.updateRole",
            change(&member, "admin"),
            "forbidden",
        ),
        (
            &manager_cookie,
            "members.updateRole",
            change(&owner, "viewer"),
            "forbidden",
        ),
        (
            &manager_cookie,
            "members.updateRole",
            change(&manager, "viewer"),
            "forbidden",
        ),
        (
            &manager_cookie,
            "members.updateRole",
            change(&member, "viewer"),
            "ok",
        ),
        (
            &owner_cookie,
            "members.updateRole",
            change(&member, "admin"),
            "reauthentication_required",
        ),
        (
            &owner_cookie,
            "members.invite",
            json!({ "node": node, "role": "owner", "email": "a@b.c" }),
            "ok",
        ),
    ];
    for (index, (cookie, name, body, expected)) in steps.into_iter().enumerate() {
        let answer = harness.run(Some(cookie), name, body).await;
        assert_eq!(code(answer), expected, "step {}", index + 1);
    }
    let fresh = harness.session(&owner).await;
    let raised = harness
        .ok(&fresh, "members.updateRole", change(&member, "owner"))
        .await;
    assert_eq!(raised["member"]["role"], "owner");
}

/// Anyone may drop their own grant and loses the node at once; removing someone else needs
/// `members.manage` and a higher rank. Grants on ancestors stay.
#[tokio::test]
async fn members_leave_or_are_removed_by_rank() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let node = harness.node(&admin, &harness.root().await, "client").await;
    let (manager, viewer) = (
        harness.person("manager").await,
        harness.person("viewer").await,
    );
    harness.grant(&manager, &node, "admin").await;
    harness.grant(&viewer, &node, "viewer").await;
    harness.grant(&harness.member, &node, "member").await;
    let remove = |user: &v1per_protocol::ids::UserId| json!({ "node": node, "user": user });

    let member = harness.session(&harness.member).await;
    assert_eq!(
        harness
            .run(Some(&member), "members.remove", remove(&viewer))
            .await
            .code(),
        "forbidden"
    );
    let manager_cookie = harness.session(&manager).await;
    assert_eq!(
        harness
            .run(
                Some(&manager_cookie),
                "members.remove",
                remove(&harness.admin)
            )
            .await
            .code(),
        "not_found",
        "the instance admin holds no grant here"
    );
    harness
        .ok(&manager_cookie, "members.remove", remove(&viewer))
        .await;
    harness
        .ok(&member, "members.remove", remove(&harness.member))
        .await;
    assert_eq!(
        harness
            .query(Some(&member), "nodes.get", &format!("node={node}"))
            .await
            .code(),
        "not_found"
    );
}

/// The member list holds everyone with access: direct and inherited grants, nearest first, and
/// instance admins.
#[tokio::test]
async fn the_member_list_shows_inherited_access() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let parent = harness
        .node(&admin, &harness.root().await, "client services")
        .await;
    let child = harness.node(&admin, &parent, "autohaus").await;
    harness.grant(&harness.member, &parent, "admin").await;
    harness.grant(&harness.member, &child, "viewer").await;

    let listed = harness
        .query(Some(&admin), "members.list", &format!("node={child}"))
        .await
        .json();
    let members = listed["members"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(
        (&members[0]["name"], &members[0]["instance_admin"]),
        (&json!("Admin"), &json!(true))
    );
    assert_eq!(members[1]["role"], "admin");
    assert_eq!(
        members[1]["grants"],
        json!([{ "node": child, "role": "viewer" }, { "node": parent, "role": "admin" }])
    );
    assert!(
        listed["cursor"]
            .as_str()
            .unwrap()
            .starts_with(&format!("members:{child}@"))
    );
}

/// A grant change reaches the member lists below it, and never makes a node edit stale.
#[tokio::test]
async fn member_changes_reach_descendants_and_leave_node_revisions_alone() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let parent = harness.node(&admin, &harness.root().await, "parent").await;
    let child = harness.node(&admin, &parent, "child").await;
    let node = harness
        .query(Some(&admin), "nodes.get", &format!("node={parent}"))
        .await
        .json();
    let below = harness
        .query(Some(&admin), "members.list", &format!("node={child}"))
        .await
        .json();

    harness.grant(&harness.member, &parent, "viewer").await;

    let head: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM topic_events WHERE topic = ?")
            .bind(format!("members:{child}"))
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap();
    assert!(head > below["revision"].as_i64().unwrap());
    let renamed = harness
        .run(
            Some(&admin),
            "nodes.rename",
            json!({ "node": parent, "name": "renamed", "expected_revision": node["revision"] }),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.json());
}
