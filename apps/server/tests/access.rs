//! The permission matrix: the briefing's tree with its grants, read through the API. Each
//! person sees exactly their nodes with exactly their capabilities, and every other node
//! answers like one that does not exist.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use axum::http::StatusCode;
use common::Harness;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    roles: BTreeMap<String, Vec<String>>,
    tree: Value,
    people: Vec<Person>,
}

#[derive(Deserialize)]
struct Person {
    name: String,
    #[serde(default)]
    instance_admin: bool,
    grants: BTreeMap<String, String>,
    sees: BTreeMap<String, String>,
    placeholders: BTreeSet<String>,
}

/// Creates `tree` below `parent` as the admin, recording every node's ID by name.
async fn build(
    harness: &Harness,
    admin: &str,
    parent: &str,
    tree: &Value,
    ids: &mut BTreeMap<String, String>,
) {
    for (name, children) in tree.as_object().unwrap() {
        let id = harness.node(admin, parent, name).await;
        ids.insert(name.clone(), id.clone());
        Box::pin(build(harness, admin, &id, children, ids)).await;
    }
}

#[tokio::test]
async fn every_grant_shows_exactly_its_nodes_and_capabilities() {
    let fixture: Fixture = serde_json::from_str(include_str!("fixtures/node-tree.json")).unwrap();
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let mut ids = BTreeMap::from([("Root".to_owned(), harness.root().await)]);
    build(
        &harness,
        &admin,
        &ids["Root"].clone(),
        &fixture.tree,
        &mut ids,
    )
    .await;
    let names: BTreeMap<String, String> = ids
        .iter()
        .map(|(name, id)| (id.clone(), name.clone()))
        .collect();

    let unknown = harness
        .query(Some(&admin), "nodes.get", "node=999999")
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);

    for person in &fixture.people {
        let user = if person.instance_admin {
            harness.admin.clone()
        } else {
            harness.person(&person.name.replace(' ', ".")).await
        };
        for (node, role) in &person.grants {
            harness.grant(&user, &ids[node], role).await;
        }
        let cookie = harness.session(&user).await;

        let tree = harness.query(Some(&cookie), "nodes.tree", "").await.json();
        let mut sees = BTreeMap::new();
        let mut placeholders = BTreeSet::new();
        for node in tree["nodes"].as_array().unwrap() {
            let name = names[node["id"].as_str().unwrap()].clone();
            match node["role"].as_str() {
                Some(role) => {
                    assert_eq!(node["name"], name.as_str());
                    assert_eq!(node["capabilities"], serde_json::json!(fixture.roles[role]));
                    sees.insert(name, role.to_owned());
                }
                None => {
                    assert!(node.get("name").is_none(), "{} leaks {name}", person.name);
                    placeholders.insert(name);
                }
            }
        }
        assert_eq!(sees, person.sees, "{} tree", person.name);
        assert_eq!(
            placeholders, person.placeholders,
            "{} placeholders",
            person.name
        );

        for (name, id) in &ids {
            let node = harness
                .query(Some(&cookie), "nodes.get", &format!("node={id}"))
                .await;
            match person.sees.get(name) {
                Some(role) => {
                    assert_eq!(node.status, StatusCode::OK, "{} {name}", person.name);
                    assert_eq!(node.json()["node"]["role"], role.as_str());
                    assert_eq!(
                        node.json()["node"]["capabilities"],
                        serde_json::json!(fixture.roles[role])
                    );
                }
                None => assert_eq!(
                    (node.status, &node.bytes),
                    (unknown.status, &unknown.bytes),
                    "{} {name}",
                    person.name
                ),
            }
        }
        println!(
            "{:<20} sees {:?} below placeholders {:?}",
            person.name, sees, placeholders
        );
    }
}
