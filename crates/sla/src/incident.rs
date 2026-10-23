//! Response and resolution outcomes of one incident.
//!
//! An outcome compares counted time on the SLA's [`Clock`] with the target: within the
//! target, including exactly at it, is met.

use crate::business::Clock;
use chrono::{DateTime, TimeDelta, Utc};

/// Incident impact, ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Impact {
    Degraded,
    PartialOutage,
    MajorOutage,
}

/// The most severe impact, or `None` for none.
pub fn worst_impact(impacts: impl IntoIterator<Item = Impact>) -> Option<Impact> {
    impacts.into_iter().max()
}

/// Time targets per impact. A missing target means no commitment for that impact.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImpactTargets {
    pub major_outage: Option<TimeDelta>,
    pub partial_outage: Option<TimeDelta>,
    pub degraded: Option<TimeDelta>,
}

impl ImpactTargets {
    pub fn get(&self, impact: Impact) -> Option<TimeDelta> {
        match impact {
            Impact::MajorOutage => self.major_outage,
            Impact::PartialOutage => self.partial_outage,
            Impact::Degraded => self.degraded,
        }
    }
}

/// The instants both clocks read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncidentTimes {
    pub started_at: DateTime<Utc>,
    /// The first update with an author.
    pub first_human_update_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Met {
        took: TimeDelta,
    },
    /// `took` is set when a person answered after the deadline.
    Missed {
        deadline: DateTime<Utc>,
        took: Option<TimeDelta>,
    },
    Open {
        deadline: DateTime<Utc>,
    },
    /// Resolved within the target without a human update: neither met nor missed.
    NotNeeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Met {
        took: TimeDelta,
    },
    /// `took` is set once the incident is resolved.
    Missed {
        deadline: DateTime<Utc>,
        took: Option<TimeDelta>,
    },
    Open {
        deadline: DateTime<Utc>,
    },
}

/// The response outcome as of `now`. An update after the resolution does not count.
pub fn response(
    incident: &IncidentTimes,
    target: TimeDelta,
    clock: &Clock,
    now: DateTime<Utc>,
) -> Response {
    let started = incident.started_at;
    let deadline = clock.deadline(started, target);
    let update = incident.first_human_update_at.filter(|update| {
        incident
            .resolved_at
            .is_none_or(|resolved| *update <= resolved)
    });
    if let Some(update) = update {
        let took = clock.duration(started, update);
        return if took <= target {
            Response::Met { took }
        } else {
            Response::Missed {
                deadline,
                took: Some(took),
            }
        };
    }
    let within = clock.duration(started, incident.resolved_at.unwrap_or(now)) <= target;
    match (incident.resolved_at, within) {
        (_, false) => Response::Missed {
            deadline,
            took: None,
        },
        (Some(_), true) => Response::NotNeeded,
        (None, true) => Response::Open { deadline },
    }
}

/// The resolution outcome as of `now`.
pub fn resolution(
    incident: &IncidentTimes,
    target: TimeDelta,
    clock: &Clock,
    now: DateTime<Utc>,
) -> Resolution {
    let started = incident.started_at;
    let deadline = clock.deadline(started, target);
    match incident.resolved_at {
        Some(resolved) => {
            let took = clock.duration(started, resolved);
            if took <= target {
                Resolution::Met { took }
            } else {
                Resolution::Missed {
                    deadline,
                    took: Some(took),
                }
            }
        }
        None if clock.duration(started, now) <= target => Resolution::Open { deadline },
        None => Resolution::Missed {
            deadline,
            took: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        "2026-10-23T08:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("valid instant")
            + TimeDelta::minutes(minutes)
    }

    fn incident(update: Option<i64>, resolved: Option<i64>) -> IncidentTimes {
        IncidentTimes {
            started_at: at(0),
            first_human_update_at: update.map(at),
            resolved_at: resolved.map(at),
        }
    }

    const TARGET: TimeDelta = TimeDelta::minutes(30);

    #[test]
    fn worst_impact_follows_severity() {
        use Impact::*;
        assert_eq!(
            worst_impact([Degraded, MajorOutage, PartialOutage]),
            Some(MajorOutage)
        );
        assert_eq!(worst_impact([Degraded, PartialOutage]), Some(PartialOutage));
        assert_eq!(worst_impact([]), None);
    }

    #[test]
    fn missing_target_means_no_commitment() {
        let targets = ImpactTargets {
            major_outage: Some(TARGET),
            ..Default::default()
        };
        assert_eq!(targets.get(Impact::MajorOutage), Some(TARGET));
        assert_eq!(targets.get(Impact::Degraded), None);
    }

    #[test]
    fn response_met_by_a_human_update() {
        let outcome = response(&incident(Some(20), None), TARGET, &Clock::AllHours, at(60));
        assert_eq!(
            outcome,
            Response::Met {
                took: TimeDelta::minutes(20)
            }
        );
    }

    #[test]
    fn response_open_then_missed() {
        let open = incident(None, None);
        assert_eq!(
            response(&open, TARGET, &Clock::AllHours, at(10)),
            Response::Open { deadline: at(30) }
        );
        assert_eq!(
            response(&open, TARGET, &Clock::AllHours, at(31)),
            Response::Missed {
                deadline: at(30),
                took: None
            }
        );
    }

    #[test]
    fn self_healed_incident_needs_no_response() {
        let healed = incident(None, Some(12));
        assert_eq!(
            response(&healed, TARGET, &Clock::AllHours, at(60)),
            Response::NotNeeded
        );
        // An update written after the resolution changes nothing.
        let late_note = incident(Some(40), Some(12));
        assert_eq!(
            response(&late_note, TARGET, &Clock::AllHours, at(60)),
            Response::NotNeeded
        );
    }

    #[test]
    fn resolution_compares_time_to_resolve() {
        let fast = incident(None, Some(25));
        assert_eq!(
            resolution(&fast, TARGET, &Clock::AllHours, at(60)),
            Resolution::Met {
                took: TimeDelta::minutes(25)
            }
        );
        let slow = incident(Some(5), Some(45));
        assert_eq!(
            resolution(&slow, TARGET, &Clock::AllHours, at(60)),
            Resolution::Missed {
                deadline: at(30),
                took: Some(TimeDelta::minutes(45))
            }
        );
        let open = incident(Some(5), None);
        assert_eq!(
            resolution(&open, TARGET, &Clock::AllHours, at(29)),
            Resolution::Open { deadline: at(30) }
        );
    }
}
