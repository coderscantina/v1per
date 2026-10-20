//! The node tree, grants and invitations (decision 0001). Owns `nodes.*`, `members.*` and
//! `invitations.*`, and [`scope::NodeScope`], which every node-scoped operation of later
//! milestones takes.
//!
//! Topics: `node:<id>` carries changes to one node and its children, `members:<id>` its grants
//! and invitations, so a node edit never conflicts with a membership change. A member list
//! shows inherited grants too, so a grant change, or a move that changes a node's ancestors,
//! also records `members.changed` on the `members:<id>` topic of every node below: grants
//! change rarely, and each list keeps following one topic. A change to what someone sees in the
//! tree is recorded on their `user:<id>` topic, so the tree snapshot follows exactly the people
//! it concerns and no one learns of other subtrees.

mod members;
pub mod scope;
mod tree;

use std::collections::BTreeSet;

use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::{Empty, Topic};
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::{NodeId, UserId};
use v1per_protocol::limits::NODE_DEPTH_MAX;
use v1per_protocol::operations::nodes::{
    Node, NodeArgs, NodeDeleted, NodeResult, NodeTree, NodesCreateArgs, NodesDeleteArgs,
    NodesHandlers, NodesMoveArgs, NodesRenameArgs, NodesReorderArgs, NodesRequireTwoFactorArgs,
    PathEntry,
};
use v1per_protocol::registry::Snapshot;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity::{Caller, validate_name};
use crate::state::AppState;
use scope::{NodeScope, path_cte};

impl NodesHandlers for AppState {
    async fn nodes_create(
        &self,
        ctx: &mut CommandCtx,
        args: NodesCreateArgs,
    ) -> Result<NodeResult, ApiError> {
        let parent = ctx.node_scope(args.parent, Capability::NodeWrite).await?;
        let name = validate_name("name", &args.name)?;
        if parent.depth() >= NODE_DEPTH_MAX {
            return Err(FieldErrors::one(
                "parent",
                FieldErrorCode::TooMany,
                Some(NODE_DEPTH_MAX),
            ));
        }
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO nodes (parent_id, name, position, created_at)
             VALUES (?1, ?2, (SELECT COUNT(*) FROM nodes WHERE parent_id = ?1), ?3)
             RETURNING id",
        )
        .bind(args.parent.get())
        .bind(&name)
        .bind(ctx.now())
        .fetch_one(ctx.conn())
        .await?;
        let node = NodeId::new(id);
        let audience = tree::audience(ctx.conn(), node).await?;
        announce(ctx, &audience).await?;
        ctx.record(
            &Topic::node(args.parent),
            "node.child_added",
            json!({ "node": node }),
        )
        .await?;
        ctx.record(&Topic::node(node), "node.created", json!({}))
            .await?;
        let scope = ctx.node_scope(node, Capability::NodeRead).await?;
        Ok(NodeResult {
            node: detail(ctx.conn(), &scope).await?,
        })
    }

    async fn nodes_rename(
        &self,
        ctx: &mut CommandCtx,
        args: NodesRenameArgs,
    ) -> Result<NodeResult, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeWrite).await?;
        let name = validate_name("name", &args.name)?;
        let topic = Topic::node(args.node);
        ctx.check_revision(&topic, args.expected_revision).await?;
        let renamed = sqlx::query("UPDATE nodes SET name = ? WHERE id = ? AND name <> ?")
            .bind(&name)
            .bind(args.node.get())
            .bind(&name)
            .execute(ctx.conn())
            .await?
            .rows_affected();
        if renamed > 0 {
            let audience = tree::audience(ctx.conn(), args.node).await?;
            announce(ctx, &audience).await?;
            ctx.record(&topic, "node.renamed", json!({ "name": name }))
                .await?;
        }
        Ok(NodeResult {
            node: detail(ctx.conn(), &scope).await?,
        })
    }

    async fn nodes_reorder(
        &self,
        ctx: &mut CommandCtx,
        args: NodesReorderArgs,
    ) -> Result<NodeResult, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeRead).await?;
        let parent = scope.parent().ok_or_else(root_refused)?;
        writable(ctx, parent).await?;
        renumber(ctx.conn(), parent, Some((args.node, args.position))).await?;
        let audience = tree::audience(ctx.conn(), parent).await?;
        announce(ctx, &audience).await?;
        ctx.record(&Topic::node(parent), "node.children_reordered", json!({}))
            .await?;
        ctx.record(&Topic::node(args.node), "node.reordered", json!({}))
            .await?;
        Ok(NodeResult {
            node: detail(ctx.conn(), &scope).await?,
        })
    }

    async fn nodes_move(
        &self,
        ctx: &mut CommandCtx,
        args: NodesMoveArgs,
    ) -> Result<NodeResult, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeMove).await?;
        let from = scope.parent().ok_or_else(root_refused)?;
        writable(ctx, from).await?;
        let to = ctx.node_scope(args.parent, Capability::NodeWrite).await?;
        if to.within(args.node) {
            return Err(FieldErrors::one(
                "parent",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let (height, _) = subtree(ctx.conn(), args.node).await?;
        if to.depth() + 1 + height > NODE_DEPTH_MAX {
            return Err(FieldErrors::one(
                "parent",
                FieldErrorCode::TooMany,
                Some(NODE_DEPTH_MAX),
            ));
        }
        let topic = Topic::node(args.node);
        ctx.check_revision(&topic, args.expected_revision).await?;
        if from == args.parent {
            return Ok(NodeResult {
                node: detail(ctx.conn(), &scope).await?,
            });
        }

        let mut audience = tree::audience(ctx.conn(), args.node).await?;
        sqlx::query(
            "UPDATE nodes SET parent_id = ?1,
                              position = (SELECT COUNT(*) FROM nodes WHERE parent_id = ?1)
             WHERE id = ?2",
        )
        .bind(args.parent.get())
        .bind(args.node.get())
        .execute(ctx.conn())
        .await?;
        let moved = crate::monitors::on_subtree(ctx.conn(), args.node).await?;
        crate::probes::prune_moved(ctx.conn(), &moved).await?;
        crate::notifications::prune_moved(ctx.conn(), &moved).await?;
        crate::incidents::maintenance::prune_moved(ctx.conn(), &moved).await?;
        crate::pages::prune_moved(ctx).await?;
        crate::incidents::maintenance::refresh_in(ctx).await?;
        renumber(ctx.conn(), from, None).await?;
        audience.extend(tree::audience(ctx.conn(), args.node).await?);
        announce(ctx, &audience).await?;
        ctx.record(
            &Topic::node(from),
            "node.child_removed",
            json!({ "node": args.node }),
        )
        .await?;
        ctx.record(
            &Topic::node(args.parent),
            "node.child_added",
            json!({ "node": args.node }),
        )
        .await?;
        ctx.record(&Topic::members(args.node), "members.changed", json!({}))
            .await?;
        members_changed_below(ctx, args.node).await?;
        ctx.record(&topic, "node.moved", json!({ "parent": args.parent }))
            .await?;
        let scope = ctx.node_scope(args.node, Capability::NodeRead).await?;
        Ok(NodeResult {
            node: detail(ctx.conn(), &scope).await?,
        })
    }

    async fn nodes_delete(
        &self,
        ctx: &mut CommandCtx,
        args: NodesDeleteArgs,
    ) -> Result<NodeDeleted, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeDelete).await?;
        let parent = scope.parent().ok_or_else(root_refused)?;
        ctx.require_fresh_login()?;
        let (_, descendants) = subtree(ctx.conn(), args.node).await?;
        if descendants > 0 && args.descendants != Some(descendants) {
            return Err(ApiError::new(ErrorCode::NodeNotEmpty).with_current(descendants.into()));
        }
        let audience = tree::audience(ctx.conn(), args.node).await?;
        // The subtree, its grants, invitations and API keys cascade.
        sqlx::query("DELETE FROM nodes WHERE id = ?")
            .bind(args.node.get())
            .execute(ctx.conn())
            .await?;
        renumber(ctx.conn(), parent, None).await?;
        announce(ctx, &audience).await?;
        ctx.record(
            &Topic::node(parent),
            "node.child_removed",
            json!({ "node": args.node }),
        )
        .await?;
        ctx.record(&Topic::node(args.node), "node.deleted", json!({}))
            .await?;
        Ok(NodeDeleted {
            node: args.node,
            descendants,
        })
    }

    async fn nodes_require_two_factor(
        &self,
        ctx: &mut CommandCtx,
        args: NodesRequireTwoFactorArgs,
    ) -> Result<NodeResult, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::MembersManage).await?;
        ctx.require_fresh_login()?;
        let changed = sqlx::query(
            "UPDATE nodes SET two_factor_required = ?1 WHERE id = ?2 AND two_factor_required <> ?1",
        )
        .bind(args.required)
        .bind(args.node.get())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if changed > 0 {
            let audience = tree::audience(ctx.conn(), args.node).await?;
            announce(ctx, &audience).await?;
            ctx.record(
                &Topic::node(args.node),
                "node.two_factor_changed",
                json!({ "required": args.required }),
            )
            .await?;
        }
        Ok(NodeResult {
            node: detail(ctx.conn(), &scope).await?,
        })
    }

    async fn nodes_tree(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<NodeTree>, ApiError> {
        let (user, key) = match ctx.caller().clone() {
            Caller::Person { user, .. } => (user, None),
            Caller::Key(key) => (key.owner.clone(), Some(key)),
            Caller::Anonymous { .. } => return Err(ApiError::new(ErrorCode::Unauthenticated)),
        };
        let nodes = tree::visible(ctx.conn(), &user, key.as_deref()).await?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: NodeTree { nodes },
        })
    }

    async fn nodes_get(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<NodeResult>, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeRead).await?;
        Ok(Snapshot {
            topic: Topic::node(args.node),
            data: NodeResult {
                node: detail(ctx.conn(), &scope).await?,
            },
        })
    }
}

fn root_refused() -> ApiError {
    FieldErrors::one("node", FieldErrorCode::InvalidValue, None)
}

/// `node.write` on a parent the request does not name, such as the one a node moves away from.
/// The caller may not be able to read it, so any refusal is `forbidden`: the node they named
/// is visible, the parent's existence is no news.
async fn writable(ctx: &mut CommandCtx, parent: NodeId) -> Result<(), ApiError> {
    match ctx.node_scope(parent, Capability::NodeWrite).await {
        Ok(_) => Ok(()),
        Err(error) if error.code() == ErrorCode::Internal => Err(error),
        Err(_) => Err(ApiError::new(ErrorCode::Forbidden)),
    }
}

/// Records `tree.changed` on the topic of everyone whose tree a change affects.
async fn announce(ctx: &mut CommandCtx, audience: &BTreeSet<UserId>) -> Result<(), ApiError> {
    for user in audience {
        ctx.record(&Topic::user(user), "tree.changed", json!({}))
            .await?;
    }
    Ok(())
}

/// Records `members.changed` on the members topic of every node below `node`, whose member
/// lists include `node`'s grants.
async fn members_changed_below(ctx: &mut CommandCtx, node: NodeId) -> Result<(), ApiError> {
    let below: Vec<i64> = sqlx::query_scalar(
        "WITH RECURSIVE down(id) AS (
             SELECT id FROM nodes WHERE parent_id = ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN down d ON n.parent_id = d.id
         )
         SELECT id FROM down",
    )
    .bind(node.get())
    .fetch_all(ctx.conn())
    .await?;
    for id in below {
        ctx.record(
            &Topic::members(NodeId::new(id)),
            "members.changed",
            json!({}),
        )
        .await?;
    }
    Ok(())
}

/// Numbers the children of `parent` from 0 in their order, with `moved` put at its index (past
/// the end means last).
async fn renumber(
    conn: &mut SqliteConnection,
    parent: NodeId,
    moved: Option<(NodeId, u32)>,
) -> Result<(), sqlx::Error> {
    let mut children: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM nodes WHERE parent_id = ? ORDER BY position, id")
            .bind(parent.get())
            .fetch_all(&mut *conn)
            .await?;
    if let Some((node, index)) = moved {
        children.retain(|id| *id != node.get());
        children.insert((index as usize).min(children.len()), node.get());
    }
    for (position, id) in children.into_iter().enumerate() {
        sqlx::query("UPDATE nodes SET position = ?1 WHERE id = ?2 AND position <> ?1")
            .bind(position as i64)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// The levels below `node` and the number of its descendants.
async fn subtree(conn: &mut SqliteConnection, node: NodeId) -> Result<(u32, u32), sqlx::Error> {
    let (height, descendants): (i64, i64) = sqlx::query_as(
        "WITH RECURSIVE down(id, level) AS (
             SELECT ?1, 0
             UNION ALL
             SELECT n.id, d.level + 1 FROM nodes n JOIN down d ON n.parent_id = d.id
         )
         SELECT MAX(level), COUNT(*) - 1 FROM down",
    )
    .bind(node.get())
    .fetch_one(conn)
    .await?;
    Ok((height as u32, descendants as u32))
}

/// The node as the scope's caller sees it, with names only on ancestors they can read.
async fn detail(conn: &mut SqliteConnection, scope: &NodeScope) -> Result<Node, ApiError> {
    let path: Vec<(i64, String, bool)> = sqlx::query_as(concat!(
        path_cte!(),
        "SELECT n.id, n.name, n.two_factor_required FROM path p JOIN nodes n ON n.id = p.id
         ORDER BY p.step"
    ))
    .bind(scope.node().get())
    .fetch_all(&mut *conn)
    .await?;
    let (name, position, two_factor_required, created_at, children): (String, i64, bool, i64, i64) =
        sqlx::query_as(
            "SELECT name, position, two_factor_required, created_at,
                    (SELECT COUNT(*) FROM nodes c WHERE c.parent_id = n.id)
             FROM nodes n WHERE id = ?",
        )
        .bind(scope.node().get())
        .fetch_one(conn)
        .await?;
    let named: std::collections::HashMap<i64, &str> = path
        .iter()
        .map(|(id, name, _)| (*id, name.as_str()))
        .collect();
    Ok(Node {
        id: scope.node(),
        parent: scope.parent(),
        name,
        depth: scope.depth(),
        position: position as u32,
        path: scope
            .ancestors()
            .map(|(id, readable)| PathEntry {
                id,
                name: readable
                    .then(|| named.get(&id.get()).map(|name| (*name).to_owned()))
                    .flatten(),
            })
            .collect(),
        role: scope.role(),
        capabilities: scope.capabilities().to_vec(),
        two_factor_required,
        two_factor_inherited: path.iter().skip(1).any(|(.., required)| *required),
        children: children as u32,
        created_at,
    })
}
