//! The tree as one person sees it, and who sees a node.
//!
//! A person sees every node at or below their grants, and the ancestors above their grants as
//! placeholders: enough to place the visible nodes, without the names of nodes they cannot
//! read. Two clients under one parent learn neither each other's nor the parent's name.

use std::collections::{BTreeSet, HashMap};

use sqlx::SqliteConnection;
use v1per_protocol::access::{Capability, Role};
use v1per_protocol::ids::{NodeId, UserId};
use v1per_protocol::operations::nodes::TreeNode;

use crate::error::ApiError;
use crate::identity::KeyCaller;

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    parent: Option<i64>,
    name: String,
    position: i64,
    two_factor_required: bool,
    /// The highest rank reaching this node, `None` for a placeholder.
    rank: Option<i64>,
}

/// What `user` sees, depth first with siblings in order. With `key`, only the key's subtree is
/// readable, with the key's capabilities; its ancestors become placeholders.
pub async fn visible(
    conn: &mut SqliteConnection,
    user: &UserId,
    key: Option<&KeyCaller>,
) -> Result<Vec<TreeNode>, ApiError> {
    let mut rows: Vec<Row> = sqlx::query_as(
        "WITH RECURSIVE
           granted(id, rank) AS (
             SELECT node_id, CASE role WHEN 'owner' THEN 3 WHEN 'admin' THEN 2
                                       WHEN 'member' THEN 1 ELSE 0 END
               FROM grants WHERE user_id = ?1
             UNION ALL
             SELECT n.id, 3 FROM nodes n JOIN users u ON u.id = ?1
              WHERE n.parent_id IS NULL AND u.instance_admin = 1
           ),
           below(id, rank) AS (
             SELECT id, rank FROM granted
             UNION ALL
             SELECT n.id, b.rank FROM nodes n JOIN below b ON n.parent_id = b.id
           ),
           above(id) AS (
             SELECT n.parent_id FROM nodes n JOIN granted g ON n.id = g.id
              WHERE n.parent_id IS NOT NULL
             UNION
             SELECT n.parent_id FROM nodes n JOIN above a ON n.id = a.id
              WHERE n.parent_id IS NOT NULL
           ),
           shown(id, rank) AS (
             SELECT id, MAX(rank) FROM below GROUP BY id
             UNION ALL
             SELECT id, NULL FROM above WHERE id NOT IN (SELECT id FROM below)
           )
         SELECT n.id, n.parent_id AS parent, n.name, n.position, n.two_factor_required, s.rank
         FROM shown s JOIN nodes n ON n.id = s.id",
    )
    .bind(user.as_str())
    .fetch_all(conn)
    .await?;
    if let Some(key) = key {
        limit_to_key(&mut rows, key.node.get());
    }

    let mut children: HashMap<Option<i64>, Vec<&Row>> = HashMap::new();
    for row in &rows {
        children.entry(row.parent).or_default().push(row);
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|row| (row.position, row.id));
    }
    let mut out = Vec::with_capacity(rows.len());
    let mut stack: Vec<(&Row, u32)> = children
        .get(&None)
        .into_iter()
        .flatten()
        .map(|row| (*row, 0))
        .collect();
    while let Some((row, depth)) = stack.pop() {
        let role = row.rank.and_then(Role::from_rank);
        let capabilities: Vec<Capability> = match (role, key) {
            (None, _) => Vec::new(),
            (Some(role), None) => role.capabilities().to_vec(),
            (Some(role), Some(key)) => role
                .capabilities()
                .iter()
                .copied()
                .filter(|c| c.key_grantable() && key.capabilities.contains(c))
                .collect(),
        };
        out.push(TreeNode {
            id: NodeId::new(row.id),
            parent: row.parent.map(NodeId::new),
            name: role.map(|_| row.name.clone()),
            depth,
            position: row.position as u32,
            role,
            capabilities,
            two_factor_required: role.is_some() && row.two_factor_required,
        });
        for child in children.get(&Some(row.id)).into_iter().flatten().rev() {
            stack.push((child, depth + 1));
        }
    }
    Ok(out)
}

/// Keeps the key's node, its readable descendants, and its ancestors as placeholders. The rows
/// hold every ancestor of a readable node, so walking parents within them reaches the root.
fn limit_to_key(rows: &mut Vec<Row>, key_node: i64) {
    let parents: HashMap<i64, Option<i64>> = rows.iter().map(|r| (r.id, r.parent)).collect();
    let chain = |mut id: Option<i64>| {
        let parents = &parents;
        std::iter::from_fn(move || {
            let current = id?;
            id = parents.get(&current).copied().flatten();
            Some(current)
        })
    };
    let ancestors: BTreeSet<i64> = chain(Some(key_node)).skip(1).collect();
    let inside: BTreeSet<i64> = rows
        .iter()
        .filter(|row| chain(Some(row.id)).any(|id| id == key_node))
        .map(|row| row.id)
        .collect();
    rows.retain_mut(|row| {
        if ancestors.contains(&row.id) {
            row.rank = None;
        }
        ancestors.contains(&row.id) || inside.contains(&row.id)
    });
    if rows.iter().all(|row| row.rank.is_none()) {
        rows.clear();
    }
}

/// Everyone who sees `node` or a node below it: grants on the node, its ancestors and its
/// descendants, and instance admins. A change to `node` changes their tree.
pub async fn audience(
    conn: &mut SqliteConnection,
    node: NodeId,
) -> Result<BTreeSet<UserId>, sqlx::Error> {
    let users: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE
           up(id) AS (
             SELECT ?1
             UNION
             SELECT n.parent_id FROM nodes n JOIN up ON n.id = up.id WHERE n.parent_id IS NOT NULL
           ),
           down(id) AS (
             SELECT ?1
             UNION
             SELECT n.id FROM nodes n JOIN down ON n.parent_id = down.id
           )
         SELECT user_id FROM grants WHERE node_id IN (SELECT id FROM up UNION SELECT id FROM down)
         UNION
         SELECT id FROM users WHERE instance_admin = 1",
    )
    .bind(node.get())
    .fetch_all(conn)
    .await?;
    Ok(users.into_iter().map(UserId::new).collect())
}
