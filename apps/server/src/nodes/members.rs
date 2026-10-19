//! `members.*` and `invitations.*`: who has access to a node, and how people get it.
//!
//! Ranks follow R-RANK on the effective role at the node: an actor changes, removes and grants
//! only roles below its own, except that owners may make and change owners. Nobody changes
//! their own grant, so nobody raises their own role; anyone may remove their own. A node never
//! ends up without an owner: the instance admin owns the root and so every node.

use std::collections::BTreeMap;

use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::access::{Capability, Role};
use v1per_protocol::envelope::{Empty, Topic};
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::{InvitationId, NodeId, UserId};
use v1per_protocol::limits::{
    INVITATION_LIFETIME_DEFAULT_DAYS, INVITATION_LIFETIME_MAX_DAYS, INVITATION_PATH,
};
use v1per_protocol::operations::invitations::{
    InvitationAccepted, InvitationCodeArgs, InvitationPreview, InvitationsAcceptArgs,
    InvitationsHandlers,
};
use v1per_protocol::operations::members::{
    Invitation, InvitationArgs, InvitationCreated, InvitationList, InvitationResult,
    InvitationSecret, InvitationState, Member, MemberArgs, MemberGrant, MemberList, MemberResult,
    MembersHandlers, MembersInviteArgs, MembersUpdateRoleArgs,
};
use v1per_protocol::operations::nodes::NodeArgs;
use v1per_protocol::registry::Snapshot;

use super::scope::{parse_role, path_cte};
use crate::context::{CommandCtx, QueryCtx};
use crate::db::new_id;
use crate::error::{ApiError, FieldErrors};
use crate::identity::{self, Caller};
use crate::instance;
use crate::state::AppState;

const DAY_MS: i64 = 86_400_000;
/// Characters of the code kept in clear to tell invitations apart.
const PREFIX_CHARS: usize = 8;

impl MembersHandlers for AppState {
    async fn members_invite(
        &self,
        ctx: &mut CommandCtx,
        args: MembersInviteArgs,
    ) -> Result<InvitationCreated, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::MembersManage).await?;
        if !scope.role().may_assign(args.role) {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        let email = identity::validate_email("email", &args.email)?;
        let days = args
            .lifetime_days
            .unwrap_or(INVITATION_LIFETIME_DEFAULT_DAYS);
        if !(1..=INVITATION_LIFETIME_MAX_DAYS).contains(&days) {
            return Err(FieldErrors::one(
                "lifetime_days",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let inviter = ctx
            .caller()
            .user()
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;

        let id = InvitationId::new(new_id());
        let (code, digest) = identity::new_session_token();
        let now = ctx.now();
        sqlx::query(
            "INSERT INTO invitations (id, node_id, role, email, prefix, code_hash, created_by, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(args.node.get())
        .bind(args.role.as_str())
        .bind(&email)
        .bind(&code[..PREFIX_CHARS])
        .bind(digest)
        .bind(inviter.as_str())
        .bind(now)
        .bind(now + i64::from(days) * DAY_MS)
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::members(args.node),
            "invitation.created",
            json!({ "invitation": id }),
        )
        .await?;
        let base = instance::effective(ctx.conn(), &self.config)
            .await?
            .public_url
            .unwrap_or_default();
        let link = format!("{base}{INVITATION_PATH}?code={code}");
        Ok(InvitationCreated {
            invitation: invitation(ctx.conn(), &id, now).await?,
            secret: Some(InvitationSecret { code, link }),
            secret_withheld: false,
        })
    }

    async fn members_revoke_invitation(
        &self,
        ctx: &mut CommandCtx,
        args: InvitationArgs,
    ) -> Result<InvitationResult, ApiError> {
        let node: Option<i64> = sqlx::query_scalar("SELECT node_id FROM invitations WHERE id = ?")
            .bind(args.invitation.as_str())
            .fetch_optional(ctx.conn())
            .await?;
        let node = NodeId::new(node.ok_or_else(ApiError::not_found)?);
        ctx.node_scope(node, Capability::MembersManage).await?;
        let now = ctx.now();
        let revoked = sqlx::query(
            "UPDATE invitations SET revoked_at = ?1
             WHERE id = ?2 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?1",
        )
        .bind(now)
        .bind(args.invitation.as_str())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if revoked > 0 {
            ctx.record(
                &Topic::members(node),
                "invitation.revoked",
                json!({ "invitation": args.invitation }),
            )
            .await?;
        }
        Ok(InvitationResult {
            invitation: invitation(ctx.conn(), &args.invitation, now).await?,
        })
    }

    async fn members_update_role(
        &self,
        ctx: &mut CommandCtx,
        args: MembersUpdateRoleArgs,
    ) -> Result<MemberResult, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::MembersManage).await?;
        let target = granted_member(ctx.conn(), args.node, &args.user).await?;
        if ctx.caller().user() == Some(&args.user)
            || !scope.role().may_assign(target.role)
            || !scope.role().may_assign(args.role)
        {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        if matches!(args.role, Role::Admin | Role::Owner) {
            ctx.require_fresh_login()?;
        }
        let topic = Topic::members(args.node);
        ctx.check_revision(&topic, args.expected_revision).await?;
        let changed = sqlx::query(
            "UPDATE grants SET role = ?1 WHERE node_id = ?2 AND user_id = ?3 AND role <> ?1",
        )
        .bind(args.role.as_str())
        .bind(args.node.get())
        .bind(args.user.as_str())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if changed > 0 {
            ctx.record(&Topic::user(&args.user), "tree.changed", json!({}))
                .await?;
            super::members_changed_below(ctx, args.node).await?;
            ctx.record(
                &topic,
                "member.role_changed",
                json!({ "user": args.user, "role": args.role }),
            )
            .await?;
        }
        Ok(MemberResult {
            member: granted_member(ctx.conn(), args.node, &args.user).await?,
        })
    }

    async fn members_remove(
        &self,
        ctx: &mut CommandCtx,
        args: MemberArgs,
    ) -> Result<Empty, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeRead).await?;
        let target = granted_member(ctx.conn(), args.node, &args.user).await?;
        if ctx.caller().user() != Some(&args.user) {
            scope.require(Capability::MembersManage)?;
            if !scope.role().may_assign(target.role) {
                return Err(ApiError::new(ErrorCode::Forbidden));
            }
        }
        sqlx::query("DELETE FROM grants WHERE node_id = ? AND user_id = ?")
            .bind(args.node.get())
            .bind(args.user.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(&Topic::user(&args.user), "tree.changed", json!({}))
            .await?;
        super::members_changed_below(ctx, args.node).await?;
        ctx.record(
            &Topic::members(args.node),
            "member.removed",
            json!({ "user": args.user }),
        )
        .await?;
        Ok(Empty {})
    }

    async fn members_list(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<MemberList>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        Ok(Snapshot {
            topic: Topic::members(args.node),
            data: MemberList {
                members: members(ctx.conn(), args.node, None).await?,
            },
        })
    }

    async fn members_invitations(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<InvitationList>, ApiError> {
        ctx.node_scope(args.node, Capability::MembersManage).await?;
        let rows: Vec<InvitationRow> = sqlx::query_as(concat!(
            select_invitation!(),
            " WHERE node_id = ? ORDER BY created_at DESC, id"
        ))
        .bind(args.node.get())
        .fetch_all(ctx.conn())
        .await?;
        let now = crate::db::now_ms();
        Ok(Snapshot {
            topic: Topic::members(args.node),
            data: InvitationList {
                invitations: rows
                    .into_iter()
                    .map(|row| invitation_of(row, now))
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}

impl InvitationsHandlers for AppState {
    async fn invitations_accept(
        &self,
        ctx: &mut CommandCtx,
        args: InvitationsAcceptArgs,
    ) -> Result<InvitationAccepted, ApiError> {
        let now = ctx.now();
        let (id, node, role, email) = pending(ctx.conn(), &args.code, now).await?;
        let user = match (ctx.caller().clone(), args.account) {
            (Caller::Person { user, .. }, None) => user,
            (Caller::Anonymous { .. }, Some(account)) => {
                let name = identity::validate_name("account.name", &account.name)?;
                identity::validate_password("account.password", &account.password)?;
                let taken: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE email = ?")
                    .bind(&email)
                    .fetch_optional(ctx.conn())
                    .await?;
                if taken.is_some() {
                    return Err(FieldErrors::one(
                        "account",
                        FieldErrorCode::InvalidValue,
                        None,
                    ));
                }
                let hash = identity::hash_password_blocking(account.password).await?;
                let user =
                    identity::create_user(ctx.conn(), &email, &name, &hash, false, now).await?;
                identity::start_session(ctx, &user).await?;
                user
            }
            (Caller::Anonymous { .. }, None) => {
                return Err(FieldErrors::one("account", FieldErrorCode::Required, None));
            }
            _ => {
                return Err(FieldErrors::one(
                    "account",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
        };

        let added = sqlx::query(
            "INSERT INTO grants (node_id, user_id, role, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (node_id, user_id) DO NOTHING",
        )
        .bind(node.get())
        .bind(user.as_str())
        .bind(role.as_str())
        .bind(ctx.now())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if added > 0 {
            sqlx::query("UPDATE invitations SET used_by = ?, used_at = ? WHERE id = ?")
                .bind(user.as_str())
                .bind(ctx.now())
                .bind(&id)
                .execute(ctx.conn())
                .await?;
            ctx.record(&Topic::user(&user), "tree.changed", json!({}))
                .await?;
            super::members_changed_below(ctx, node).await?;
            ctx.record(
                &Topic::members(node),
                "member.joined",
                json!({ "user": user, "invitation": id }),
            )
            .await?;
        }
        let account = identity::account(ctx.conn(), &user)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
        let factor = identity::two_factor_state(ctx.conn(), &user).await?;
        Ok(InvitationAccepted {
            node,
            account,
            enrollment_required: factor.required && !factor.enrolled,
        })
    }

    async fn invitations_preview(
        &self,
        ctx: &mut QueryCtx,
        args: InvitationCodeArgs,
    ) -> Result<Snapshot<InvitationPreview>, ApiError> {
        let now = crate::db::now_ms();
        let row: Option<(String, String, String, String, i64, bool)> = sqlx::query_as(
            "SELECT n.name, i.role, i.email, u.name, i.expires_at,
                    EXISTS (SELECT 1 FROM users a WHERE a.email = i.email)
             FROM invitations i JOIN nodes n ON n.id = i.node_id JOIN users u ON u.id = i.created_by
             WHERE i.code_hash = ? AND i.used_at IS NULL AND i.revoked_at IS NULL AND i.expires_at > ?",
        )
        .bind(identity::token_digest(args.code.trim()))
        .bind(now)
        .fetch_optional(ctx.conn())
        .await?;
        let (node_name, role, email, inviter_name, expires_at, account_exists) =
            row.ok_or_else(ApiError::not_found)?;
        Ok(Snapshot {
            topic: Topic::instance(),
            data: InvitationPreview {
                node_name,
                role: parse_role(&role)?,
                email,
                inviter_name,
                expires_at,
                account_exists,
            },
        })
    }
}

/// The pending invitation with this code: ID, node, role and email. Unknown, used, revoked and
/// expired codes are all `not_found`.
async fn pending(
    conn: &mut SqliteConnection,
    code: &str,
    now: i64,
) -> Result<(String, NodeId, Role, String), ApiError> {
    let row: Option<(String, i64, String, String)> = sqlx::query_as(
        "SELECT id, node_id, role, email FROM invitations
         WHERE code_hash = ? AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(identity::token_digest(code.trim()))
    .bind(now)
    .fetch_optional(conn)
    .await?;
    let (id, node, role, email) = row.ok_or_else(ApiError::not_found)?;
    Ok((id, NodeId::new(node), parse_role(&role)?, email))
}

/// `user` as a member of `node`, holding a grant on the node itself; `not_found` otherwise.
async fn granted_member(
    conn: &mut SqliteConnection,
    node: NodeId,
    user: &UserId,
) -> Result<Member, ApiError> {
    members(conn, node, Some(user))
        .await?
        .pop()
        .filter(|member| member.grants.iter().any(|grant| grant.node == node))
        .ok_or_else(ApiError::not_found)
}

/// Everyone with access to `node`, or only `only`: grants on the node and its ancestors, and
/// instance admins. By role, highest first, then by name.
async fn members(
    conn: &mut SqliteConnection,
    node: NodeId,
    only: Option<&UserId>,
) -> Result<Vec<Member>, ApiError> {
    let grants: Vec<(String, i64, String)> = sqlx::query_as(concat!(
        path_cte!(),
        "SELECT g.user_id, g.node_id, g.role FROM path p JOIN grants g ON g.node_id = p.id
         WHERE ?2 IS NULL OR g.user_id = ?2
         ORDER BY p.step"
    ))
    .bind(node.get())
    .bind(only.map(UserId::as_str))
    .fetch_all(&mut *conn)
    .await?;
    let people: Vec<(String, String, String, bool, bool)> = sqlx::query_as(concat!(
        path_cte!(),
        "SELECT u.id, u.name, u.email, u.instance_admin,
                EXISTS (SELECT 1 FROM two_factor t WHERE t.user_id = u.id AND t.secret IS NOT NULL)
         FROM users u
         WHERE (?2 IS NULL OR u.id = ?2)
           AND (u.instance_admin = 1
                OR EXISTS (SELECT 1 FROM grants g JOIN path p ON p.id = g.node_id
                           WHERE g.user_id = u.id))"
    ))
    .bind(node.get())
    .bind(only.map(UserId::as_str))
    .fetch_all(&mut *conn)
    .await?;

    let mut held: BTreeMap<String, Vec<MemberGrant>> = BTreeMap::new();
    for (user, node, role) in grants {
        held.entry(user).or_default().push(MemberGrant {
            node: NodeId::new(node),
            role: parse_role(&role)?,
        });
    }
    let mut members: Vec<Member> = people
        .into_iter()
        .map(|(id, name, email, instance_admin, two_factor_enrolled)| {
            let grants = held.remove(&id).unwrap_or_default();
            let role = grants
                .iter()
                .map(|grant| grant.role)
                .chain(instance_admin.then_some(Role::Owner))
                .max_by_key(|role| role.rank())
                .unwrap_or(Role::Viewer);
            Member {
                user: UserId::new(id),
                name,
                email,
                role,
                grants,
                instance_admin,
                two_factor_enrolled,
            }
        })
        .collect();
    members.sort_by(|a, b| {
        b.role
            .rank()
            .cmp(&a.role.rank())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.user.cmp(&b.user))
    });
    Ok(members)
}

macro_rules! select_invitation {
    () => {
        "SELECT id, node_id, role, email, prefix, created_by, created_at, expires_at, used_by,
                used_at, revoked_at
         FROM invitations"
    };
}
use select_invitation;

type InvitationRow = (
    String,
    i64,
    String,
    String,
    String,
    String,
    i64,
    i64,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

async fn invitation(
    conn: &mut SqliteConnection,
    id: &InvitationId,
    now: i64,
) -> Result<Invitation, ApiError> {
    let row: InvitationRow = sqlx::query_as(concat!(select_invitation!(), " WHERE id = ?"))
        .bind(id.as_str())
        .fetch_one(conn)
        .await?;
    invitation_of(row, now)
}

fn invitation_of(
    (
        id,
        node,
        role,
        email,
        prefix,
        created_by,
        created_at,
        expires_at,
        used_by,
        used_at,
        revoked_at,
    ): InvitationRow,
    now: i64,
) -> Result<Invitation, ApiError> {
    let state = if used_at.is_some() {
        InvitationState::Used
    } else if revoked_at.is_some() {
        InvitationState::Revoked
    } else if expires_at <= now {
        InvitationState::Expired
    } else {
        InvitationState::Pending
    };
    Ok(Invitation {
        id: InvitationId::new(id),
        node: NodeId::new(node),
        role: parse_role(&role)?,
        email,
        prefix,
        state,
        created_by: UserId::new(created_by),
        created_at,
        expires_at,
        used_by: used_by.map(UserId::new),
    })
}
