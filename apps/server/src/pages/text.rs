//! The few texts the server writes on a page's behalf, in each page locale. Everything else
//! is the status page app's.

use v1per_protocol::operations::incidents::IncidentState;
use v1per_protocol::status_page::{Locale, PageStatus};

/// The title of an auto-incident: `name` is the component as the page shows it, absent when
/// the page does not show the monitor.
pub(super) fn monitor_down(locale: Locale, name: Option<&str>) -> String {
    match (locale, name) {
        (Locale::En, Some(name)) => format!("{name} is down"),
        (Locale::De, Some(name)) => format!("{name} ist ausgefallen"),
        (Locale::En, None) => "Service disruption".into(),
        (Locale::De, None) => "Dienststörung".into(),
    }
}

/// The message of an update the server wrote for an auto-incident. The check's own message
/// can name hosts and addresses, so pages never show it.
pub(super) fn auto_update(locale: Locale, state: IncidentState, name: Option<&str>) -> String {
    let resolved = state == IncidentState::Resolved;
    match (locale, name, resolved) {
        (Locale::En, Some(name), false) => {
            format!("{name} is not reachable. We are looking into it.")
        }
        (Locale::En, Some(name), true) => format!("{name} is reachable again."),
        (Locale::En, None, false) => "A service is not reachable. We are looking into it.".into(),
        (Locale::En, None, true) => "The service is reachable again.".into(),
        (Locale::De, Some(name), false) => {
            format!("{name} ist nicht erreichbar. Wir sehen uns das an.")
        }
        (Locale::De, Some(name), true) => format!("{name} ist wieder erreichbar."),
        (Locale::De, None, false) => {
            "Ein Dienst ist nicht erreichbar. Wir sehen uns das an.".into()
        }
        (Locale::De, None, true) => "Der Dienst ist wieder erreichbar.".into(),
    }
}

pub(super) fn status(locale: Locale, status: Option<PageStatus>) -> &'static str {
    match (locale, status) {
        (Locale::En, Some(PageStatus::Operational)) => "operational",
        (Locale::En, Some(PageStatus::Maintenance)) => "maintenance",
        (Locale::En, Some(PageStatus::Degraded)) => "degraded",
        (Locale::En, Some(PageStatus::PartialOutage)) => "partial outage",
        (Locale::En, Some(PageStatus::MajorOutage)) => "major outage",
        (Locale::En, None) => "no data",
        (Locale::De, Some(PageStatus::Operational)) => "in Betrieb",
        (Locale::De, Some(PageStatus::Maintenance)) => "Wartung",
        (Locale::De, Some(PageStatus::Degraded)) => "eingeschränkt",
        (Locale::De, Some(PageStatus::PartialOutage)) => "teilweiser Ausfall",
        (Locale::De, Some(PageStatus::MajorOutage)) => "schwerer Ausfall",
        (Locale::De, None) => "keine Daten",
    }
}

pub(super) fn status_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "status",
        Locale::De => "Status",
    }
}

/// `window` is `24h`, `30d` or `90d`.
pub(super) fn uptime_label(locale: Locale, window: &str) -> String {
    match locale {
        Locale::En => format!("uptime {window}"),
        Locale::De => format!("Verfügbarkeit {window}"),
    }
}

/// Subject and plain-text body of a sign-in link.
pub(super) fn sign_in_mail(locale: Locale, title: &str, link: &str) -> (String, String) {
    match locale {
        Locale::En => (
            format!("Sign in to {title}"),
            format!(
                "Open this link to view {title}:\n\n{link}\n\nThe link works once and for 15 \
                 minutes. If you did not ask for it, ignore this email.\n"
            ),
        ),
        Locale::De => (
            format!("Anmeldung bei {title}"),
            format!(
                "Öffnen Sie diesen Link, um {title} anzusehen:\n\n{link}\n\nDer Link \
                 funktioniert einmal und 15 Minuten lang. Wenn Sie ihn nicht angefordert \
                 haben, ignorieren Sie diese E-Mail.\n"
            ),
        ),
    }
}
