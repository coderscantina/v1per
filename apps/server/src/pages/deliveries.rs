//! What a page's subscribers hear, and the queue that tells them.
//!
//! Whatever publishes an incident to a page, adds an update to a published one or resolves it
//! writes a notice in its own transaction ([`incident`], [`incident_everywhere`]), with the
//! incident as the public page shows it then in each locale, and one delivery per confirmed
//! subscriber and webhook that follows it. Maintenance has no such moment, since it shows 14
//! days ahead and starts on the clock, so [`run`] looks at every page with subscriptions on
//! once a minute, and announces each window once when it first shows and once when it starts.
//!
//! [`run`] sends the deliveries, per subscriber oldest first, and retries a failure like the
//! notification outbox: after 10 seconds, doubling up to 15 minutes, for 24 hours. A delivery
//! is dropped when the page turned subscriptions off, the incident is no longer published
//! there, or a restricted page no longer allows the address.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use tokio::task::JoinSet;
use v1per_notify::{self as notify, SendContext};
use v1per_protocol::ids::{IncidentId, PageComponentId, PageId};
use v1per_protocol::status_page::{
    Locale, PageNamedComponent, PageNotification, PageNotificationEvent, PageNotificationPage,
    PublicIncident, PublicMaintenance,
};

use super::subscription_text::{self, About, Links};
use super::{PageRow, enum_text, mail, select_page, snapshot, subscribers};
use crate::instance;
use crate::notifications::{GIVE_UP_AFTER_MS, retry_delay};
use crate::state::AppState;

/// Deliveries sent at once.
const CONCURRENCY: usize = 16;
/// The longest the worker sleeps, and how often it looks for maintenance to announce.
const IDLE_MS: i64 = 60_000;
/// Announced windows of recurring maintenance are forgotten this long after they started.
const ANNOUNCED_KEEP_MS: i64 = 30 * 86_400_000;

/// A notice's incident or maintenance as the public page showed it in one locale.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Content {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    incident: Option<PublicIncident>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maintenance: Option<PublicMaintenance>,
    components: Vec<PageNamedComponent>,
}

/// Writes a notice and one delivery per confirmed subscriber and webhook that follows one of
/// its components, or the whole page. A notice nobody follows is not kept.
async fn queue(
    conn: &mut SqliteConnection,
    page: &PageRow,
    event: PageNotificationEvent,
    incident: Option<IncidentId>,
    content: &HashMap<Locale, Content>,
    now: i64,
) -> sqlx::Result<()> {
    let components: Vec<PageComponentId> = content
        .values()
        .next()
        .map(|content| content.components.iter().map(|c| c.id).collect())
        .unwrap_or_default();
    let notice: i64 = sqlx::query_scalar(
        "INSERT INTO page_notices (page_id, event, incident_id, content, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(page.id)
    .bind(enum_text(event))
    .bind(incident.map(IncidentId::get))
    .bind(serde_json::to_string(content).expect("notices serialize"))
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    let components = serde_json::to_string(&components).expect("IDs serialize");
    let mut queued = 0;
    for (table, column, confirmed) in [
        (
            "page_subscribers",
            "subscriber_id",
            "confirmed_at IS NOT NULL",
        ),
        ("page_webhooks", "webhook_id", "1"),
    ] {
        // Table and column names are the constants above; values go in as binds.
        let sql = format!(
            "INSERT INTO page_deliveries (notice_id, {column}, next_attempt_at, created_at)
             SELECT ?1, s.id, ?2, ?2 FROM {table} s
             WHERE s.page_id = ?3 AND {confirmed}
               AND (s.components = '[]' OR EXISTS (
                    SELECT 1 FROM json_each(s.components) c
                    WHERE c.value IN (SELECT value FROM json_each(?4))))"
        );
        queued += sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(notice)
            .bind(now)
            .bind(page.id)
            .bind(&components)
            .execute(&mut *conn)
            .await?
            .rows_affected();
    }
    if queued == 0 {
        sqlx::query("DELETE FROM page_notices WHERE id = ?")
            .bind(notice)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Tells `page`'s subscribers about `incident`, as the public page shows it now. Nothing when
/// the page has subscriptions off or does not show the incident publicly. Call it after the
/// change is written, in its transaction.
pub(crate) async fn incident(
    conn: &mut SqliteConnection,
    page: PageId,
    incident: IncidentId,
    event: PageNotificationEvent,
    now: i64,
) -> sqlx::Result<()> {
    let Ok(row) = super::load(conn, page).await else {
        return Ok(());
    };
    if !row.subscriptions {
        return Ok(());
    }
    let mut content = HashMap::new();
    for locale in Locale::ALL {
        match snapshot::public_incident(conn, &row, incident, locale).await {
            Ok(Some((incident, components))) => {
                content.insert(
                    locale,
                    Content {
                        incident: Some(incident),
                        maintenance: None,
                        components,
                    },
                );
            }
            Ok(None) => return Ok(()),
            // A notice is never worth failing the change that caused it.
            Err(error) => {
                tracing::error!(?error, page = row.id, "a subscriber notice was not written");
                return Ok(());
            }
        }
    }
    queue(conn, &row, event, Some(incident), &content, now).await
}

/// [`incident`] for every page with subscriptions on that `incident` is published to.
pub(crate) async fn incident_everywhere(
    conn: &mut SqliteConnection,
    incident: IncidentId,
    event: PageNotificationEvent,
    now: i64,
) -> sqlx::Result<()> {
    let pages: Vec<i64> = sqlx::query_scalar(
        "SELECT p.page_id FROM incident_publications p JOIN pages ON pages.id = p.page_id
         WHERE p.incident_id = ? AND pages.subscriptions = 1",
    )
    .bind(incident.get())
    .fetch_all(&mut *conn)
    .await?;
    for page in pages {
        self::incident(conn, PageId::new(page), incident, event, now).await?;
    }
    Ok(())
}

/// Announces maintenance windows that show on a page for the first time or started since
/// the last look. A window first seen under way is announced once, as started.
async fn announce_maintenance(state: &AppState, now: i64) -> anyhow::Result<()> {
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let pages: Vec<PageRow> = sqlx::query_as(concat!(select_page!(), " WHERE subscriptions = 1"))
        .fetch_all(&mut *tx)
        .await?;
    for page in pages {
        for (maintenance, window, components) in snapshot::public_maintenance(&mut tx, &page, now)
            .await
            .map_err(|error| anyhow::anyhow!("{error:?}"))?
        {
            let starts_at = window.starts_at.unwrap_or(0);
            let mut events = vec![PageNotificationEvent::MaintenanceScheduled];
            if window.active {
                events.push(PageNotificationEvent::MaintenanceStarted);
            }
            let mut newest = None;
            for event in events {
                let added = sqlx::query(
                    "INSERT OR IGNORE INTO page_maintenance_notices
                         (page_id, maintenance_id, starts_at, event)
                     VALUES (?, ?, ?, ?)",
                )
                .bind(page.id)
                .bind(maintenance.get())
                .bind(starts_at)
                .bind(enum_text(event))
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if added > 0 {
                    newest = Some(event);
                }
            }
            if let Some(event) = newest {
                // Maintenance texts are the operator's own, the same in every locale.
                let content = Locale::ALL
                    .into_iter()
                    .map(|locale| {
                        let content = Content {
                            incident: None,
                            maintenance: Some(window.clone()),
                            components: components.clone(),
                        };
                        (locale, content)
                    })
                    .collect();
                queue(&mut tx, &page, event, None, &content, now).await?;
            }
        }
    }
    sqlx::query("DELETE FROM page_maintenance_notices WHERE starts_at > 0 AND starts_at < ?")
        .bind(now - ANNOUNCED_KEEP_MS)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Sends due deliveries and announces maintenance until `stop`, then waits for the sends in
/// flight. Page events wake it, since every notice is written with one.
pub async fn run(state: AppState, mut stop: watch::Receiver<bool>) {
    let ctx = SendContext::new();
    let (mut wake, _) = state.hub.subscribe();
    let mut running: JoinSet<i64> = JoinSet::new();
    let mut in_flight: HashSet<i64> = HashSet::new();
    let mut look_at = 0;
    while !*stop.borrow() {
        let now = state.clock.now_ms();
        if now >= look_at {
            if let Err(error) = announce_maintenance(&state, now).await {
                tracing::error!(%error, "announcing maintenance to page subscribers failed");
            }
            look_at = now + IDLE_MS;
        }
        let next = match claim(&state, now, &in_flight).await {
            Ok((due, next)) => {
                for id in due.into_iter().take(CONCURRENCY - running.len()) {
                    in_flight.insert(id);
                    let (state, ctx) = (state.clone(), ctx.clone());
                    running.spawn(async move {
                        if let Err(error) = deliver(&state, &ctx, id).await {
                            tracing::error!(%error, delivery = id, "a page delivery failed to run; retrying");
                        }
                        id
                    });
                }
                next
            }
            Err(error) => {
                tracing::error!(%error, "reading the page delivery queue failed; retrying");
                None
            }
        };
        let until = next.unwrap_or(look_at).min(look_at);
        tokio::select! {
            // A wake only reads the queue again; maintenance keeps its once-a-minute look, so
            // a flapping monitor does not take the writer for it.
            woken = wake.recv() => if let Err(RecvError::Closed) = woken {
                break;
            },
            Some(done) = running.join_next(), if !running.is_empty() => {
                if let Ok(id) = done {
                    in_flight.remove(&id);
                }
            }
            _ = state.clock.sleep_until(until) => {}
            _ = stop.changed() => {}
        }
    }
    while running.join_next().await.is_some() {}
}

/// Due deliveries not in flight, each the oldest of its subscriber or webhook, and when the
/// next one falls due.
async fn claim(
    state: &AppState,
    now: i64,
    in_flight: &HashSet<i64>,
) -> sqlx::Result<(Vec<i64>, Option<i64>)> {
    let mut conn = state.db.reader.acquire().await?;
    let due: Vec<i64> = sqlx::query_scalar(
        "SELECT d.id FROM page_deliveries d
         WHERE d.next_attempt_at <= ?1
           AND NOT EXISTS (SELECT 1 FROM page_deliveries p
                           WHERE p.id < d.id AND (p.subscriber_id = d.subscriber_id
                                                  OR p.webhook_id = d.webhook_id))
         ORDER BY d.next_attempt_at, d.id LIMIT ?2",
    )
    .bind(now)
    .bind((CONCURRENCY + in_flight.len()) as i64)
    .fetch_all(&mut *conn)
    .await?;
    let next: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(next_attempt_at) FROM page_deliveries WHERE next_attempt_at > ?",
    )
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    Ok((
        due.into_iter()
            .filter(|id| !in_flight.contains(id))
            .collect(),
        next,
    ))
}

#[derive(sqlx::FromRow)]
struct Due {
    notice_id: i64,
    subscriber_id: Option<i64>,
    webhook_id: Option<i64>,
    attempts: i64,
    created_at: i64,
    page_id: i64,
    event: String,
    incident_id: Option<i64>,
    content: String,
}

enum Outcome {
    Sent,
    Retry(String),
    /// Not sent, and never will be: the reason is logged.
    Dropped(String),
}

/// Where `page` is served for links in mail and webhooks: its first claimed hostname, else
/// the instance's public URL.
async fn page_url(
    state: &AppState,
    conn: &mut SqliteConnection,
    page: &PageRow,
) -> sqlx::Result<Option<String>> {
    let public_url = instance::effective(conn, &state.config).await?.public_url;
    let hostname: Option<String> = sqlx::query_scalar(
        "SELECT hostname FROM page_hostnames WHERE page_id = ? ORDER BY hostname LIMIT 1",
    )
    .bind(page.id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match hostname {
        Some(hostname) => {
            let scheme = match &public_url {
                Some(url) if url.starts_with("http://") => "http",
                _ => "https",
            };
            Some(format!("{scheme}://{hostname}"))
        }
        None => public_url.map(|url| format!("{url}{}", page.path())),
    })
}

/// Sends one delivery and records the outcome.
async fn deliver(state: &AppState, ctx: &SendContext, id: i64) -> anyhow::Result<()> {
    let mut conn = state.db.reader.acquire().await?;
    let due: Option<Due> = sqlx::query_as(
        "SELECT d.notice_id, d.subscriber_id, d.webhook_id, d.attempts, d.created_at, n.page_id,
                n.event, n.incident_id, n.content
         FROM page_deliveries d JOIN page_notices n ON n.id = d.notice_id WHERE d.id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(due) = due else {
        return Ok(());
    };
    let page: PageRow = sqlx::query_as(concat!(select_page!(), " WHERE id = ?"))
        .bind(due.page_id)
        .fetch_one(&mut *conn)
        .await?;
    let event: PageNotificationEvent =
        serde_json::from_value(serde_json::Value::String(due.event.clone()))?;
    let content: HashMap<Locale, Content> = serde_json::from_str(&due.content)?;
    let published = match due.incident_id {
        Some(incident) => sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM incident_publications WHERE incident_id = ? AND page_id = ?",
        )
        .bind(incident)
        .bind(page.id)
        .fetch_optional(&mut *conn)
        .await?
        .is_some(),
        None => true,
    };
    let url = page_url(state, &mut conn, &page).await?;
    let in_locale = |locale: Locale| {
        content
            .get(&locale)
            .or_else(|| content.values().next())
            .cloned()
    };
    let outcome = if !page.subscriptions {
        Outcome::Dropped("the page turned subscriptions off".into())
    } else if !published {
        Outcome::Dropped("the incident is no longer published".into())
    } else if let Some(subscriber) = due.subscriber_id {
        let row: (String, String, i64) =
            sqlx::query_as("SELECT email, locale, created_at FROM page_subscribers WHERE id = ?")
                .bind(subscriber)
                .fetch_one(&mut *conn)
                .await?;
        drop(conn);
        let (email, locale, created_at) = row;
        let locale = subscribers::mail_locale(&page, &locale);
        match (url, in_locale(locale)) {
            _ if !page.public && !page.allows_email(&email) => {
                Outcome::Dropped("the page no longer allows the address".into())
            }
            (None, _) => Outcome::Dropped("mail links need the instance's public URL".into()),
            (_, None) => Outcome::Dropped("the notice is empty".into()),
            (Some(url), Some(content)) => {
                let secret = subscribers::link_secret(state, page.id, &email, created_at);
                let manage = format!("{url}/subscription?token={secret}");
                let unsubscribe = format!("{manage}&action=unsubscribe");
                let one_click = format!("{url}/unsubscribe?token={secret}");
                let (subject, body) = subscription_text::notice_mail(
                    locale,
                    &page.title,
                    page.zone(),
                    About {
                        event,
                        incident: content.incident.as_ref(),
                        maintenance: content.maintenance.as_ref(),
                        components: &content.components,
                    },
                    Links {
                        page: &url,
                        manage: &manage,
                        unsubscribe: &unsubscribe,
                    },
                );
                match mail::deliver(state, &email, subject, body, Some(&one_click)).await {
                    Ok(()) => Outcome::Sent,
                    Err(error)
                        if error
                            .downcast_ref::<lettre::transport::smtp::Error>()
                            .is_some_and(lettre::transport::smtp::Error::is_permanent) =>
                    {
                        Outcome::Dropped(error.to_string())
                    }
                    Err(error) => Outcome::Retry(error.to_string()),
                }
            }
        }
    } else if let Some(webhook) = due.webhook_id {
        let sealed: Vec<u8> = sqlx::query_scalar("SELECT url FROM page_webhooks WHERE id = ?")
            .bind(webhook)
            .fetch_one(&mut *conn)
            .await?;
        drop(conn);
        let target = state
            .secrets
            .open(&subscribers::webhook_context(webhook), &sealed)
            .and_then(|url| String::from_utf8(url).ok());
        match (target, in_locale(page.default_locale())) {
            (None, _) => Outcome::Dropped("the webhook URL does not open with this key".into()),
            (_, None) => Outcome::Dropped("the notice is empty".into()),
            (Some(target), Some(content)) => {
                let body = PageNotification {
                    event,
                    page: PageNotificationPage {
                        slug: page.slug.clone(),
                        title: page.title.clone(),
                        url,
                    },
                    incident: content.incident,
                    maintenance: content.maintenance,
                    components: content.components,
                    sent_at: state.clock.now_ms(),
                };
                match notify::post_json(&target, &body, ctx).await {
                    Ok(_) => Outcome::Sent,
                    Err(error) if error.is_retryable() => Outcome::Retry(error.to_string()),
                    Err(error) => Outcome::Dropped(error.to_string()),
                }
            }
        }
    } else {
        Outcome::Dropped("the delivery has no recipient".into())
    };
    finish(state, id, &due, outcome).await
}

async fn finish(state: &AppState, id: i64, due: &Due, outcome: Outcome) -> anyhow::Result<()> {
    let now = state.clock.now_ms();
    let attempts = due.attempts + 1;
    let retry = match outcome {
        Outcome::Sent => None,
        Outcome::Retry(error) if now - due.created_at < GIVE_UP_AFTER_MS => {
            tracing::warn!(delivery = id, %error, "a page delivery failed; retrying");
            Some(now + retry_delay(attempts))
        }
        Outcome::Retry(error) | Outcome::Dropped(error) => {
            tracing::warn!(delivery = id, %error, "a page delivery was dropped");
            None
        }
    };
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    match retry {
        Some(next) => {
            sqlx::query(
                "UPDATE page_deliveries SET attempts = ?, next_attempt_at = ? WHERE id = ?",
            )
            .bind(attempts)
            .bind(next)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM page_deliveries WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "DELETE FROM page_notices WHERE id = ?1
                   AND NOT EXISTS (SELECT 1 FROM page_deliveries WHERE notice_id = ?1)",
            )
            .bind(due.notice_id)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
