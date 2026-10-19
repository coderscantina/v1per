//! Who is calling, resolved once per request.
//!
//! A person sends the session cookie; an API key sends `Authorization: Bearer <secret>`, which
//! wins over a cookie. A login between password and second factor carries a challenge cookie
//! and is still anonymous.

use std::sync::Arc;

use axum::http::{HeaderMap, header};
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::errors::ErrorCode;
use v1per_protocol::ids::{ApiKeyId, NodeId, UserId};
use v1per_protocol::limits::{API_KEY_SCHEME, REAUTHENTICATION_WINDOW_MS, SESSION_IDLE_MS};
use v1per_protocol::registry::{Access, OperationInfo};

use super::{CHALLENGE_COOKIE, SESSION_COOKIE, token_digest, two_factor_state};
use crate::db::Db;
use crate::error::ApiError;

/// `last_seen_at` and `last_used_at` are written at most this often.
const TOUCH_EVERY_MS: i64 = 60_000;

/// The principal behind one request.
#[derive(Debug, Clone)]
pub enum Caller {
    Anonymous {
        address: String,
        /// Digest of the login challenge cookie between password and second factor.
        challenge: Option<String>,
    },
    Person {
        user: UserId,
        /// Digest of the session cookie, the session's ID.
        session: String,
        address: String,
        /// Must enroll a second factor first. Derived on every request, so a requirement
        /// switched on applies at once and confirming enrollment upgrades the same session.
        enrollment_only: bool,
        reauthenticated_at: Option<i64>,
    },
    Key(Arc<KeyCaller>),
}

/// An API key request. The key acts as `owner` on `node` and its subtree, limited to
/// `capabilities` and to what the owner currently holds there.
#[derive(Debug)]
pub struct KeyCaller {
    pub id: ApiKeyId,
    pub owner: UserId,
    pub node: NodeId,
    pub capabilities: Vec<Capability>,
    pub address: String,
}

impl Caller {
    pub fn address(&self) -> &str {
        match self {
            Caller::Anonymous { address, .. } | Caller::Person { address, .. } => address,
            Caller::Key(key) => &key.address,
        }
    }

    /// The signed-in person. `None` for API keys and anonymous callers.
    pub fn user(&self) -> Option<&UserId> {
        match self {
            Caller::Person { user, .. } => Some(user),
            _ => None,
        }
    }

    /// Owner of the caller's idempotency keys. A key's commands are its own, apart from its
    /// owner's browser sessions.
    pub fn key_owner(&self) -> String {
        match self {
            Caller::Person { user, .. } => format!("user:{user}"),
            Caller::Key(key) => format!("key:{}", key.id),
            Caller::Anonymous { address, .. } => format!("addr:{address}"),
        }
    }

    /// Refuses before any resource is looked up: person operations without a credential, API
    /// keys outside key operations, and enrollment-only sessions outside enrollment.
    pub fn admit(&self, op: &OperationInfo) -> Result<(), ApiError> {
        match self {
            Caller::Anonymous { .. } if op.access == Access::Person => {
                Err(ApiError::new(ErrorCode::Unauthenticated))
            }
            Caller::Key(_) if !op.policy.key => Err(ApiError::new(ErrorCode::OperationNotGranted)),
            Caller::Person {
                enrollment_only: true,
                ..
            } if op.access == Access::Person && !op.policy.enrollment => {
                Err(ApiError::new(ErrorCode::SecondFactorEnrollmentRequired))
            }
            _ => Ok(()),
        }
    }

    /// A passed password check within the last 10 minutes of this login session.
    pub fn require_fresh_login(&self, now: i64) -> Result<(), ApiError> {
        match self {
            Caller::Person {
                reauthenticated_at: Some(at),
                ..
            } if now - at < REAUTHENTICATION_WINDOW_MS => Ok(()),
            _ => Err(ApiError::new(ErrorCode::ReauthenticationRequired)),
        }
    }
}

/// Resolves the request's credential. No credential is an anonymous caller, and so is a
/// missing, unknown, expired or idle session cookie. A bad API key or `Authorization` header
/// is `unauthenticated` for every operation, so a revoked key looks like an unknown one.
pub async fn resolve_caller(
    db: &Db,
    headers: &HeaderMap,
    address: String,
    now: i64,
) -> Result<Caller, ApiError> {
    let mut conn = db.reader.acquire().await?;
    match authorization(headers) {
        Some(Some(secret)) => {
            let key = key(db, &mut conn, &token_digest(secret), address, now)
                .await?
                .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
            Ok(Caller::Key(Arc::new(key)))
        }
        Some(None) => Err(ApiError::new(ErrorCode::Unauthenticated)),
        None => person(db, &mut conn, headers, address, now).await,
    }
}

/// The bearer secret of an `Authorization` header: `None` without the header, `Some(None)`
/// for any other scheme or an empty token.
fn authorization(headers: &HeaderMap) -> Option<Option<&str>> {
    let value = headers.get(header::AUTHORIZATION)?;
    Some(
        value
            .to_str()
            .ok()
            .and_then(|v| v.split_once(' '))
            .filter(|(scheme, _)| *scheme == API_KEY_SCHEME)
            .map(|(_, token)| token.trim())
            .filter(|token| !token.is_empty()),
    )
}

/// Whether the request authenticates with an API key. Such commands skip the Origin check: a
/// browser cannot attach `Authorization` cross-site without a CORS preflight, which the server
/// never answers.
pub fn has_api_key(headers: &HeaderMap) -> bool {
    matches!(authorization(headers), Some(Some(_)))
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(cookie, _)| *cookie == name)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

async fn person(
    db: &Db,
    conn: &mut SqliteConnection,
    headers: &HeaderMap,
    address: String,
    now: i64,
) -> Result<Caller, ApiError> {
    let challenge = cookie(headers, CHALLENGE_COOKIE).map(token_digest);
    let Some(token) = cookie(headers, SESSION_COOKIE) else {
        return Ok(Caller::Anonymous { address, challenge });
    };
    let session = token_digest(token);
    let row: Option<(String, Option<i64>, i64)> = sqlx::query_as(
        "SELECT user_id, reauthenticated_at, last_seen_at FROM sessions
         WHERE id = ? AND expires_at > ? AND last_seen_at > ?",
    )
    .bind(&session)
    .bind(now)
    .bind(now - SESSION_IDLE_MS)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((user, reauthenticated_at, last_seen_at)) = row else {
        return Ok(Caller::Anonymous { address, challenge });
    };
    let user = UserId::new(user);
    let factor = two_factor_state(conn, &user).await?;
    if now - last_seen_at >= TOUCH_EVERY_MS {
        touch(
            db,
            "UPDATE sessions SET last_seen_at = ? WHERE id = ?",
            session.clone(),
            now,
        );
    }
    Ok(Caller::Person {
        user,
        session,
        address,
        enrollment_only: factor.required && !factor.enrolled,
        reauthenticated_at,
    })
}

/// Records use of a session or key off the request path. Reads stay off the single writer;
/// at most one write a minute per credential.
fn touch(db: &Db, sql: &'static str, id: String, now: i64) {
    let writer = db.writer.clone();
    tokio::spawn(async move {
        if let Err(error) = sqlx::query(sql).bind(now).bind(id).execute(&writer).await {
            tracing::warn!(%error, "could not record credential use");
        }
    });
}

/// The active key with this secret digest.
async fn key(
    db: &Db,
    conn: &mut SqliteConnection,
    secret_hash: &str,
    address: String,
    now: i64,
) -> Result<Option<KeyCaller>, ApiError> {
    let row: Option<(String, String, i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, owner_id, node_id, capabilities, last_used_at FROM api_keys
         WHERE secret_hash = ? AND revoked_at IS NULL",
    )
    .bind(secret_hash)
    .fetch_optional(conn)
    .await?;
    let Some((id, owner, node, capabilities, last_used_at)) = row else {
        return Ok(None);
    };
    if last_used_at.is_none_or(|at| now - at >= TOUCH_EVERY_MS) {
        touch(
            db,
            "UPDATE api_keys SET last_used_at = ? WHERE id = ?",
            id.clone(),
            now,
        );
    }
    Ok(Some(KeyCaller {
        id: ApiKeyId::new(id),
        owner: UserId::new(owner),
        node: NodeId::new(node),
        capabilities: serde_json::from_str(&capabilities).map_err(ApiError::internal)?,
        address,
    }))
}
