//! What subscriber mails and feeds say, in each page locale. Incident titles, updates and
//! maintenance texts come from the public snapshot; this adds the frame around them.

use chrono::DateTime;
use chrono_tz::Tz;
use v1per_protocol::operations::incidents::IncidentState;
use v1per_protocol::status_page::{
    Locale, PageNamedComponent, PageNotificationEvent, PublicIncident, PublicMaintenance,
};

/// The words the status page app uses for a state.
pub(super) fn state(locale: Locale, state: IncidentState) -> &'static str {
    match (locale, state) {
        (Locale::En, IncidentState::Investigating) => "Investigating",
        (Locale::En, IncidentState::Identified) => "Identified",
        (Locale::En, IncidentState::Monitoring) => "Monitoring",
        (Locale::En, IncidentState::Resolved) => "Resolved",
        (Locale::De, IncidentState::Investigating) => "Wird untersucht",
        (Locale::De, IncidentState::Identified) => "Ursache gefunden",
        (Locale::De, IncidentState::Monitoring) => "Wird beobachtet",
        (Locale::De, IncidentState::Resolved) => "Behoben",
    }
}

/// `2026-11-12 22:00` in `zone`.
pub(super) fn time(ms: i64, zone: Tz) -> String {
    DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .with_timezone(&zone)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

pub(super) fn affected(locale: Locale, components: &[PageNamedComponent]) -> Option<String> {
    if components.is_empty() {
        return None;
    }
    let names: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();
    let label = match locale {
        Locale::En => "Affected",
        Locale::De => "Betroffen",
    };
    Some(format!("{label}: {}", names.join(", ")))
}

/// Subject and plain-text body of the mail that confirms a subscription.
pub(super) fn confirm_mail(locale: Locale, title: &str, link: &str) -> (String, String) {
    match locale {
        Locale::En => (
            format!("Confirm your subscription to {title}"),
            format!(
                "Open this link to get an email when {title} reports an incident or \
                 maintenance:\n\n{link}\n\nThe link works for 24 hours. If you did not ask \
                 for it, ignore this email and nothing happens.\n"
            ),
        ),
        Locale::De => (
            format!("Bestätigen Sie Ihr Abonnement von {title}"),
            format!(
                "Öffnen Sie diesen Link, um eine E-Mail zu erhalten, wenn {title} eine \
                 Störung oder Wartung meldet:\n\n{link}\n\nDer Link funktioniert 24 Stunden \
                 lang. Wenn Sie ihn nicht angefordert haben, ignorieren Sie diese E-Mail, \
                 dann geschieht nichts.\n"
            ),
        ),
    }
}

/// Links of a notification mail.
pub(super) struct Links<'a> {
    pub page: &'a str,
    pub manage: &'a str,
    pub unsubscribe: &'a str,
}

/// What a notification is about, as the public page showed it.
pub(super) struct About<'a> {
    pub event: PageNotificationEvent,
    pub incident: Option<&'a PublicIncident>,
    pub maintenance: Option<&'a PublicMaintenance>,
    pub components: &'a [PageNamedComponent],
}

/// Subject and plain-text body of a notification about an incident or a maintenance, with
/// maintenance times in the page's `zone`.
pub(super) fn notice_mail(
    locale: Locale,
    title: &str,
    zone: Tz,
    about: About<'_>,
    links: Links<'_>,
) -> (String, String) {
    let About {
        event,
        incident,
        maintenance,
        components,
    } = about;
    let name = incident
        .map(|incident| incident.title.as_str())
        .or(maintenance.map(|window| window.title.as_str()))
        .unwrap_or_default();
    let what = match (locale, event) {
        (Locale::En, PageNotificationEvent::IncidentPublished) => name.to_owned(),
        (Locale::En, PageNotificationEvent::IncidentUpdated) => format!("Update on {name}"),
        (Locale::En, PageNotificationEvent::IncidentResolved) => format!("Resolved: {name}"),
        (Locale::En, PageNotificationEvent::MaintenanceScheduled) => {
            format!("Maintenance scheduled: {name}")
        }
        (Locale::En, PageNotificationEvent::MaintenanceStarted) => {
            format!("Maintenance started: {name}")
        }
        (Locale::De, PageNotificationEvent::IncidentPublished) => name.to_owned(),
        (Locale::De, PageNotificationEvent::IncidentUpdated) => format!("Neues zu {name}"),
        (Locale::De, PageNotificationEvent::IncidentResolved) => format!("Behoben: {name}"),
        (Locale::De, PageNotificationEvent::MaintenanceScheduled) => {
            format!("Wartung geplant: {name}")
        }
        (Locale::De, PageNotificationEvent::MaintenanceStarted) => {
            format!("Wartung begonnen: {name}")
        }
    };
    let mut lines = vec![name.to_owned()];
    if let Some(incident) = incident {
        lines.push(format!("Status: {}", state(locale, incident.state)));
    }
    if let Some(window) = maintenance
        && let Some(start) = window.starts_at
    {
        let start = time(start, zone);
        lines.push(match (locale, window.ends_at.map(|end| time(end, zone))) {
            (Locale::En, Some(end)) => format!("From {start} to {end} ({zone})"),
            (Locale::En, None) => format!("From {start} ({zone})"),
            (Locale::De, Some(end)) => format!("Von {start} bis {end} ({zone})"),
            (Locale::De, None) => format!("Ab {start} ({zone})"),
        });
    }
    lines.extend(affected(locale, components));
    let message = incident
        .and_then(|incident| incident.updates.first())
        .map(|update| update.message.as_str())
        .or(maintenance.map(|window| window.description.as_str()))
        .filter(|text| !text.trim().is_empty());
    let mut body = lines.join("\n");
    if let Some(message) = message {
        body.push_str("\n\n");
        body.push_str(message.trim());
    }
    let Links {
        page,
        manage,
        unsubscribe,
    } = links;
    body.push_str(&match locale {
        Locale::En => format!(
            "\n\nStatus page: {page}\n\n--\nYou get this email because you follow {title}.\n\
             Choose what you follow: {manage}\nUnsubscribe: {unsubscribe}\n"
        ),
        Locale::De => format!(
            "\n\nStatusseite: {page}\n\n--\nSie erhalten diese E-Mail, weil Sie {title} \
             folgen.\nAuswahl ändern: {manage}\nAbbestellen: {unsubscribe}\n"
        ),
    });
    (format!("{title}: {what}"), body)
}

/// The title of a feed.
pub(super) fn feed_title(locale: Locale, title: &str) -> String {
    match locale {
        Locale::En => format!("{title} incidents"),
        Locale::De => format!("{title} Störungen"),
    }
}

/// The name of the maintenance calendar.
pub(super) fn calendar_name(locale: Locale, title: &str) -> String {
    match locale {
        Locale::En => format!("{title} maintenance"),
        Locale::De => format!("{title} Wartung"),
    }
}
