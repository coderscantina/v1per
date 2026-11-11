//! Link preview tags for the HTML of a page the visitor may read: `description`, Open Graph and
//! Twitter card tags, and a canonical URL where the page's origin is known. Texts are in the
//! page's default locale. The gate never gets them, so a closed page's preview names nothing.

use v1per_protocol::operations::incidents::{Impact, IncidentState};
use v1per_protocol::status_page::{Locale, PageSnapshot, PageStatus, PublicIncident};

use super::{PageRow, logo};

/// Longest description, in characters.
const DESCRIPTION_MAX: usize = 200;

/// What a preview shows and where the page lives.
pub(super) struct Preview {
    pub title: String,
    pub description: String,
    /// The page's own URL without any token: `<origin><base><view>`; absent without an origin.
    pub url: Option<String>,
    /// An absolute raster logo URL.
    pub image: Option<String>,
}

impl Preview {
    /// The page: its title, and its status in words before its description.
    pub fn page(snapshot: &PageSnapshot, locale: Locale) -> Self {
        let status = status_sentence(locale, snapshot.status);
        let description = snapshot.description.trim();
        Self {
            title: snapshot.appearance.title.clone(),
            description: if description.is_empty() {
                format!("{status}.")
            } else {
                clip(&format!("{status}. {description}"))
            },
            url: None,
            image: None,
        }
    }

    /// One incident: its title, its state and impact in words, and its latest update.
    pub fn incident(incident: &PublicIncident, locale: Locale) -> Self {
        // A resolved incident has every component operational, so it shows its worst impact.
        let impact = match incident.state {
            IncidentState::Resolved => incident
                .updates
                .iter()
                .flat_map(|update| &update.impacts)
                .map(|impact| impact.impact)
                .max(),
            _ => incident.impact,
        }
        .filter(|impact| *impact > Impact::Operational);
        let mut description = format!("{}.", state_word(locale, incident.state));
        if let Some(impact) = impact {
            description.push_str(&format!(" {}.", impact_sentence(locale, impact)));
        }
        if let Some(update) = incident
            .updates
            .iter()
            .find(|update| !update.message.trim().is_empty())
        {
            description.push(' ');
            description.push_str(&clip(update.message.trim()));
        }
        Self {
            title: incident.title.clone(),
            description,
            url: None,
            image: None,
        }
    }

    /// Adds the page's URL at `path` below `base` and its raster logo, when `origin` is known.
    pub fn at(mut self, origin: Option<&str>, base: &str, path: &str, page: &PageRow) -> Self {
        let Some(origin) = origin else { return self };
        let path = format!("{base}{path}");
        self.url = Some(format!(
            "{origin}{}",
            if path.is_empty() { "/" } else { &path }
        ));
        let raster = page
            .logo_type
            .as_deref()
            .is_some_and(|kind| kind != "image/svg+xml");
        if raster && let Some(logo) = logo::path(page) {
            self.image = Some(format!("{origin}{base}{logo}"));
        }
        self
    }

    /// The tags, each value escaped for an attribute.
    pub fn tags(&self) -> String {
        let mut tags = String::new();
        let mut meta = |key: &str, name: &str, value: &str| {
            tags.push_str(&format!(
                "<meta {key}=\"{name}\" content=\"{}\">",
                attribute(value)
            ));
        };
        meta("name", "description", &self.description);
        meta("property", "og:type", "website");
        meta("property", "og:title", &self.title);
        meta("property", "og:description", &self.description);
        if let Some(url) = &self.url {
            meta("property", "og:url", url);
        }
        if let Some(image) = &self.image {
            meta("property", "og:image", image);
        }
        meta("name", "twitter:card", "summary");
        meta("name", "twitter:title", &self.title);
        meta("name", "twitter:description", &self.description);
        if let Some(image) = &self.image {
            meta("name", "twitter:image", image);
        }
        if let Some(url) = &self.url {
            tags.push_str(&format!(
                "<link rel=\"canonical\" href=\"{}\">",
                attribute(url)
            ));
        }
        tags
    }
}

/// `text` cut to [`DESCRIPTION_MAX`] characters, the last one an ellipsis when cut.
fn clip(text: &str) -> String {
    if text.chars().count() <= DESCRIPTION_MAX {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(DESCRIPTION_MAX - 1).collect();
    clipped.truncate(clipped.trim_end().len());
    clipped.push('…');
    clipped
}

/// `text` safe inside a double-quoted attribute.
fn attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn status_sentence(locale: Locale, status: PageStatus) -> &'static str {
    match (locale, status) {
        (Locale::En, PageStatus::Operational) => "All systems operational",
        (Locale::En, PageStatus::Maintenance) => "Maintenance in progress",
        (Locale::En, PageStatus::Degraded) => "Degraded performance",
        (Locale::En, PageStatus::PartialOutage) => "Partial outage",
        (Locale::En, PageStatus::MajorOutage) => "Major outage",
        (Locale::De, PageStatus::Operational) => "Alle Systeme betriebsbereit",
        (Locale::De, PageStatus::Maintenance) => "Wartung läuft",
        (Locale::De, PageStatus::Degraded) => "Eingeschränkte Leistung",
        (Locale::De, PageStatus::PartialOutage) => "Teilausfall",
        (Locale::De, PageStatus::MajorOutage) => "Größerer Ausfall",
    }
}

fn state_word(locale: Locale, state: IncidentState) -> &'static str {
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

fn impact_sentence(locale: Locale, impact: Impact) -> &'static str {
    match (locale, impact) {
        (Locale::En, Impact::Operational) => "Impact: none",
        (Locale::En, Impact::Degraded) => "Impact: degraded",
        (Locale::En, Impact::PartialOutage) => "Impact: partial outage",
        (Locale::En, Impact::MajorOutage) => "Impact: major outage",
        (Locale::De, Impact::Operational) => "Auswirkung: keine",
        (Locale::De, Impact::Degraded) => "Auswirkung: eingeschränkt",
        (Locale::De, Impact::PartialOutage) => "Auswirkung: Teilausfall",
        (Locale::De, Impact::MajorOutage) => "Auswirkung: Ausfall",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use v1per_protocol::ids::IncidentId;
    use v1per_protocol::status_page::PublicIncidentUpdate;

    #[test]
    fn hostile_values_stay_inside_their_attributes() {
        let preview = Preview {
            title: r#"A "quoted" <script>alert('x')</script> & co"#.into(),
            description: "\"><meta http-equiv=refresh content=0>".into(),
            url: Some("https://status.example.test/?a=1&b=\"2\"".into()),
            image: None,
        };
        let tags = preview.tags();
        assert!(!tags.contains("<script"));
        assert!(!tags.contains("\"><meta http-equiv"));
        assert!(tags.contains(
            "content=\"A &quot;quoted&quot; &lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; co\""
        ));
        assert!(tags.contains("href=\"https://status.example.test/?a=1&amp;b=&quot;2&quot;\""));
        // Every tag opens and closes on its own: no value added a quote or an angle bracket.
        assert_eq!(tags.matches('<').count(), tags.matches('>').count());
        assert_eq!(tags.matches('"').count() % 2, 0);
    }

    #[test]
    fn an_incident_reads_state_impact_and_its_latest_update_cut_short() {
        let incident = PublicIncident {
            id: IncidentId::new(1),
            title: "Checkout slow".into(),
            state: IncidentState::Resolved,
            impact: Some(Impact::Operational),
            started_at: 0,
            resolved_at: Some(1),
            components: Vec::new(),
            updates: vec![
                PublicIncidentUpdate {
                    state: IncidentState::Resolved,
                    message: "x".repeat(300),
                    created_at: 1,
                    impacts: Vec::new(),
                },
                PublicIncidentUpdate {
                    state: IncidentState::Investigating,
                    message: String::new(),
                    created_at: 0,
                    impacts: vec![v1per_protocol::status_page::PublicImpact {
                        component: v1per_protocol::ids::PageComponentId::new(1),
                        impact: Impact::PartialOutage,
                    }],
                },
            ],
            postmortem: None,
        };
        let preview = Preview::incident(&incident, Locale::En);
        let (head, update) = preview
            .description
            .split_at("Resolved. Impact: partial outage. ".len());
        assert_eq!(head, "Resolved. Impact: partial outage. ");
        assert_eq!(update.chars().count(), DESCRIPTION_MAX);
        assert!(update.ends_with('…'));
        assert!(
            Preview::incident(&incident, Locale::De)
                .description
                .starts_with("Behoben. Auswirkung: Teilausfall. x")
        );
    }
}
