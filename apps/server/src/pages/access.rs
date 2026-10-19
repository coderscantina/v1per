//! Who may read a page, decided per request and again while a stream is open.
//!
//! A grant holder on the page's node (a signed-in person whose scope resolves there) gets the
//! internal audience on any page. Everyone else gets the public audience on a public page,
//! and on a restricted page with a valid share token in the request or a page cookie from the
//! password, a sign-in link or a share token. Each cookie is checked against the page's
//! current settings: a password session needs the password to still be set, an email session
//! its address to still be allowed, a token session its token to still exist and not expire.

use axum::http::{HeaderMap, header};
use sqlx::SqliteConnection;
use v1per_protocol::limits::SESSION_LIFETIME_MS;
use v1per_protocol::status_page::PageAudience;

use super::PageRow;
use crate::error::ApiError;
use crate::identity::{self, Caller, token_digest};
use crate::nodes::scope;
use crate::state::AppState;

/// The page cookie, scoped to the page's path on the instance host and to `/` on a claimed
/// hostname.
pub(super) const PAGE_COOKIE: &str = "v1per_page";

/// How a visitor got a page session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Password,
    Email,
    Token,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Password => "password",
            Kind::Email => "email",
            Kind::Token => "token",
        }
    }
}

/// What the request carries besides headers: a `?token=` or `/t/<token>` share token.
pub(super) struct Visitor<'a> {
    pub headers: &'a HeaderMap,
    pub address: &'a str,
    pub token: Option<&'a str>,
}

/// The audience `visitor` gets on `page`, or `None` when the page is closed to them.
pub(super) async fn audience(
    state: &AppState,
    page: &PageRow,
    visitor: &Visitor<'_>,
) -> Result<Option<PageAudience>, ApiError> {
    let now = state.clock.now_ms();
    // A broken or foreign credential is no credential here; the page is not the admin API.
    let caller =
        identity::resolve_caller(&state.db, visitor.headers, visitor.address.to_owned(), now)
            .await
            .unwrap_or(Caller::Anonymous {
                address: visitor.address.to_owned(),
                challenge: None,
            });
    let mut conn = state.db.reader.acquire().await?;
    if let Caller::Person {
        enrollment_only: false,
        ..
    } = &caller
        && scope::resolve(&mut conn, &caller, page.node())
            .await
            .is_ok()
    {
        return Ok(Some(PageAudience::Internal));
    }
    if page.public {
        return Ok(Some(PageAudience::Public));
    }
    if let Some(token) = visitor.token
        && share_token(&mut conn, page, token, now).await?.is_some()
    {
        return Ok(Some(PageAudience::Public));
    }
    for cookie in cookies(visitor.headers) {
        if session_valid(&mut conn, page, cookie, now).await? {
            return Ok(Some(PageAudience::Public));
        }
    }
    Ok(None)
}

/// The share token's ID and expiry when `token` opens `page` now.
pub(super) async fn share_token(
    conn: &mut SqliteConnection,
    page: &PageRow,
    token: &str,
    now: i64,
) -> Result<Option<(i64, Option<i64>)>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT id, expires_at FROM page_share_tokens
         WHERE token_hash = ? AND page_id = ? AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(token_digest(token))
    .bind(page.id)
    .bind(now)
    .fetch_optional(conn)
    .await?)
}

/// Every page cookie the request carries; several pages' cookies can share a request.
fn cookies(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, value)| *name == PAGE_COOKIE && !value.is_empty())
        .map(|(_, value)| value)
}

async fn session_valid(
    conn: &mut SqliteConnection,
    page: &PageRow,
    cookie: &str,
    now: i64,
) -> Result<bool, ApiError> {
    let row: Option<(String, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT kind, email, share_token_id FROM page_sessions
         WHERE id = ? AND page_id = ? AND expires_at > ?",
    )
    .bind(token_digest(cookie))
    .bind(page.id)
    .bind(now)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match row {
        None => false,
        Some((kind, email, token)) => match kind.as_str() {
            "password" => page.password_hash.is_some(),
            "email" => email.is_some_and(|email| page.allows_email(&email)),
            "token" => {
                let open: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM page_share_tokens
                     WHERE id = ? AND (expires_at IS NULL OR expires_at > ?)",
                )
                .bind(token)
                .bind(now)
                .fetch_optional(conn)
                .await?;
                open.is_some()
            }
            _ => false,
        },
    })
}

/// Starts a page session and returns its cookie value. It ends after the login session
/// lifetime, or earlier at `until`.
pub(super) async fn start(
    conn: &mut SqliteConnection,
    page: &PageRow,
    kind: Kind,
    email: Option<&str>,
    token: Option<i64>,
    until: Option<i64>,
    now: i64,
) -> Result<(String, i64), ApiError> {
    let (secret, digest) = identity::new_session_token();
    let expires_at = until.map_or(now + SESSION_LIFETIME_MS, |until| {
        until.min(now + SESSION_LIFETIME_MS)
    });
    sqlx::query(
        "INSERT INTO page_sessions (id, page_id, kind, email, share_token_id, expires_at,
                                    created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(digest)
    .bind(page.id)
    .bind(kind.as_str())
    .bind(email)
    .bind(token)
    .bind(expires_at)
    .bind(now)
    .execute(conn)
    .await?;
    Ok((secret, expires_at))
}

/// The `Set-Cookie` value for a page session under `path`.
pub(super) fn cookie(value: &str, path: &str, expires_at: i64, now: i64, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    let path = if path.is_empty() { "/" } else { path };
    format!(
        "{PAGE_COOKIE}={value}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        (expires_at - now).max(0) / 1000
    )
}
