//! Page logos: `POST /api/pages/{page}/logo` takes the raw image from a page manager, and the
//! public `<base>/logo` serves it. Only PNG, JPEG, GIF, WebP and SVG are kept, recognized by
//! their bytes, never by a client's content type. SVG is served under a sandboxing content
//! security policy, so a script inside it never runs, not even when opened directly.

use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::PageId;
use v1per_protocol::limits::PAGE_LOGO_MAX_BYTES;
use v1per_protocol::registry::result_object;

use super::{PageRow, bump, changed, detail};
use crate::context::{CommandCtx, CommandParts};
use crate::error::{ApiError, FieldErrors};
use crate::events;
use crate::http::client_address;
use crate::http::operations::check_origin;
use crate::identity::{self, Caller};
use crate::state::AppState;

/// Logos change only with their URL's `v`, so browsers keep them for a day.
const CACHE: &str = "public, max-age=86400";

/// The logo's path below the page's base, versioned by its upload time.
pub(super) fn path(row: &PageRow) -> Option<String> {
    row.logo_type
        .as_ref()
        .map(|_| format!("/logo?v={}", row.logo_updated_at.unwrap_or_default()))
}

/// The image type of `bytes`, if it is one pages accept.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.trim_start_matches('\u{feff}').trim_start();
    (start.starts_with('<') && text.contains("<svg")).then_some("image/svg+xml")
}

/// `POST /api/pages/{page}/logo` with the image as the body. Answers like a command: the
/// page detail with `revision`.
pub(crate) async fn upload(
    State(state): State<AppState>,
    Path(page): Path<String>,
    request: Request,
) -> Response {
    match run(&state, &page, request).await {
        Ok(answer) => {
            let mut response = axum::Json(answer).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Err(error) => error.into_response(),
    }
}

async fn run(state: &AppState, page: &str, request: Request) -> Result<Value, ApiError> {
    let (parts, body) = request.into_parts();
    check_origin(&state.config, &parts.headers)?;
    let page: PageId = page.parse().map_err(|_| ApiError::not_found())?;
    let now = state.clock.now_ms();
    let caller =
        identity::resolve_caller(&state.db, &parts.headers, client_address(&parts), now).await?;
    match &caller {
        Caller::Person {
            enrollment_only: false,
            ..
        } => {}
        Caller::Person { .. } => {
            return Err(ApiError::new(ErrorCode::SecondFactorEnrollmentRequired));
        }
        Caller::Key(_) => return Err(ApiError::new(ErrorCode::OperationNotGranted)),
        Caller::Anonymous { .. } => return Err(ApiError::new(ErrorCode::Unauthenticated)),
    }
    let bytes = axum::body::to_bytes(body, PAGE_LOGO_MAX_BYTES)
        .await
        .map_err(|_| ApiError::new(ErrorCode::PayloadTooLarge))?;

    let tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let mut ctx = CommandCtx::new(tx, caller, now);
    let row = super::load(ctx.conn(), page).await?;
    ctx.node_scope(row.node(), Capability::PagesManage).await?;
    let content_type = sniff(&bytes)
        .ok_or_else(|| FieldErrors::one("logo", FieldErrorCode::InvalidFormat, None))?;
    sqlx::query("UPDATE pages SET logo = ?, logo_type = ?, logo_updated_at = ? WHERE id = ?")
        .bind(bytes.as_ref())
        .bind(content_type)
        .bind(now)
        .bind(row.id)
        .execute(ctx.conn())
        .await?;
    bump(&mut ctx, page).await?;
    changed(&mut ctx, &row, "page.updated").await?;
    let mut answer = result_object(&detail(ctx.conn(), page).await?);
    let CommandParts {
        mut tx,
        last_event,
        touched,
        ..
    } = ctx.into_parts();
    let revision = match last_event {
        Some(seq) => seq,
        None => events::head(&mut tx, &Topic::instance()).await?,
    };
    tx.commit().await?;
    for topic in &touched {
        state.hub.publish(topic);
    }
    answer.insert("revision".into(), revision.into());
    Ok(Value::Object(answer))
}

/// The logo of `page`, or a 404 without one.
pub(super) async fn serve(state: &AppState, page: &PageRow) -> Response {
    let stored: Result<Option<(Vec<u8>, String)>, sqlx::Error> = async {
        let mut conn = state.db.reader.acquire().await?;
        sqlx::query_as("SELECT logo, logo_type FROM pages WHERE id = ? AND logo IS NOT NULL")
            .bind(page.id)
            .fetch_optional(&mut *conn)
            .await
    }
    .await;
    match stored {
        Ok(Some((bytes, content_type))) => (
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, CACHE.to_owned()),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "default-src 'none'; style-src 'unsafe-inline'; sandbox".to_owned(),
                ),
                (header::CONTENT_DISPOSITION, "inline".to_owned()),
            ],
            bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => ApiError::internal(error).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::sniff;

    #[test]
    fn only_image_bytes_pass() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(
            sniff(b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            Some("image/svg+xml")
        );
        assert_eq!(sniff(b"<html><script>alert(1)</script></html>"), None);
        assert_eq!(sniff(b"MZ\x90\0"), None);
    }
}
