//! The public page routes, at `/s/{slug}` on the instance and at the root of claimed
//! hostnames (`docs/contracts/status-page-api.md`). They take no admin session, answer per
//! audience, and never serve the admin app or API on a claimed hostname.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Request, State};
use axum::handler::HandlerWithoutStateExt;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::watch;
use tokio::time::Instant;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::{MonitorId, PageComponentId, PageId};
use v1per_protocol::limits::{PAGE_EMAIL_LINK_MS, PAGE_PATH};
use v1per_protocol::live::{LiveEvent, LiveFrame, ResyncReason};
use v1per_protocol::status_page::{
    Locale, PageAudience, PageBoot, PageEmailBody, PageGate, PagePasswordBody, PageRedeemBody,
};
use v1per_storage::UptimeWindow;

use super::access::{self, Kind, Visitor};
use super::{PageRow, badge, logo, mail, origin_host, select_page, snapshot, text};
use crate::error::{ApiError, FieldErrors};
use crate::http::client_address;
use crate::http::operations::same_origin;
use crate::identity::{self, validate_email};
use crate::instance;
use crate::state::AppState;
use crate::{events, identity::verify_password};

/// Re-checks access at least this often while a stream is idle.
const RECHECK: Duration = Duration::from_secs(5);
/// Waits this long after a wake before reading, so a burst of changes is one frame.
const COALESCE: Duration = Duration::from_millis(250);
const PAGE: usize = 500;
/// Largest body of an access request.
const ACCESS_BODY_MAX: usize = 4 * 1024;
/// Anonymous visitors of a public page may get the HTML and JSON from a shared cache briefly.
const PUBLIC_CACHE: &str = "public, max-age=15";
const PRIVATE: &str = "private, no-store";

/// Set by [`host_routing`] on requests to a claimed hostname.
#[derive(Clone, Copy)]
struct HostPage(i64);

/// The page a request is for, and the path prefix of its routes.
struct Located {
    page: PageRow,
    /// `/s/<slug>`, or empty on a claimed hostname.
    base: String,
    claimed: bool,
}

impl FromRequestParts<AppState> for Located {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let not_found = || ApiError::not_found().into_response();
        let mut conn = state
            .db
            .reader
            .acquire()
            .await
            .map_err(|error| ApiError::from(error).into_response())?;
        if let Some(HostPage(id)) = parts.extensions.get::<HostPage>().copied() {
            let page = super::load(&mut conn, PageId::new(id))
                .await
                .map_err(IntoResponse::into_response)?;
            return Ok(Located {
                page,
                base: String::new(),
                claimed: true,
            });
        }
        let params = axum::extract::RawPathParams::from_request_parts(parts, state)
            .await
            .map_err(|_| not_found())?;
        let slug = params
            .iter()
            .find(|(name, _)| *name == "slug")
            .map(|(_, value)| value.to_owned())
            .ok_or_else(not_found)?;
        let page: PageRow = sqlx::query_as(concat!(select_page!(), " WHERE slug = ?"))
            .bind(&slug)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|error| ApiError::from(error).into_response())?
            .ok_or_else(not_found)?;
        Ok(Located {
            base: page.path(),
            page,
            claimed: false,
        })
    }
}

#[derive(Default, Deserialize)]
struct Params {
    token: Option<String>,
    locale: Option<String>,
    cursor: Option<String>,
    component: Option<String>,
    limit: Option<String>,
}

fn params(parts: &Parts) -> Params {
    serde_html_form::from_str(parts.uri.query().unwrap_or_default()).unwrap_or_default()
}

/// The page routes, relative to the page's base.
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(html))
        .route("/t/{token}", get(html))
        .route("/access/email", get(html).post(request_link))
        .route("/access/email/redeem", post(redeem))
        .route("/access/password", post(password))
        .route("/snapshot.json", get(snapshot_json))
        .route("/incidents.json", get(incidents_json))
        .route("/events", get(stream))
        .route("/badge/{name}", get(badge))
        .route("/logo", get(logo_file))
        .layer(DefaultBodyLimit::max(ACCESS_BODY_MAX))
}

/// The router for claimed hostnames: the page routes at the root and the built assets.
pub(crate) fn hostname_router(state: AppState) -> Router {
    let mut router = routes();
    if let Some(dir) = state.config.web_dir.clone() {
        let missing = || async { StatusCode::NOT_FOUND };
        router = router
            .nest_service(
                "/assets",
                ServeDir::new(dir.join("assets")).fallback(missing.into_service()),
            )
            .route_service("/favicon.svg", ServeFile::new(dir.join("favicon.svg")));
    }
    router
        .fallback(|| async { ApiError::not_found() })
        .with_state(state)
}

/// Routes requests whose `Host` a page claimed to that page, unless the host is the
/// instance's own public one.
pub(crate) async fn host_routing(
    State((state, hostnames)): State<(AppState, Router)>,
    mut request: Request,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| request.uri().host())
        .map(|host| {
            let host = host.rsplit_once(':').map_or(host, |(host, _)| host);
            host.trim_end_matches('.').to_ascii_lowercase()
        });
    let own = state.config.public_origin.as_deref().and_then(origin_host);
    if let Some(host) = host
        && Some(&host) != own.as_ref()
    {
        let claimed: Result<Option<i64>, sqlx::Error> = async {
            let mut conn = state.db.reader.acquire().await?;
            sqlx::query_scalar("SELECT page_id FROM page_hostnames WHERE hostname = ?")
                .bind(&host)
                .fetch_optional(&mut *conn)
                .await
        }
        .await;
        match claimed {
            Ok(Some(page)) => {
                request.extensions_mut().insert(HostPage(page));
                return match hostnames.oneshot(request).await {
                    Ok(response) => response,
                    Err(never) => match never {},
                };
            }
            Ok(None) => {}
            Err(error) => return ApiError::from(error).into_response(),
        }
    }
    next.run(request).await
}

/// The locale a request asks for among the page's: `?locale=`, then `Accept-Language`, then
/// the page's default.
fn pick_locale(page: &PageRow, asked: Option<&str>, headers: &HeaderMap) -> Locale {
    let enabled = page.locales();
    let accepted = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(',')
        .filter_map(|range| range.split(';').next())
        .filter_map(|tag| tag.trim().split('-').next())
        .filter_map(|tag| Locale::parse(&tag.to_ascii_lowercase()));
    asked
        .and_then(Locale::parse)
        .into_iter()
        .chain(accepted)
        .find(|locale| enabled.contains(locale))
        .unwrap_or_else(|| page.default_locale())
}

fn visitor<'a>(parts: &'a Parts, address: &'a str, token: Option<&'a str>) -> Visitor<'a> {
    Visitor {
        headers: &parts.headers,
        address,
        token,
    }
}

/// `private, no-store` unless an anonymous visitor reads a public page.
fn cache_header(
    page: &PageRow,
    audience: Option<PageAudience>,
    headers: &HeaderMap,
) -> &'static str {
    let anonymous = !headers.contains_key(header::COOKIE);
    if page.public && anonymous && audience == Some(PageAudience::Public) {
        PUBLIC_CACHE
    } else {
        PRIVATE
    }
}

/// JSON for a `<script type="application/json">`: `<`, `>` and `&` escaped so the text can
/// never close the element or open a comment, and the JavaScript line separators too.
fn script_json(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// `status.html` with the boot JSON before `</head>`, the page title and the language set.
fn inject(template: &str, boot: &str, title: &str, locale: Locale) -> String {
    let mut html = template.replacen(
        "<html lang=\"en\">",
        &format!("<html lang=\"{}\">", locale.as_str()),
        1,
    );
    if let (Some(start), Some(end)) = (html.find("<title>"), html.find("</title>"))
        && start < end
    {
        html.replace_range(start + "<title>".len()..end, &escape_html(title));
    }
    let script = format!(
        "<script id=\"v1per-page\" type=\"application/json\">{}</script>",
        script_json(boot)
    );
    match html.find("</head>") {
        Some(at) => html.insert_str(at, &script),
        None => html.push_str(&script),
    }
    html
}

async fn gate(state: &AppState, located: &Located, locale: Locale) -> Result<PageGate, ApiError> {
    let page = &located.page;
    let sign_in_url = if located.claimed {
        let mut conn = state.db.reader.acquire().await?;
        instance::effective(&mut conn, &state.config)
            .await?
            .public_url
            .map(|url| format!("{url}/login?redirect={}", page.path()))
    } else {
        Some(format!("/login?redirect={}", page.path()))
    };
    Ok(PageGate {
        appearance: snapshot::appearance(page),
        locale,
        password: page.password_hash.is_some(),
        email: page.email_access(),
        sign_in_url,
    })
}

/// `<base>`, `<base>/t/<token>` and `<base>/access/email`: the page app with its boot JSON.
async fn html(State(state): State<AppState>, located: Located, request: Request) -> Response {
    match render(&state, &located, request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn render(
    state: &AppState,
    located: &Located,
    request: Request,
) -> Result<Response, ApiError> {
    let (parts, _) = request.into_parts();
    let Some(dir) = &state.config.web_dir else {
        return Ok((
            StatusCode::NOT_FOUND,
            "No web app configured. Set V1PER_WEB_DIR.",
        )
            .into_response());
    };
    let template = tokio::fs::read_to_string(dir.join("status.html"))
        .await
        .map_err(|error| ApiError::internal(format!("status.html: {error}")))?;
    let query = params(&parts);
    let path_token = parts
        .uri
        .path()
        .rsplit_once("/t/")
        .map(|(_, token)| token.to_owned());
    let token = path_token.or(query.token.clone());
    let address = client_address(&parts);
    let page = &located.page;
    let audience =
        access::audience(state, page, &visitor(&parts, &address, token.as_deref())).await?;
    let locale = pick_locale(page, query.locale.as_deref(), &parts.headers);

    let mut cookie = None;
    if audience == Some(PageAudience::Public)
        && !page.public
        && let Some(token) = &token
    {
        // A share token opens a session, so the page's later requests need no token.
        let now = state.clock.now_ms();
        let mut tx = state.db.writer.begin().await?;
        if let Some((id, expires_at)) = access::share_token(&mut tx, page, token, now).await? {
            let (value, until) =
                access::start(&mut tx, page, Kind::Token, None, Some(id), expires_at, now).await?;
            cookie = Some(access::cookie(
                &value,
                &located.base,
                until,
                now,
                state.config.secure_cookies(),
            ));
        }
        tx.commit().await?;
    }

    let boot = match audience {
        Some(audience) => {
            let built = snapshot::cached(state, page, audience, locale).await?;
            format!(
                "{{\"kind\":\"page\",\"base\":{},\"snapshot\":{}}}",
                serde_json::Value::String(located.base.clone()),
                built.json
            )
        }
        None => serde_json::to_string(&PageBoot::Gate {
            base: located.base.clone(),
            gate: gate(state, located, locale).await?,
        })
        .map_err(ApiError::internal)?,
    };
    let mut response = (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                cache_header(page, audience, &parts.headers),
            ),
            (header::VARY, "Cookie, Accept-Language"),
        ],
        inject(&template, &boot, &page.title, locale),
    )
        .into_response();
    if let Some(cookie) = cookie.and_then(|cookie| HeaderValue::from_str(&cookie).ok()) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

/// `<base>/snapshot.json`: the snapshot, or `unauthenticated` when the page is closed to the
/// visitor.
async fn snapshot_json(State(state): State<AppState>, located: Located, parts: Parts) -> Response {
    let query = params(&parts);
    let address = client_address(&parts);
    let page = &located.page;
    let answer = async {
        let audience = access::audience(
            &state,
            page,
            &visitor(&parts, &address, query.token.as_deref()),
        )
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
        let locale = pick_locale(page, query.locale.as_deref(), &parts.headers);
        let built = snapshot::cached(&state, page, audience, locale).await?;
        Ok::<_, ApiError>((audience, built))
    };
    match answer.await {
        Ok((audience, built)) => (
            [
                (header::CONTENT_TYPE, "application/json"),
                (
                    header::CACHE_CONTROL,
                    cache_header(page, Some(audience), &parts.headers),
                ),
                (header::VARY, "Cookie, Accept-Language"),
            ],
            built.json.to_string(),
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}

/// Incidents an `incidents.json` page lists by default and at most.
const HISTORY_PAGE: u32 = 20;
const HISTORY_PAGE_MAX: u32 = 100;

/// `<base>/incidents.json?cursor=&limit=`: the page's incident history, newest first, one
/// page at a time, with the snapshot's access and audience.
async fn incidents_json(State(state): State<AppState>, located: Located, parts: Parts) -> Response {
    let query = params(&parts);
    let address = client_address(&parts);
    let page = &located.page;
    let answer = async {
        let mut errors = FieldErrors::default();
        let limit = match query.limit.as_deref().map(str::parse::<u32>) {
            None => HISTORY_PAGE,
            Some(Ok(limit)) if (1..=HISTORY_PAGE_MAX).contains(&limit) => limit,
            Some(_) => {
                errors.add(
                    "limit",
                    FieldErrorCode::InvalidValue,
                    Some(HISTORY_PAGE_MAX),
                );
                HISTORY_PAGE
            }
        };
        let after = query.cursor.as_deref().map(snapshot::HistoryCursor::parse);
        if after.is_some_and(|cursor| cursor.is_none()) {
            errors.add("cursor", FieldErrorCode::InvalidValue, None);
        }
        errors.finish()?;
        let audience = access::audience(
            &state,
            page,
            &visitor(&parts, &address, query.token.as_deref()),
        )
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
        let locale = pick_locale(page, query.locale.as_deref(), &parts.headers);
        let history =
            snapshot::incident_history(&state, page, audience, locale, after.flatten(), limit)
                .await?;
        Ok::<_, ApiError>((audience, history))
    };
    match answer.await {
        Ok((audience, history)) => (
            [
                (
                    header::CACHE_CONTROL,
                    cache_header(page, Some(audience), &parts.headers),
                ),
                (header::VARY, "Cookie, Accept-Language"),
            ],
            axum::Json(history),
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}

/// `<base>/badge/{status,uptime-24h,uptime-30d,uptime-90d}.svg`, for public pages and share
/// tokens, of the page or of one `?component=`.
async fn badge(
    State(state): State<AppState>,
    located: Located,
    axum::extract::Path(params_path): axum::extract::Path<Vec<(String, String)>>,
    parts: Parts,
) -> Response {
    let name = params_path
        .iter()
        .find(|(key, _)| key == "name")
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    match render_badge(&state, &located, name, &parts).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn render_badge(
    state: &AppState,
    located: &Located,
    name: &str,
    parts: &Parts,
) -> Result<Response, ApiError> {
    let page = &located.page;
    let query = params(parts);
    let now = state.clock.now_ms();
    if !page.public {
        let mut conn = state.db.reader.acquire().await?;
        let opened = match &query.token {
            Some(token) => access::share_token(&mut conn, page, token, now)
                .await?
                .is_some(),
            None => false,
        };
        if !opened {
            return Err(ApiError::not_found());
        }
    }
    let component: Option<PageComponentId> = match &query.component {
        Some(id) => Some(id.parse().map_err(|_| ApiError::not_found())?),
        None => None,
    };
    let locale = page.default_locale();
    let built = snapshot::cached(state, page, PageAudience::Public, locale).await?;
    let shown = built
        .snapshot
        .groups
        .iter()
        .flat_map(|group| &group.components)
        .find(|shown| Some(shown.id) == component);
    if component.is_some() && shown.is_none() {
        return Err(ApiError::not_found());
    }
    let monitors: Vec<MonitorId> = {
        let mut conn = state.db.reader.acquire().await?;
        sqlx::query_scalar(
            "SELECT monitor_id FROM page_components
             WHERE page_id = ?1 AND internal = 0 AND monitor_id IS NOT NULL
               AND (?2 IS NULL OR id = ?2)",
        )
        .bind(page.id)
        .bind(component.map(PageComponentId::get))
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(MonitorId::new)
        .collect()
    };
    let svg = match name {
        "status.svg" => badge::status(
            locale,
            shown.map_or(Some(built.snapshot.status), |shown| shown.status),
        ),
        "uptime-24h.svg" => badge::uptime(
            locale,
            "24h",
            snapshot::uptime(state, &monitors, UptimeWindow::Day).await?,
        ),
        "uptime-30d.svg" => badge::uptime(
            locale,
            "30d",
            snapshot::uptime(state, &monitors, UptimeWindow::Month).await?,
        ),
        "uptime-90d.svg" => badge::uptime(
            locale,
            "90d",
            match shown {
                Some(shown) => shown.uptime_90d,
                None => snapshot::uptime_90d(state, page, &monitors).await?,
            },
        ),
        _ => return Err(ApiError::not_found()),
    };
    let cache = if page.public {
        "public, max-age=60"
    } else {
        "private, max-age=60"
    };
    Ok((
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, cache),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'; sandbox",
            ),
        ],
        svg,
    )
        .into_response())
}

async fn logo_file(State(state): State<AppState>, located: Located) -> Response {
    logo::serve(&state, &located.page).await
}

/// Access requests must come from the page's own origin as JSON.
fn check_post(state: &AppState, located: &Located, headers: &HeaderMap) -> Result<(), ApiError> {
    let rejected = || ApiError::new(ErrorCode::OriginRejected);
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(rejected)?;
    let allowed = if located.claimed {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let origin = origin.to_ascii_lowercase();
        origin == format!("https://{host}") || origin == format!("http://{host}")
    } else {
        same_origin(&state.config, headers, origin)
    };
    if allowed { Ok(()) } else { Err(rejected()) }
}

fn json_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|error| ApiError::with_message(ErrorCode::MalformedRequest, error.to_string()))
}

fn signed_in(cookie: String) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `POST <base>/access/password`: a page cookie for the right password.
async fn password(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let address = client_address(&parts);
    let answer = async {
        check_post(&state, &located, &parts.headers)?;
        let body: PagePasswordBody = json_body(&body)?;
        state.page_attempts.check(&address)?;
        let page = &located.page;
        let hash = page
            .password_hash
            .clone()
            .ok_or_else(|| ApiError::new(ErrorCode::InvalidCredentials))?;
        if !verify_password(hash, body.password).await {
            return Err(ApiError::new(ErrorCode::InvalidCredentials));
        }
        let now = state.clock.now_ms();
        let mut tx = state.db.writer.begin().await?;
        let (value, until) =
            access::start(&mut tx, page, Kind::Password, None, None, None, now).await?;
        tx.commit().await?;
        Ok::<_, ApiError>(access::cookie(
            &value,
            &located.base,
            until,
            now,
            state.config.secure_cookies(),
        ))
    };
    match answer.await {
        Ok(cookie) => signed_in(cookie),
        Err(error) => error.into_response(),
    }
}

/// `POST <base>/access/email`: mails a sign-in link to an allowed address. Answers `202`
/// whether or not the address is allowed, so the list cannot be probed.
async fn request_link(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let address = client_address(&parts);
    let answer = async {
        check_post(&state, &located, &parts.headers)?;
        let body: PageEmailBody = json_body(&body)?;
        state.page_attempts.check(&address)?;
        let email = validate_email("email", &body.email)?;
        state.page_mails.check(&email)?;
        let page = &located.page;
        if !page.allows_email(&email) {
            return Ok(());
        }
        let mut conn = state.db.reader.acquire().await?;
        let public_url = instance::effective(&mut conn, &state.config)
            .await?
            .public_url;
        drop(conn);
        let origin = if located.claimed {
            let scheme = match &public_url {
                Some(url) if url.starts_with("http://") => "http",
                _ => "https",
            };
            let host = parts
                .headers
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            Some(format!("{scheme}://{host}"))
        } else {
            // The request's own Host could be forged, so links name the configured URL only.
            public_url
        };
        let Some(origin) = origin else {
            tracing::warn!(
                page = page.id,
                "sign-in links need the instance's public URL"
            );
            return Ok(());
        };
        let now = state.clock.now_ms();
        let (code, digest) = identity::new_session_token();
        let mut tx = state.db.writer.begin().await?;
        sqlx::query(
            "INSERT INTO page_email_codes (id, page_id, email, expires_at, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(digest)
        .bind(page.id)
        .bind(&email)
        .bind(now + PAGE_EMAIL_LINK_MS)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM page_email_codes WHERE expires_at <= ?")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let locale = body
            .locale
            .filter(|locale| page.locales().contains(locale))
            .unwrap_or_else(|| page.default_locale());
        let link = format!("{origin}{}/access/email?code={code}", located.base);
        let (subject, text) = text::sign_in_mail(locale, &page.title, &link);
        mail::send(&state, email, subject, text);
        Ok::<_, ApiError>(())
    };
    match answer.await {
        Ok(()) => (StatusCode::ACCEPTED, axum::Json(serde_json::json!({}))).into_response(),
        Err(error) => error.into_response(),
    }
}

/// `POST <base>/access/email/redeem`: trades the code of a sign-in link, once, for a page
/// cookie. The link itself opens the page app, which posts the code, so mail scanners that
/// follow links do not use it up.
async fn redeem(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let address = client_address(&parts);
    let answer = async {
        check_post(&state, &located, &parts.headers)?;
        let body: PageRedeemBody = json_body(&body)?;
        state.page_attempts.check(&address)?;
        let page = &located.page;
        let now = state.clock.now_ms();
        let mut tx = state.db.writer.begin().await?;
        let email: Option<String> = sqlx::query_scalar(
            "DELETE FROM page_email_codes WHERE id = ? AND page_id = ? AND expires_at > ?
             RETURNING email",
        )
        .bind(identity::token_digest(&body.code))
        .bind(page.id)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(email) = email.filter(|email| page.allows_email(email)) else {
            tx.commit().await?;
            return Err(ApiError::new(ErrorCode::InvalidCredentials));
        };
        let (value, until) =
            access::start(&mut tx, page, Kind::Email, Some(&email), None, None, now).await?;
        tx.commit().await?;
        Ok::<_, ApiError>(access::cookie(
            &value,
            &located.base,
            until,
            now,
            state.config.secure_cookies(),
        ))
    };
    match answer.await {
        Ok(cookie) => signed_in(cookie),
        Err(error) => error.into_response(),
    }
}

/// `<base>/events?cursor=page:<id>@<seq>`: the page topic's events as [`LiveFrame`]s, each
/// stripped to its sequence and kind. Access is checked as for the snapshot, again before
/// every frame and every 5 seconds; a visitor who lost it gets `resync` with reason
/// `unavailable` and the stream ends.
async fn stream(State(state): State<AppState>, located: Located, parts: Parts) -> Response {
    let headers = parts.headers.clone();
    let address = client_address(&parts);
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let own = if located.claimed {
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            origin == format!("https://{host}") || origin == format!("http://{host}")
        } else {
            same_origin(&state.config, &headers, origin)
        };
        if !own {
            return ApiError::new(ErrorCode::OriginRejected).into_response();
        }
    }
    let query = params(&parts);
    let page = located.page;
    let topic = Topic::page(page.page());
    let cursor = match &query.cursor {
        None => None,
        Some(cursor) => match cursor
            .rsplit_once('@')
            .filter(|(on, _)| *on == topic.as_str())
            .and_then(|(_, seq)| seq.parse::<i64>().ok())
            .filter(|seq| *seq >= 0)
        {
            Some(seq) => Some(seq),
            None => {
                return ApiError::with_message(
                    ErrorCode::MalformedRequest,
                    "expected cursor=page:<id>@<seq>",
                )
                .into_response();
            }
        },
    };
    let token = query.token;
    let live = PageLive {
        state,
        page: page.page(),
        headers,
        address,
        token,
        topic,
        sent: 0,
        wake: None,
        closing: None,
        next_check: Instant::now() + RECHECK,
        queue: VecDeque::new(),
        done: false,
    };
    match live.open(cursor).await {
        Ok(live) => {
            let stream = futures_util::stream::unfold(live, |mut live| async move {
                let frame = live.next().await?;
                let event = Event::default()
                    .json_data(&frame)
                    .expect("frames serialize");
                Some((Ok::<_, Infallible>(event), live))
            });
            Sse::new(stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        Err(error) => error.into_response(),
    }
}

enum Wake {
    Read,
    Refused,
    Stopping,
}

struct PageLive {
    state: AppState,
    page: PageId,
    headers: HeaderMap,
    address: String,
    token: Option<String>,
    topic: Topic,
    sent: i64,
    wake: Option<broadcast::Receiver<Arc<str>>>,
    closing: Option<watch::Receiver<bool>>,
    next_check: Instant,
    queue: VecDeque<LiveFrame>,
    done: bool,
}

impl PageLive {
    /// Authorizes the visitor and queues `subscribed`, or `expired` for a cursor past the head.
    async fn open(mut self, cursor: Option<i64>) -> Result<Self, ApiError> {
        if !self.authorized().await {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        }
        let head = {
            let mut conn = self.state.db.reader.acquire().await?;
            events::head(&mut conn, &self.topic).await?
        };
        let cursor = cursor.unwrap_or(head);
        self.sent = cursor;
        self.queue.push_back(LiveFrame::Subscribed {
            head: self.topic.cursor(head),
            cursor: self.topic.cursor(cursor),
        });
        if cursor > head {
            self.queue.push_back(LiveFrame::Expired {
                head: self.topic.cursor(head),
            });
            self.done = true;
        }
        let (wake, closing) = self.state.hub.subscribe();
        self.wake = Some(wake);
        self.closing = Some(closing);
        Ok(self)
    }

    async fn next(&mut self) -> Option<LiveFrame> {
        if let Some(frame) = self.queue.pop_front() {
            return Some(frame);
        }
        if self.done {
            return None;
        }
        loop {
            let page = match self.read().await {
                Ok(page) => page,
                Err(error) => {
                    tracing::warn!(%error, topic = self.topic.as_str(), "page stream read failed");
                    return Some(self.resync());
                }
            };
            if let Some(last) = page.last().map(|event| event.seq) {
                if !self.authorized().await {
                    return Some(self.resync());
                }
                self.sent = last;
                return Some(LiveFrame::Events {
                    cursor: self.topic.cursor(last),
                    events: page,
                });
            }
            match self.wait().await {
                Wake::Read => {}
                Wake::Refused => return Some(self.resync()),
                Wake::Stopping => return None,
            }
        }
    }

    /// The next events, with only their sequence and kind: a page's audience learns that
    /// something changed and reads the snapshot again.
    async fn read(&self) -> Result<Vec<LiveEvent>, sqlx::Error> {
        let _permit = self
            .state
            .hub
            .replay
            .acquire()
            .await
            .expect("the replay semaphore is never closed");
        let mut conn = self.state.db.reader.acquire().await?;
        let events = events::after(&mut conn, &self.topic, self.sent, PAGE).await?;
        Ok(events
            .into_iter()
            .map(|event| LiveEvent {
                seq: event.seq,
                kind: event.kind,
                monitor: None,
                probe: None,
                channel: None,
                delivery: None,
                incident: None,
                maintenance: None,
                component: None,
                page: None,
            })
            .collect())
    }

    async fn wait(&mut self) -> Wake {
        let (Some(wake), Some(closing)) = (self.wake.as_mut(), self.closing.as_mut()) else {
            return Wake::Stopping;
        };
        loop {
            let recheck = tokio::select! {
                woke = wake.recv() => match woke {
                    Ok(topic) if *topic == *self.topic.as_str() => break,
                    Ok(_) => false,
                    Err(RecvError::Lagged(_)) => return Wake::Read,
                    Err(RecvError::Closed) => return Wake::Stopping,
                },
                _ = tokio::time::sleep_until(self.next_check) => true,
                _ = closing.wait_for(|closing| *closing) => return Wake::Stopping,
            };
            if recheck {
                self.next_check = Instant::now() + RECHECK;
                if !authorized(
                    &self.state,
                    self.page,
                    &self.headers,
                    &self.address,
                    self.token.as_deref(),
                )
                .await
                {
                    return Wake::Refused;
                }
            }
        }
        tokio::time::sleep(COALESCE).await;
        Wake::Read
    }

    async fn authorized(&mut self) -> bool {
        self.next_check = Instant::now() + RECHECK;
        authorized(
            &self.state,
            self.page,
            &self.headers,
            &self.address,
            self.token.as_deref(),
        )
        .await
    }

    fn resync(&mut self) -> LiveFrame {
        self.done = true;
        LiveFrame::Resync {
            cursor: self.topic.cursor(self.sent),
            reason: ResyncReason::Unavailable,
        }
    }
}

/// Whether the visitor still reads the page; a failed check counts as a refusal.
async fn authorized(
    state: &AppState,
    page: PageId,
    headers: &HeaderMap,
    address: &str,
    token: Option<&str>,
) -> bool {
    let check = async {
        let mut conn = state.db.reader.acquire().await?;
        let row = super::load(&mut conn, page).await?;
        drop(conn);
        let visitor = Visitor {
            headers,
            address,
            token,
        };
        access::audience(state, &row, &visitor).await
    };
    matches!(check.await, Ok(Some(_)))
}

/// `/s/` routes on the instance's own host.
pub(crate) fn instance_routes() -> Router<AppState> {
    Router::new().nest(&format!("{PAGE_PATH}{{slug}}"), routes())
}

#[cfg(test)]
mod tests {
    use super::{inject, script_json};
    use v1per_protocol::status_page::Locale;

    #[test]
    fn injected_json_cannot_close_its_script() {
        let boot = r#"{"title":"</script><script>alert(1)</script><!--"}"#;
        let html = inject(
            "<html lang=\"en\"><head><title>Status</title></head><body></body></html>",
            boot,
            "A & <B>",
            Locale::De,
        );
        assert!(html.starts_with("<html lang=\"de\">"));
        assert!(html.contains("<title>A &amp; &lt;B&gt;</title>"));
        assert_eq!(html.matches("</script>").count(), 1);
        assert!(!html.contains("<!--"));
        let json = script_json(boot);
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["title"], "</script><script>alert(1)</script><!--");
    }
}
