//! Page subscriptions (`docs/contracts/status-page-api.md`, Subscriptions): visitors follow a
//! page by email with double opt-in, page managers add webhooks, and both can follow only some
//! components. Every route answers `404` while the page's `subscriptions` setting is off.
//!
//! A subscriber's links carry one secret, derived from the instance key, the page, the address
//! and when the subscription started, and stored only as its SHA-256. The same secret confirms
//! the subscription, opens its settings and unsubscribes. Opening a link never changes
//! anything: the page app posts the secret once the visitor continues.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::{PageComponentId, PageId, PageSubscriberId, PageWebhookId};
use v1per_protocol::limits::{
    PAGE_SUBSCRIBERS_MAX, PAGE_SUBSCRIPTION_CONFIRM_MS, PAGE_WEBHOOKS_MAX,
};
use v1per_protocol::operations::pages::{
    PageArgs, PageSubscriber, PageSubscriberArgs, PageSubscriberRemoved, PageSubscribers,
    PageWebhook, PageWebhookArgs, PageWebhookRemoved, PagesAddWebhookArgs,
};
use v1per_protocol::registry::Snapshot;
use v1per_protocol::status_page::{
    Locale, PageAudience, PageNamedComponent, PageSubscribeBody, PageSubscription,
    PageSubscriptionComponents, PageSubscriptionToken, PageSubscriptions,
};

use super::public::{Located, check_post, json_body, link_origin, params, visitor};
use super::{PageRow, access, locate, mail, snapshot, subscription_text};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::http::client_address;
use crate::identity::{token_digest, validate_email};
use crate::incidents::{record_tree, record_tree_ctx};
use crate::state::AppState;

/// The `pages:<id>` event of a change to who follows a page.
const CHANGED: &str = "page.subscribers_changed";

/// The subscription routes, relative to the page's base.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/subscribe", post(subscribe))
        .route("/subscription", axum::routing::get(view).post(read))
        .route("/subscription/confirm", post(confirm))
        .route("/subscription/components", post(choose))
        .route("/unsubscribe", post(unsubscribe))
}

/// What the snapshot tells visitors about following `page`; `None` with the setting off.
pub(super) async fn offer(
    conn: &mut SqliteConnection,
    page: &PageRow,
) -> Result<Option<PageSubscriptions>, ApiError> {
    if !page.subscriptions {
        return Ok(None);
    }
    let email: bool = sqlx::query_scalar(
        "SELECT smtp_host IS NOT NULL AND smtp_from IS NOT NULL FROM instance_settings WHERE id = 1",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(Some(PageSubscriptions {
        email,
        feeds: page.public,
        components: followable(conn, page).await?,
    }))
}

/// The page's components a subscription may follow: those not marked internal.
async fn followable(
    conn: &mut SqliteConnection,
    page: &PageRow,
) -> sqlx::Result<Vec<PageComponentId>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT pc.id FROM page_components pc JOIN page_groups g ON g.id = pc.group_id
         WHERE pc.page_id = ? AND pc.internal = 0 ORDER BY g.position, pc.position, pc.id",
    )
    .bind(page.id)
    .fetch_all(conn)
    .await?;
    Ok(ids.into_iter().map(PageComponentId::new).collect())
}

/// `components` once each, as stored; `invalid_value` on `components` for one the page does
/// not let anyone follow.
async fn check_components(
    conn: &mut SqliteConnection,
    page: &PageRow,
    components: &[PageComponentId],
) -> Result<String, ApiError> {
    let allowed = followable(conn, page).await?;
    let mut chosen: Vec<PageComponentId> = Vec::new();
    for component in components {
        if !allowed.contains(component) {
            return Err(FieldErrors::one(
                "components",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        if !chosen.contains(component) {
            chosen.push(*component);
        }
    }
    serde_json::to_string(&chosen).map_err(ApiError::internal)
}

fn stored_components(text: &str) -> Vec<PageComponentId> {
    serde_json::from_str(text).unwrap_or_default()
}

/// The secret in a subscriber's links. It changes when the subscription starts again.
pub(super) fn link_secret(state: &AppState, page: i64, email: &str, created_at: i64) -> String {
    URL_SAFE_NO_PAD.encode(
        state
            .secrets
            .derive(&format!("page-subscriber:{page}:{email}:{created_at}")),
    )
}

/// `404` unless the page has subscriptions on.
fn enabled(located: &Located) -> Result<(), ApiError> {
    if located.page.subscriptions {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

/// `<base>/subscription?token=`: the page app, which shows the subscription. It changes
/// nothing; the app posts the token once the visitor continues.
async fn view(state: State<AppState>, located: Located, request: Request) -> Response {
    if let Err(error) = enabled(&located) {
        return error.into_response();
    }
    super::public::html(state, located, request).await
}

/// `POST <base>/subscribe`: mails a confirmation link. Answers `202` whether or not one was
/// sent, so nobody learns who follows the page.
async fn subscribe(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let address = client_address(&parts);
    let answer = async {
        enabled(&located)?;
        check_post(&state, &located, &parts.headers)?;
        let body: PageSubscribeBody = json_body(&body)?;
        state.page_attempts.check(&address)?;
        let email = validate_email("email", &body.email)?;
        let page = &located.page;
        let query = params(&parts);
        access::audience(
            &state,
            page,
            &visitor(&parts, &address, query.token.as_deref()),
        )
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
        let components = {
            let mut conn = state.db.reader.acquire().await?;
            check_components(&mut conn, page, &body.components).await?
        };
        state.page_mails.check(&email)?;
        if !page.public && !page.allows_email(&email) {
            return Ok(());
        }
        let Some(origin) = link_origin(&state, &located, &parts.headers).await? else {
            tracing::warn!(
                page = page.id,
                "subscription links need the instance's public URL"
            );
            return Ok(());
        };
        let locale = body
            .locale
            .filter(|locale| page.locales().contains(locale))
            .unwrap_or_else(|| page.default_locale());
        let now = state.clock.now_ms();
        let secret = link_secret(&state, page.id, &email, now);
        let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<(i64, Option<i64>)> = sqlx::query_as(
            "SELECT id, confirmed_at FROM page_subscribers WHERE page_id = ? AND email = ?",
        )
        .bind(page.id)
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
        match existing {
            // Already following: their mails carry the links they need.
            Some((_, Some(_))) => return Ok(()),
            Some((id, None)) => {
                sqlx::query(
                    "UPDATE page_subscribers
                     SET locale = ?, components = ?, token_hash = ?, created_at = ? WHERE id = ?",
                )
                .bind(locale.as_str())
                .bind(&components)
                .bind(token_digest(&secret))
                .bind(now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            }
            None => {
                let count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM page_subscribers
                     WHERE page_id = ? AND (confirmed_at IS NOT NULL OR created_at > ?)",
                )
                .bind(page.id)
                .bind(now - PAGE_SUBSCRIPTION_CONFIRM_MS)
                .fetch_one(&mut *tx)
                .await?;
                if count as usize >= PAGE_SUBSCRIBERS_MAX {
                    tracing::warn!(page = page.id, "the page has as many subscribers as it may");
                    return Ok(());
                }
                sqlx::query(
                    "INSERT INTO page_subscribers (page_id, email, locale, components, token_hash,
                                                   created_at)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(page.id)
                .bind(&email)
                .bind(locale.as_str())
                .bind(&components)
                .bind(token_digest(&secret))
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        let link = format!("{origin}{}/subscription?token={secret}", located.base);
        let (subject, text) = subscription_text::confirm_mail(locale, &page.title, &link);
        mail::send(&state, email, subject, text);
        Ok::<_, ApiError>(())
    };
    match answer.await {
        Ok(()) => (StatusCode::ACCEPTED, axum::Json(json!({}))).into_response(),
        Err(error) => error.into_response(),
    }
}

#[derive(sqlx::FromRow)]
struct Subscriber {
    id: i64,
    email: String,
    components: String,
    confirmed_at: Option<i64>,
}

/// The subscriber `token` opens on the page, unless it waited too long for its confirmation.
async fn find(
    conn: &mut SqliteConnection,
    page: &PageRow,
    token: &str,
    now: i64,
) -> Result<Subscriber, ApiError> {
    sqlx::query_as(
        "SELECT id, email, components, confirmed_at FROM page_subscribers
         WHERE token_hash = ? AND page_id = ? AND (confirmed_at IS NOT NULL OR created_at > ?)",
    )
    .bind(token_digest(token))
    .bind(page.id)
    .bind(now - PAGE_SUBSCRIPTION_CONFIRM_MS)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| ApiError::new(ErrorCode::InvalidCredentials))
}

/// The subscription as its links show it, with the names the public page gives components.
async fn answer(
    state: &AppState,
    page: &PageRow,
    subscriber: Subscriber,
) -> Result<PageSubscription, ApiError> {
    let built = snapshot::cached(state, page, PageAudience::Public, page.default_locale()).await?;
    Ok(PageSubscription {
        email: subscriber.email,
        confirmed: subscriber.confirmed_at.is_some(),
        components: stored_components(&subscriber.components),
        available: built
            .snapshot
            .groups
            .iter()
            .flat_map(|group| &group.components)
            .map(|component| PageNamedComponent {
                id: component.id,
                name: component.name.clone(),
            })
            .collect(),
    })
}

/// Checks a POST that carries a subscriber's token: the setting, the origin, the body and the
/// per-client limit.
fn token_post<T: serde::de::DeserializeOwned>(
    state: &AppState,
    located: &Located,
    parts: &Parts,
    body: &[u8],
) -> Result<T, ApiError> {
    enabled(located)?;
    check_post(state, located, &parts.headers)?;
    let body = json_body(body)?;
    state.page_attempts.check(&client_address(parts))?;
    Ok(body)
}

fn json_answer(answer: Result<PageSubscription, ApiError>) -> Response {
    match answer {
        Ok(subscription) => (
            [(header::CACHE_CONTROL, "no-store")],
            axum::Json(subscription),
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}

/// `POST <base>/subscription`: the subscription its token opens.
async fn read(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let answer = async {
        let body: PageSubscriptionToken = token_post(&state, &located, &parts, &body)?;
        let subscriber = {
            let mut conn = state.db.reader.acquire().await?;
            find(&mut conn, &located.page, &body.token, state.clock.now_ms()).await?
        };
        answer(&state, &located.page, subscriber).await
    };
    json_answer(answer.await)
}

/// Records [`CHANGED`] on the page node's `pages:<id>` topics outside a command and wakes them
/// after commit.
async fn changed_by_visitor(
    tx: &mut SqliteConnection,
    page: &PageRow,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    record_tree(
        tx,
        Topic::pages,
        page.node(),
        CHANGED,
        &json!({ "page": page.page() }),
        now,
    )
    .await
}

/// `POST <base>/subscription/confirm`: confirms the subscription its token opens, once the
/// visitor continued from the confirmation link. On a restricted page the address must still
/// be allowed.
async fn confirm(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let answer = async {
        let body: PageSubscriptionToken = token_post(&state, &located, &parts, &body)?;
        let page = &located.page;
        let now = state.clock.now_ms();
        let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let mut subscriber = find(&mut tx, page, &body.token, now).await?;
        if !page.public && !page.allows_email(&subscriber.email) {
            return Err(ApiError::new(ErrorCode::InvalidCredentials));
        }
        let mut topics = Vec::new();
        if subscriber.confirmed_at.is_none() {
            sqlx::query("UPDATE page_subscribers SET confirmed_at = ? WHERE id = ?")
                .bind(now)
                .bind(subscriber.id)
                .execute(&mut *tx)
                .await?;
            subscriber.confirmed_at = Some(now);
            topics = changed_by_visitor(&mut tx, page, now).await?;
        }
        tx.commit().await?;
        for topic in &topics {
            state.hub.publish(topic);
        }
        answer(&state, page, subscriber).await
    };
    json_answer(answer.await)
}

/// `POST <base>/subscription/components`: replaces what the subscription follows.
async fn choose(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let answer = async {
        let body: PageSubscriptionComponents = token_post(&state, &located, &parts, &body)?;
        let page = &located.page;
        let now = state.clock.now_ms();
        let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let mut subscriber = find(&mut tx, page, &body.token, now).await?;
        let components = check_components(&mut tx, page, &body.components).await?;
        sqlx::query("UPDATE page_subscribers SET components = ? WHERE id = ?")
            .bind(&components)
            .bind(subscriber.id)
            .execute(&mut *tx)
            .await?;
        subscriber.components = components;
        let topics = changed_by_visitor(&mut tx, page, now).await?;
        tx.commit().await?;
        for topic in &topics {
            state.hub.publish(topic);
        }
        answer(&state, page, subscriber).await
    };
    json_answer(answer.await)
}

/// `POST <base>/unsubscribe`: deletes the subscriber, with the token in a JSON body from the
/// page app or in `?token=` from a mail client's one-click unsubscribe (RFC 8058), which sends
/// a form and no `Origin`. A foreign `Origin` is refused. Answers `204` either way.
async fn unsubscribe(
    State(state): State<AppState>,
    located: Located,
    parts: Parts,
    body: Bytes,
) -> Response {
    let answer = async {
        enabled(&located)?;
        if parts.headers.contains_key(header::ORIGIN) {
            check_post(&state, &located, &parts.headers)?;
        }
        let token = match params(&parts).token {
            Some(token) => token,
            None => json_body::<PageSubscriptionToken>(&body)?.token,
        };
        let page = &located.page;
        let now = state.clock.now_ms();
        let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let deleted =
            sqlx::query("DELETE FROM page_subscribers WHERE token_hash = ? AND page_id = ?")
                .bind(token_digest(&token))
                .bind(page.id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        let topics = if deleted > 0 {
            changed_by_visitor(&mut tx, page, now).await?
        } else {
            Vec::new()
        };
        tx.commit().await?;
        for topic in &topics {
            state.hub.publish(topic);
        }
        Ok::<_, ApiError>(())
    };
    match answer.await {
        Ok(()) => (
            StatusCode::NO_CONTENT,
            [(header::CACHE_CONTROL, "no-store")],
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}

/// `pages.subscribers`.
pub(super) async fn list(
    ctx: &mut QueryCtx,
    args: PageArgs,
) -> Result<Snapshot<PageSubscribers>, ApiError> {
    let row = super::load(ctx.conn(), args.page).await?;
    ctx.node_scope(row.node(), Capability::PagesManage).await?;
    let subscribers: Vec<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT id, email, components, confirmed_at FROM page_subscribers
         WHERE page_id = ? AND confirmed_at IS NOT NULL ORDER BY confirmed_at DESC, id DESC",
    )
    .bind(row.id)
    .fetch_all(ctx.conn())
    .await?;
    let webhooks: Vec<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT id, host, components, created_at FROM page_webhooks WHERE page_id = ? ORDER BY id",
    )
    .bind(row.id)
    .fetch_all(ctx.conn())
    .await?;
    Ok(Snapshot {
        topic: Topic::pages(row.node()),
        data: PageSubscribers {
            subscribers: subscribers
                .into_iter()
                .map(|(id, email, components, confirmed_at)| PageSubscriber {
                    id: PageSubscriberId::new(id),
                    email,
                    components: stored_components(&components),
                    confirmed_at,
                })
                .collect(),
            webhooks: webhooks
                .into_iter()
                .map(|(id, host, components, created_at)| PageWebhook {
                    id: PageWebhookId::new(id),
                    host,
                    components: stored_components(&components),
                    created_at,
                })
                .collect(),
        },
    })
}

/// Records [`CHANGED`] in a command.
async fn changed(ctx: &mut CommandCtx, page: &PageRow) -> Result<(), ApiError> {
    record_tree_ctx(
        ctx,
        Topic::pages,
        page.node(),
        CHANGED,
        json!({ "page": page.page() }),
    )
    .await
}

/// `pages.removeSubscriber`.
pub(super) async fn remove_subscriber(
    ctx: &mut CommandCtx,
    args: PageSubscriberArgs,
) -> Result<PageSubscriberRemoved, ApiError> {
    let page: i64 = sqlx::query_scalar("SELECT page_id FROM page_subscribers WHERE id = ?")
        .bind(args.subscriber.get())
        .fetch_optional(ctx.conn())
        .await?
        .ok_or_else(ApiError::not_found)?;
    let row = locate(ctx, PageId::new(page), Capability::PagesManage, None).await?;
    sqlx::query("DELETE FROM page_subscribers WHERE id = ?")
        .bind(args.subscriber.get())
        .execute(ctx.conn())
        .await?;
    changed(ctx, &row).await?;
    Ok(PageSubscriberRemoved {
        subscriber: args.subscriber,
    })
}

/// The context a webhook's URL is sealed under.
pub(super) fn webhook_context(webhook: i64) -> String {
    format!("page_webhook:{webhook}:url")
}

/// `pages.addWebhook`. Any `http` or `https` URL, like webhook channels.
pub(super) async fn add_webhook(
    state: &AppState,
    ctx: &mut CommandCtx,
    args: PagesAddWebhookArgs,
) -> Result<PageWebhook, ApiError> {
    let row = locate(ctx, args.page, Capability::PagesManage, None).await?;
    let url = args.url.trim();
    let host = url::Url::parse(url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| url.host_str().map(str::to_owned))
        .ok_or_else(|| FieldErrors::one("url", FieldErrorCode::InvalidFormat, None))?;
    let components = check_components(ctx.conn(), &row, &args.components).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_webhooks WHERE page_id = ?")
        .bind(row.id)
        .fetch_one(ctx.conn())
        .await?;
    if count as usize >= PAGE_WEBHOOKS_MAX {
        return Err(FieldErrors::one(
            "url",
            FieldErrorCode::TooMany,
            Some(PAGE_WEBHOOKS_MAX as u32),
        ));
    }
    let now = ctx.now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO page_webhooks (page_id, url, host, components, created_at)
         VALUES (?, x'', ?, ?, ?) RETURNING id",
    )
    .bind(row.id)
    .bind(&host)
    .bind(&components)
    .bind(now)
    .fetch_one(ctx.conn())
    .await?;
    sqlx::query("UPDATE page_webhooks SET url = ? WHERE id = ?")
        .bind(state.secrets.seal(&webhook_context(id), url.as_bytes()))
        .bind(id)
        .execute(ctx.conn())
        .await?;
    changed(ctx, &row).await?;
    Ok(PageWebhook {
        id: PageWebhookId::new(id),
        host,
        components: stored_components(&components),
        created_at: now,
    })
}

/// `pages.removeWebhook`.
pub(super) async fn remove_webhook(
    ctx: &mut CommandCtx,
    args: PageWebhookArgs,
) -> Result<PageWebhookRemoved, ApiError> {
    let page: i64 = sqlx::query_scalar("SELECT page_id FROM page_webhooks WHERE id = ?")
        .bind(args.webhook.get())
        .fetch_optional(ctx.conn())
        .await?
        .ok_or_else(ApiError::not_found)?;
    let row = locate(ctx, PageId::new(page), Capability::PagesManage, None).await?;
    sqlx::query("DELETE FROM page_webhooks WHERE id = ?")
        .bind(args.webhook.get())
        .execute(ctx.conn())
        .await?;
    changed(ctx, &row).await?;
    Ok(PageWebhookRemoved {
        webhook: args.webhook,
    })
}

/// The locale a subscriber's mail is in: theirs while the page still offers it.
pub(super) fn mail_locale(page: &PageRow, stored: &str) -> Locale {
    Locale::parse(stored)
        .filter(|locale| page.locales().contains(locale))
        .unwrap_or_else(|| page.default_locale())
}
