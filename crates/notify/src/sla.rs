//! SLA warnings and breaches (decision 0012), a V1Per event Kuma does not have. Every provider
//! sends them like Kuma's certificate and probe offline messages: `msg` alone, with
//! `monitorJSON` and `heartbeatJSON` empty. The text is in English or German.

use serde::{Deserialize, Serialize};

/// The language of an SLA message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    De,
}

/// One SLA alert for one period.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlaAlert {
    pub language: Language,
    pub sla: String,
    /// The name of the SLA's node.
    pub node: String,
    /// First and last day of the period, `YYYY-MM-DD` in the SLA's timezone.
    pub first_day: String,
    pub last_day: String,
    pub kind: SlaAlertKind,
}

/// What the alert is about. Percentages are in percent, `used` a share where 1 is all of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SlaAlertKind {
    BudgetWarning {
        used: f64,
        actual: Option<f64>,
        target: f64,
    },
    /// `monitor` names the monitor that missed in `each` mode; absent for the service in
    /// `any_down` mode.
    AvailabilityBreach {
        monitor: Option<String>,
        actual: Option<f64>,
        target: f64,
    },
    LatencyBreach {
        monitor: String,
        actual_ms: Option<f64>,
        target_ms: u32,
    },
    ResponseMissed {
        incident: SlaIncident,
        elapsed_ms: Option<i64>,
        target_minutes: u32,
    },
    ResolutionMissed {
        incident: SlaIncident,
        elapsed_ms: Option<i64>,
        target_minutes: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlaIncident {
    pub id: i64,
    pub title: String,
}

/// A percentage with at most three decimals: `99.95%`, `99,95 %`.
fn percent(language: Language, value: f64) -> String {
    let text = format!("{:.3}", (value * 1000.0).round() / 1000.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match language {
        Language::En => format!("{text}%"),
        Language::De => format!("{} %", text.replace('.', ",")),
    }
}

/// Whole minutes as `45 min`, `4 h` or `1 h 30 min`.
fn duration(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

impl SlaAlert {
    /// One line such as `[Acme hosting] [🔴 SLA availability missed] API: 98.5% (target
    /// 99.9%). Acme, period 2026-10-01 to 2026-10-31`.
    pub(crate) fn message(&self) -> String {
        let de = self.language == Language::De;
        let pct = |value: f64| percent(self.language, value);
        let no_data = if de { "keine Daten" } else { "no data" };
        let or_no_data = |value: Option<String>| value.unwrap_or_else(|| no_data.to_owned());
        let target = if de { "Ziel" } else { "target" };
        let incident = |incident: &SlaIncident| {
            let word = if de { "Vorfall" } else { "Incident" };
            format!("{word} #{} \"{}\"", incident.id, incident.title)
        };
        let (label, detail) = match &self.kind {
            SlaAlertKind::BudgetWarning {
                used,
                actual,
                target: goal,
            } => {
                let used = (used * 100.0).floor() as i64;
                let actual = or_no_data(actual.map(pct));
                if de {
                    (
                        "⚠️ SLA-Budget",
                        format!(
                            "{used} % des Ausfallbudgets verbraucht, Verfügbarkeit {actual} \
                             ({target} {})",
                            pct(*goal)
                        ),
                    )
                } else {
                    (
                        "⚠️ SLA budget",
                        format!(
                            "{used}% of the downtime budget used, availability {actual} \
                             ({target} {})",
                            pct(*goal)
                        ),
                    )
                }
            }
            SlaAlertKind::AvailabilityBreach {
                monitor,
                actual,
                target: goal,
            } => {
                let label = if de {
                    "🔴 SLA-Verfügbarkeit verfehlt"
                } else {
                    "🔴 SLA availability missed"
                };
                let subject = match monitor {
                    Some(monitor) => monitor.clone(),
                    None if de => "Verfügbarkeit".to_owned(),
                    None => "Availability".to_owned(),
                };
                let actual = or_no_data(actual.map(pct));
                (
                    label,
                    format!("{subject}: {actual} ({target} {})", pct(*goal)),
                )
            }
            SlaAlertKind::LatencyBreach {
                monitor,
                actual_ms,
                target_ms,
            } => {
                let (label, measure) = if de {
                    ("🔴 SLA-Latenz verfehlt", "durchschnittliche Latenz")
                } else {
                    ("🔴 SLA latency missed", "average latency")
                };
                let actual = or_no_data(actual_ms.map(|ms| format!("{} ms", ms.round() as i64)));
                (
                    label,
                    format!("{monitor}: {measure} {actual} ({target} {target_ms} ms)"),
                )
            }
            SlaAlertKind::ResponseMissed {
                incident: which,
                elapsed_ms,
                target_minutes,
            }
            | SlaAlertKind::ResolutionMissed {
                incident: which,
                elapsed_ms,
                target_minutes,
            } => {
                let response = matches!(self.kind, SlaAlertKind::ResponseMissed { .. });
                let (label, measure) = match (response, de) {
                    (true, false) => ("🔴 SLA response missed", "response time"),
                    (true, true) => ("🔴 SLA-Reaktionszeit verfehlt", "Reaktionszeit"),
                    (false, false) => ("🔴 SLA resolution missed", "resolution time"),
                    (false, true) => ("🔴 SLA-Lösungszeit verfehlt", "Lösungszeit"),
                };
                let actual = or_no_data(elapsed_ms.map(|ms| duration(ms / 60_000)));
                (
                    label,
                    format!(
                        "{}: {measure} {actual} ({target} {})",
                        incident(which),
                        duration(i64::from(*target_minutes))
                    ),
                )
            }
        };
        let period = if de {
            format!("Zeitraum {} bis {}", self.first_day, self.last_day)
        } else {
            format!("period {} to {}", self.first_day, self.last_day)
        };
        format!("[{}] [{label}] {detail}. {}, {period}", self.sla, self.node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(language: Language, kind: SlaAlertKind) -> String {
        SlaAlert {
            language,
            sla: "Acme hosting".into(),
            node: "Acme".into(),
            first_day: "2026-10-01".into(),
            last_day: "2026-10-31".into(),
            kind,
        }
        .message()
    }

    fn incident() -> SlaIncident {
        SlaIncident {
            id: 12,
            title: "Database down".into(),
        }
    }

    #[test]
    fn english_names_the_sla_subject_figures_node_and_period() {
        let en = |kind| alert(Language::En, kind);
        assert_eq!(
            en(SlaAlertKind::BudgetWarning {
                used: 0.8213,
                actual: Some(99.9179),
                target: 99.9
            }),
            "[Acme hosting] [⚠️ SLA budget] 82% of the downtime budget used, availability \
             99.918% (target 99.9%). Acme, period 2026-10-01 to 2026-10-31"
        );
        assert_eq!(
            en(SlaAlertKind::AvailabilityBreach {
                monitor: Some("API".into()),
                actual: Some(98.5),
                target: 99.9
            }),
            "[Acme hosting] [🔴 SLA availability missed] API: 98.5% (target 99.9%). Acme, \
             period 2026-10-01 to 2026-10-31"
        );
        assert!(
            en(SlaAlertKind::LatencyBreach {
                monitor: "API".into(),
                actual_ms: Some(812.4),
                target_ms: 500
            })
            .contains("[🔴 SLA latency missed] API: average latency 812 ms (target 500 ms).")
        );
        assert!(
            en(SlaAlertKind::ResolutionMissed {
                incident: incident(),
                elapsed_ms: Some(312 * 60_000),
                target_minutes: 240
            })
            .contains(
                "[🔴 SLA resolution missed] Incident #12 \"Database down\": resolution time \
                 5 h 12 min (target 4 h)."
            )
        );
    }

    #[test]
    fn german_uses_its_words_and_decimal_comma() {
        let de = |kind| alert(Language::De, kind);
        assert_eq!(
            de(SlaAlertKind::AvailabilityBreach {
                monitor: None,
                actual: None,
                target: 99.95
            }),
            "[Acme hosting] [🔴 SLA-Verfügbarkeit verfehlt] Verfügbarkeit: keine Daten \
             (Ziel 99,95 %). Acme, Zeitraum 2026-10-01 bis 2026-10-31"
        );
        assert!(
            de(SlaAlertKind::ResponseMissed {
                incident: incident(),
                elapsed_ms: Some(45 * 60_000),
                target_minutes: 30
            })
            .contains(
                "[🔴 SLA-Reaktionszeit verfehlt] Vorfall #12 \"Database down\": Reaktionszeit \
                 45 min (Ziel 30 min)."
            )
        );
    }
}
