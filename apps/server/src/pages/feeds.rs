//! Feeds of a page with subscriptions on: `feed.atom` and `feed.rss` with the incident history
//! of `incidents.json`, and `maintenance.ics` with the snapshot's maintenance, all for the
//! public audience. Like badges they open on public pages, and on restricted ones with
//! `?token=<share token>`; otherwise `404`.

use std::fmt::Write as _;

use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use chrono::{DateTime, Utc};
use v1per_protocol::status_page::{Locale, PageAudience, PageNamedComponent, PublicIncident};

use super::public::{Located, escape_html, page_origin, params, pick_locale};
use super::subscription_text::{self, affected, calendar_name, feed_title};
use super::{PageRow, access, snapshot};
use crate::error::ApiError;
use crate::state::AppState;

/// Incidents a feed lists, newest first.
const FEED_ITEMS: u32 = 50;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/feed.atom", get(atom))
        .route("/feed.rss", get(rss))
        .route("/maintenance.ics", get(ics))
}

/// The `<link rel="alternate">` tags for the HTML of a public page with subscriptions on.
pub(super) fn alternate_links(page: &PageRow, base: &str) -> Option<String> {
    (page.public && page.subscriptions).then(|| {
        let title = escape_html(&page.title);
        format!(
            "<link rel=\"alternate\" type=\"application/atom+xml\" title=\"{title}\" \
             href=\"{base}/feed.atom\"><link rel=\"alternate\" type=\"application/rss+xml\" \
             title=\"{title}\" href=\"{base}/feed.rss\">"
        )
    })
}

/// What a feed is built from, once the request may read it.
struct Feed {
    locale: Locale,
    /// Absolute URL of the page; relative when no origin is known.
    url: String,
    cache: &'static str,
}

/// `404` unless the page has subscriptions on and is public or opened by a share token.
async fn open(state: &AppState, located: &Located, parts: &Parts) -> Result<Feed, ApiError> {
    let page = &located.page;
    if !page.subscriptions {
        return Err(ApiError::not_found());
    }
    let query = params(parts);
    if !page.public {
        let mut conn = state.db.reader.acquire().await?;
        let opened = match &query.token {
            Some(token) => access::share_token(&mut conn, page, token, state.clock.now_ms())
                .await?
                .is_some(),
            None => false,
        };
        if !opened {
            return Err(ApiError::not_found());
        }
    }
    let origin = page_origin(state, located, &parts.headers)
        .await?
        .unwrap_or_default();
    Ok(Feed {
        locale: pick_locale(page, query.locale.as_deref(), &parts.headers),
        url: format!("{origin}{}", located.base),
        cache: if page.public {
            "public, max-age=60"
        } else {
            "private, max-age=60"
        },
    })
}

fn answer(feed: &Feed, content_type: &'static str, body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, feed.cache),
        ],
        body,
    )
        .into_response()
}

/// The newest published incidents and the names the public page gives components.
async fn incidents(
    state: &AppState,
    page: &PageRow,
    locale: Locale,
) -> Result<(Vec<PublicIncident>, Vec<PageNamedComponent>), ApiError> {
    let history =
        snapshot::incident_history(state, page, PageAudience::Public, locale, None, FEED_ITEMS)
            .await?;
    let built = snapshot::cached(state, page, PageAudience::Public, locale).await?;
    let names = built
        .snapshot
        .groups
        .iter()
        .flat_map(|group| &group.components)
        .map(|component| PageNamedComponent {
            id: component.id,
            name: component.name.clone(),
        })
        .collect();
    Ok((history.incidents, names))
}

/// An incident's updates, newest first, as plain text.
fn summary(
    incident: &PublicIncident,
    locale: Locale,
    page: &PageRow,
    names: &[PageNamedComponent],
) -> String {
    let components: Vec<PageNamedComponent> = incident
        .components
        .iter()
        .filter_map(|id| names.iter().find(|named| named.id == *id).cloned())
        .collect();
    let mut text = affected(locale, &components).unwrap_or_default();
    for update in &incident.updates {
        let _ = write!(
            text,
            "\n\n{} ({}): {}",
            subscription_text::state(locale, update.state),
            subscription_text::time(update.created_at, page.zone()),
            update.message.trim()
        );
    }
    text.trim().to_owned()
}

fn updated(incident: &PublicIncident) -> i64 {
    incident
        .updates
        .first()
        .map_or(incident.started_at, |update| update.created_at)
}

fn utc(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

async fn atom(State(state): State<AppState>, located: Located, parts: Parts) -> Response {
    let built = async {
        let feed = open(&state, &located, &parts).await?;
        let page = &located.page;
        let (incidents, names) = incidents(&state, page, feed.locale).await?;
        let newest = incidents
            .iter()
            .map(updated)
            .max()
            .unwrap_or(page.updated_at);
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <feed xmlns=\"http://www.w3.org/2005/Atom\" xml:lang=\"{}\">\n\
             <title>{}</title>\n<id>{}/incidents</id>\n\
             <link rel=\"alternate\" type=\"text/html\" href=\"{}/incidents\"/>\n\
             <link rel=\"self\" href=\"{}/feed.atom\"/>\n<updated>{}</updated>\n",
            feed.locale.as_str(),
            escape_html(&feed_title(feed.locale, &page.title)),
            escape_html(&feed.url),
            escape_html(&feed.url),
            escape_html(&feed.url),
            utc(newest).to_rfc3339(),
        );
        for incident in &incidents {
            let _ = write!(
                xml,
                "<entry>\n<title>{}</title>\n<id>{}/incidents#{}</id>\n\
                 <link rel=\"alternate\" type=\"text/html\" href=\"{}/incidents\"/>\n\
                 <published>{}</published>\n<updated>{}</updated>\n\
                 <author><name>{}</name></author>\n<content type=\"text\">{}</content>\n</entry>\n",
                escape_html(&incident.title),
                escape_html(&feed.url),
                incident.id,
                escape_html(&feed.url),
                utc(incident.started_at).to_rfc3339(),
                utc(updated(incident)).to_rfc3339(),
                escape_html(&page.title),
                escape_html(&summary(incident, feed.locale, page, &names)),
            );
        }
        xml.push_str("</feed>\n");
        Ok::<_, ApiError>(answer(&feed, "application/atom+xml; charset=utf-8", xml))
    };
    built.await.unwrap_or_else(IntoResponse::into_response)
}

async fn rss(State(state): State<AppState>, located: Located, parts: Parts) -> Response {
    let built = async {
        let feed = open(&state, &located, &parts).await?;
        let page = &located.page;
        let (incidents, names) = incidents(&state, page, feed.locale).await?;
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<rss version=\"2.0\">\n<channel>\n\
             <title>{}</title>\n<link>{}/incidents</link>\n<description>{}</description>\n\
             <language>{}</language>\n",
            escape_html(&feed_title(feed.locale, &page.title)),
            escape_html(&feed.url),
            escape_html(&feed_title(feed.locale, &page.title)),
            feed.locale.as_str(),
        );
        for incident in &incidents {
            let _ = write!(
                xml,
                "<item>\n<title>{}</title>\n<link>{}/incidents</link>\n\
                 <guid isPermaLink=\"false\">{}/incidents#{}</guid>\n<pubDate>{}</pubDate>\n\
                 <description>{}</description>\n</item>\n",
                escape_html(&incident.title),
                escape_html(&feed.url),
                escape_html(&feed.url),
                incident.id,
                utc(updated(incident)).to_rfc2822(),
                escape_html(&summary(incident, feed.locale, page, &names)),
            );
        }
        xml.push_str("</channel>\n</rss>\n");
        Ok::<_, ApiError>(answer(&feed, "application/rss+xml; charset=utf-8", xml))
    };
    built.await.unwrap_or_else(IntoResponse::into_response)
}

/// Text for an iCalendar value: backslashes, semicolons, commas and newlines escaped.
fn ics_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// Folds a content line at 75 octets, as RFC 5545 asks, without splitting a character.
fn fold(line: &str) -> String {
    let mut folded = String::with_capacity(line.len() + line.len() / 74 * 3);
    let mut width = 0;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if width + len > 75 {
            folded.push_str("\r\n ");
            width = 1;
        }
        folded.push(ch);
        width += len;
    }
    folded.push_str("\r\n");
    folded
}

fn ics_time(ms: i64) -> String {
    utc(ms).format("%Y%m%dT%H%M%SZ").to_string()
}

async fn ics(State(state): State<AppState>, located: Located, parts: Parts) -> Response {
    let built = async {
        let feed = open(&state, &located, &parts).await?;
        let page = &located.page;
        let built = snapshot::cached(&state, page, PageAudience::Public, feed.locale).await?;
        let names: Vec<_> = built
            .snapshot
            .groups
            .iter()
            .flat_map(|group| &group.components)
            .collect();
        let now = ics_time(state.clock.now_ms());
        let mut lines = vec![
            "BEGIN:VCALENDAR".to_owned(),
            "VERSION:2.0".to_owned(),
            "PRODID:-//V1Per//Status page//EN".to_owned(),
            "CALSCALE:GREGORIAN".to_owned(),
            format!(
                "X-WR-CALNAME:{}",
                ics_text(&calendar_name(feed.locale, &page.title))
            ),
        ];
        for window in &built.snapshot.maintenance {
            let Some(start) = window.starts_at else {
                continue;
            };
            let uid =
                blake3::hash(format!("{}:{}:{start}", page.slug, window.title).as_bytes()).to_hex();
            let covered: Vec<&str> = window
                .components
                .iter()
                .filter_map(|id| names.iter().find(|shown| shown.id == *id))
                .map(|shown| shown.name.as_str())
                .collect();
            let description = [window.description.trim(), &covered.join(", ")]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            lines.push("BEGIN:VEVENT".to_owned());
            lines.push(format!("UID:{}@v1per", &uid[..32]));
            lines.push(format!("DTSTAMP:{now}"));
            lines.push(format!("DTSTART:{}", ics_time(start)));
            if let Some(end) = window.ends_at {
                lines.push(format!("DTEND:{}", ics_time(end)));
            }
            lines.push(format!("SUMMARY:{}", ics_text(&window.title)));
            if !description.is_empty() {
                lines.push(format!("DESCRIPTION:{}", ics_text(&description)));
            }
            lines.push(format!("URL:{}/maintenance", feed.url));
            lines.push("END:VEVENT".to_owned());
        }
        lines.push("END:VCALENDAR".to_owned());
        let body: String = lines.iter().map(|line| fold(line)).collect();
        Ok::<_, ApiError>(answer(&feed, "text/calendar; charset=utf-8", body))
    };
    built.await.unwrap_or_else(IntoResponse::into_response)
}

#[cfg(test)]
mod tests {
    use super::{fold, ics_text};

    #[test]
    fn calendar_lines_fold_at_75_octets_and_escape_text() {
        let line = format!("SUMMARY:{}", "ä".repeat(50));
        let folded = fold(&line);
        assert!(folded.split("\r\n").all(|part| part.len() <= 75));
        assert_eq!(folded.replace("\r\n ", "").trim_end(), line);
        assert_eq!(ics_text("a,b;c\\d\ne"), "a\\,b\\;c\\\\d\\ne");
    }
}
