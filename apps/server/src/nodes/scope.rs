//! Authorized node scopes. Only [`resolve`] constructs a [`NodeScope`], so code holding one
//! knows authorization already happened for this caller on this node.
//!
//! The effective role on a node is the highest grant on it or an ancestor; the instance admin
//! is owner everywhere. A node without any such grant answers `not_found`, exactly like a node
//! that does not exist. An API key reaches only its node's subtree, with the capabilities it
//! holds that its owner still holds there.

use sqlx::SqliteConnection;
use v1per_protocol::access::{Capability, Role};
use v1per_protocol::errors::ErrorCode;
use v1per_protocol::ids::{NodeId, UserId};

use crate::error::ApiError;
use crate::identity::Caller;

/// Common table expression `path(id, parent_id, step)`: node `?1` at step 0 and its ancestors
/// up to the root. The depth limit keeps it at most 9 primary-key probes.
macro_rules! path_cte {
    () => {
        "WITH RECURSIVE path(id, parent_id, step) AS (
             SELECT id, parent_id, 0 FROM nodes WHERE id = ?1
             UNION ALL
             SELECT n.id, n.parent_id, p.step + 1 FROM nodes n JOIN path p ON n.id = p.parent_id
         ) "
    };
}
pub(crate) use path_cte;

#[derive(Debug, Clone)]
pub struct NodeScope {
    /// The node and its ancestors, nearest first.
    path: Vec<NodeId>,
    /// How many path entries, counted from the node upward, the caller can read.
    readable: usize,
    role: Role,
    capabilities: Vec<Capability>,
}

impl NodeScope {
    pub fn node(&self) -> NodeId {
        self.path[0]
    }

    /// `None` for the root.
    pub fn parent(&self) -> Option<NodeId> {
        self.path.get(1).copied()
    }

    /// Levels below the root; the root is 0.
    pub fn depth(&self) -> u32 {
        (self.path.len() - 1) as u32
    }

    /// The effective role, for rank checks. For an API key, its owner's role.
    pub fn role(&self) -> Role {
        self.role
    }

    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    pub fn can(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// `forbidden` when the scope lacks the capability; the node is already known to be visible.
    pub fn require(&self, capability: Capability) -> Result<(), ApiError> {
        if self.can(capability) {
            Ok(())
        } else {
            Err(ApiError::new(ErrorCode::Forbidden))
        }
    }

    /// Whether `node` is this node or one of its ancestors.
    pub fn within(&self, node: NodeId) -> bool {
        self.path.contains(&node)
    }

    /// Ancestors from the root down to the parent, each with whether the caller can read it.
    pub fn ancestors(&self) -> impl Iterator<Item = (NodeId, bool)> + '_ {
        self.path
            .iter()
            .enumerate()
            .skip(1)
            .rev()
            .map(|(index, node)| (*node, index < self.readable))
    }
}

/// Resolves `caller`'s scope on `node`. Unknown, deleted and unreachable nodes all answer the
/// same `not_found`.
pub async fn resolve(
    conn: &mut SqliteConnection,
    caller: &Caller,
    node: NodeId,
) -> Result<NodeScope, ApiError> {
    match caller {
        Caller::Person { user, .. } => person(conn, user, node).await,
        Caller::Key(key) => {
            let scope = person(conn, &key.owner, node).await?;
            let at = scope
                .path
                .iter()
                .position(|id| *id == key.node)
                .ok_or_else(ApiError::not_found)?;
            let capabilities: Vec<Capability> = scope
                .capabilities
                .iter()
                .copied()
                .filter(|c| c.key_grantable() && key.capabilities.contains(c))
                .collect();
            if !capabilities.contains(&Capability::NodeRead) {
                return Err(ApiError::not_found());
            }
            Ok(NodeScope {
                readable: scope.readable.min(at + 1),
                capabilities,
                ..scope
            })
        }
        Caller::Anonymous { .. } => Err(ApiError::new(ErrorCode::Unauthenticated)),
    }
}

/// `user`'s scope from their grants on the path, read in one recursive query.
pub(crate) async fn person(
    conn: &mut SqliteConnection,
    user: &UserId,
    node: NodeId,
) -> Result<NodeScope, ApiError> {
    let rows: Vec<(i64, Option<String>, bool)> = sqlx::query_as(concat!(
        path_cte!(),
        "SELECT p.id, g.role, (SELECT instance_admin FROM users WHERE id = ?2)
         FROM path p LEFT JOIN grants g ON g.node_id = p.id AND g.user_id = ?2
         ORDER BY p.step"
    ))
    .bind(node.get())
    .bind(user.as_str())
    .fetch_all(conn)
    .await?;
    let instance_admin = rows.first().is_some_and(|(_, _, admin)| *admin);
    let mut role: Option<Role> = instance_admin.then_some(Role::Owner);
    let mut readable = if instance_admin { rows.len() } else { 0 };
    for (index, (_, granted, _)) in rows.iter().enumerate() {
        let Some(granted) = granted else { continue };
        let granted = parse_role(granted)?;
        if role.is_none_or(|role| role.rank() < granted.rank()) {
            role = Some(granted);
        }
        readable = readable.max(index + 1);
    }
    let role = role.ok_or_else(ApiError::not_found)?;
    Ok(NodeScope {
        path: rows.iter().map(|(id, ..)| NodeId::new(*id)).collect(),
        readable,
        role,
        capabilities: role.capabilities().to_vec(),
    })
}

/// A role as stored. Anything else is a broken row, so `internal`.
pub(crate) fn parse_role(role: &str) -> Result<Role, ApiError> {
    Role::parse(role).ok_or_else(|| ApiError::internal(format!("unknown role {role}")))
}

/// `forbidden` unless the caller is a signed-in instance admin, read now so a demotion applies
/// at once. Returns the admin's ID.
pub async fn require_instance_admin(
    conn: &mut SqliteConnection,
    caller: &Caller,
) -> Result<UserId, ApiError> {
    let user = caller
        .user()
        .ok_or_else(|| ApiError::new(ErrorCode::Forbidden))?;
    let admin: Option<bool> = sqlx::query_scalar("SELECT instance_admin FROM users WHERE id = ?")
        .bind(user.as_str())
        .fetch_optional(conn)
        .await?;
    match admin {
        Some(true) => Ok(user.clone()),
        _ => Err(ApiError::new(ErrorCode::Forbidden)),
    }
}
