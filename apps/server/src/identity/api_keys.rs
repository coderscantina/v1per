//! `apiKeys.*` operations: scoped keys of the caller.
//!
//! A key never holds more than its owner: at creation every capability must be key-grantable
//! and held by the owner on the key's node, and every request resolves the owner's current
//! role again, so a lost grant takes the key's reach with it.

use std::collections::BTreeSet;

use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::{Empty, Topic};
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::{ApiKeyId, NodeId, UserId};
use v1per_protocol::limits::API_KEYS_PER_USER_MAX;
use v1per_protocol::operations::api_keys::{
    ApiKey, ApiKeyArgs, ApiKeyCreated, ApiKeyList, ApiKeyResult, ApiKeysCreateArgs, ApiKeysHandlers,
};
use v1per_protocol::registry::Snapshot;

use super::{new_session_token, signed_in, validate_name};
use crate::context::{CommandCtx, QueryCtx};
use crate::db::new_id;
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

/// Characters of the secret kept in clear to tell keys apart.
const PREFIX_CHARS: usize = 8;

impl ApiKeysHandlers for AppState {
    async fn api_keys_create(
        &self,
        ctx: &mut CommandCtx,
        args: ApiKeysCreateArgs,
    ) -> Result<ApiKeyCreated, ApiError> {
        let user = signed_in(ctx.caller())?;
        let scope = ctx.node_scope(args.node, Capability::NodeRead).await?;
        let capabilities: BTreeSet<Capability> = args.capabilities.into_iter().collect();
        if capabilities
            .iter()
            .any(|capability| !capability.key_grantable() || !scope.can(*capability))
        {
            return Err(ApiError::new(ErrorCode::GrantExceedsPrincipal));
        }
        ctx.require_fresh_login()?;

        let name = validate_name("name", &args.name)?;
        if capabilities.is_empty() {
            return Err(FieldErrors::one(
                "capabilities",
                FieldErrorCode::Required,
                None,
            ));
        }
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_keys WHERE owner_id = ? AND revoked_at IS NULL",
        )
        .bind(user.as_str())
        .fetch_one(ctx.conn())
        .await?;
        if active as usize >= API_KEYS_PER_USER_MAX {
            return Err(FieldErrors::one(
                "name",
                FieldErrorCode::TooMany,
                Some(API_KEYS_PER_USER_MAX as u32),
            ));
        }

        let (secret, digest) = new_session_token();
        let key = ApiKey {
            id: ApiKeyId::new(new_id()),
            name,
            prefix: secret[..PREFIX_CHARS].to_owned(),
            node: args.node,
            capabilities: capabilities.into_iter().collect(),
            created_at: ctx.now(),
            last_used_at: None,
            revoked_at: None,
        };
        sqlx::query(
            "INSERT INTO api_keys (id, owner_id, node_id, name, prefix, secret_hash, capabilities, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(key.id.as_str())
        .bind(user.as_str())
        .bind(key.node.get())
        .bind(&key.name)
        .bind(&key.prefix)
        .bind(digest)
        .bind(serde_json::to_string(&key.capabilities).map_err(ApiError::internal)?)
        .bind(key.created_at)
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::user(&user),
            "api_key.created",
            json!({ "key": key.id }),
        )
        .await?;
        Ok(ApiKeyCreated {
            key,
            secret: Some(secret),
            secret_withheld: false,
        })
    }

    async fn api_keys_revoke(
        &self,
        ctx: &mut CommandCtx,
        args: ApiKeyArgs,
    ) -> Result<ApiKeyResult, ApiError> {
        let user = signed_in(ctx.caller())?;
        let revoked = sqlx::query(
            "UPDATE api_keys SET revoked_at = ? WHERE id = ? AND owner_id = ? AND revoked_at IS NULL",
        )
        .bind(ctx.now())
        .bind(args.key.as_str())
        .bind(user.as_str())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        let key = list(ctx.conn(), &user, Some(&args.key))
            .await?
            .pop()
            .ok_or_else(ApiError::not_found)?;
        if revoked > 0 {
            ctx.record(
                &Topic::user(&user),
                "api_key.revoked",
                json!({ "key": key.id }),
            )
            .await?;
        }
        Ok(ApiKeyResult { key })
    }

    async fn api_keys_list(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<ApiKeyList>, ApiError> {
        let user = signed_in(ctx.caller())?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: ApiKeyList {
                keys: list(ctx.conn(), &user, None).await?,
            },
        })
    }
}

/// The owner's keys, newest first, or the one key `only`.
async fn list(
    conn: &mut SqliteConnection,
    owner: &UserId,
    only: Option<&ApiKeyId>,
) -> Result<Vec<ApiKey>, ApiError> {
    type Row = (
        String,
        String,
        String,
        i64,
        String,
        i64,
        Option<i64>,
        Option<i64>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, name, prefix, node_id, capabilities, created_at, last_used_at, revoked_at
         FROM api_keys WHERE owner_id = ?1 AND (?2 IS NULL OR id = ?2)
         ORDER BY created_at DESC, id",
    )
    .bind(owner.as_str())
    .bind(only.map(ApiKeyId::as_str))
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(
            |(id, name, prefix, node, capabilities, created_at, last_used_at, revoked_at)| {
                Ok(ApiKey {
                    id: ApiKeyId::new(id),
                    name,
                    prefix,
                    node: NodeId::new(node),
                    capabilities: serde_json::from_str(&capabilities)
                        .map_err(ApiError::internal)?,
                    created_at,
                    last_used_at,
                    revoked_at,
                })
            },
        )
        .collect()
}
