//! `account.*` operations: the caller's own account, grants and preferences.

use serde_json::json;
use v1per_protocol::envelope::{Empty, Topic};
use v1per_protocol::errors::ErrorCode;
use v1per_protocol::ids::NodeId;
use v1per_protocol::operations::account::{
    AccountHandlers, AccountResult, ChangePasswordArgs, Grant, GrantList, Preferences,
};
use v1per_protocol::registry::Snapshot;

use super::{
    Caller, account, end_sessions, hash_password_blocking, password_hash, signed_in,
    validate_password, verify_password,
};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::ApiError;
use crate::nodes::scope::parse_role;
use crate::state::AppState;

impl AccountHandlers for AppState {
    async fn account_change_password(
        &self,
        ctx: &mut CommandCtx,
        args: ChangePasswordArgs,
    ) -> Result<Empty, ApiError> {
        self.login_limiter.check(ctx.caller().address())?;
        let Caller::Person { user, session, .. } = ctx.caller().clone() else {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        };
        let hash = password_hash(ctx.conn(), &user).await?;
        if !verify_password(hash, args.current_password).await {
            return Err(ApiError::new(ErrorCode::InvalidCredentials));
        }
        validate_password("new_password", &args.new_password)?;
        let hash = hash_password_blocking(args.new_password).await?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(hash)
            .bind(user.as_str())
            .execute(ctx.conn())
            .await?;
        end_sessions(ctx.conn(), &user, Some(&session)).await?;
        ctx.record(&Topic::user(&user), "account.password_changed", json!({}))
            .await?;
        Ok(Empty {})
    }

    async fn account_update_preferences(
        &self,
        ctx: &mut CommandCtx,
        args: Preferences,
    ) -> Result<Preferences, ApiError> {
        let user = signed_in(ctx.caller())?;
        sqlx::query("UPDATE users SET preferences = ? WHERE id = ?")
            .bind(serde_json::to_string(&args).expect("preferences serialize"))
            .bind(user.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::user(&user),
            "account.preferences_changed",
            json!({}),
        )
        .await?;
        Ok(args)
    }

    async fn account_get(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<AccountResult>, ApiError> {
        let user = signed_in(ctx.caller())?;
        let created_at: i64 = sqlx::query_scalar("SELECT created_at FROM users WHERE id = ?")
            .bind(user.as_str())
            .fetch_one(ctx.conn())
            .await?;
        let account = account(ctx.conn(), &user)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: AccountResult {
                account,
                created_at,
            },
        })
    }

    async fn account_grants(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<GrantList>, ApiError> {
        let user = signed_in(ctx.caller())?;
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT g.node_id, n.name, g.role FROM grants g JOIN nodes n ON n.id = g.node_id
             WHERE g.user_id = ? ORDER BY n.name COLLATE NOCASE, n.id",
        )
        .bind(user.as_str())
        .fetch_all(ctx.conn())
        .await?;
        let grants = rows
            .into_iter()
            .map(|(node, name, role)| {
                Ok(Grant {
                    node: NodeId::new(node),
                    name,
                    role: parse_role(&role)?,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: GrantList { grants },
        })
    }

    async fn account_preferences(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<Preferences>, ApiError> {
        let user = signed_in(ctx.caller())?;
        let stored: String = sqlx::query_scalar("SELECT preferences FROM users WHERE id = ?")
            .bind(user.as_str())
            .fetch_one(ctx.conn())
            .await?;
        // A preset a later release dropped reads as the default, not as an error.
        let preferences = serde_json::from_str(&stored).unwrap_or_default();
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: preferences,
        })
    }
}
