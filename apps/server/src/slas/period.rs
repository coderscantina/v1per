//! One SLA period computed from the terms and the data: availability in both modes with its
//! downtime budget, average latency, and the response and resolution clocks of the incidents
//! that started in it (monitoring contract "SLAs"). The rules live in `v1per-sla`; this module
//! reads the records they need.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc, Weekday};
use chrono_tz::Tz;
use sqlx::SqliteConnection;
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::{IncidentId, MonitorId};
use v1per_protocol::operations::incidents::Impact;
use v1per_protocol::operations::slas::{
    Sla, SlaAvailabilityStanding, SlaClock, SlaClockOutcome, SlaClockSummary, SlaCombine,
    SlaIncidentStanding, SlaInput, SlaMonitorStanding, SlaOutcome, SlaPeriod, SlaPeriodKind,
    SlaTimeTarget,
};
use v1per_sla::{
    self as rules, BudgetBasis, BudgetState, BusinessHours, Clock, DowntimeBudget, Period,
    PeriodCalendar, PeriodKind, WeeklyRange,
};
use v1per_storage::{self as storage, ReadError, Storage};

use super::{DATE, json_ids, minute};
use crate::error::{ApiError, FieldErrors};

/// A period freezes this long after it ends, so late agent results are in. Days are tallied
/// after the same delay.
pub(crate) const FREEZE_DELAY_MS: i64 = 3_600_000;
/// Monitor types whose up beats carry a latency.
const LATENCY_TYPES: [&str; 6] = ["http", "keyword", "json-query", "port", "ping", "dns"];

pub(super) fn read_failed(error: ReadError) -> ApiError {
    match error {
        ReadError::Busy | ReadError::Timeout => ApiError::new(ErrorCode::QueryTimeout),
        other => ApiError::internal(other),
    }
}

pub(crate) fn instant(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).expect("instant in range")
}

fn millis(delta: TimeDelta) -> i64 {
    delta.num_milliseconds()
}

/// The stored timezone and start date passed validation when they were written.
pub(crate) fn calendar(terms: &SlaInput) -> Result<PeriodCalendar, ApiError> {
    let zone: Tz = terms.timezone.parse().map_err(ApiError::internal)?;
    let start = NaiveDate::parse_from_str(&terms.start_date, DATE).map_err(ApiError::internal)?;
    let kind = match terms.period {
        SlaPeriodKind::Month => PeriodKind::Month,
        SlaPeriodKind::Quarter => PeriodKind::Quarter,
        SlaPeriodKind::Year => PeriodKind::Year,
    };
    Ok(PeriodCalendar { kind, zone, start })
}

/// The period starting on `start`, or the one running at `now` without it (the first one
/// while the contract has not started). `invalid_value` on `start` unless it is the first day
/// of a period.
pub(crate) fn resolve(
    calendar: &PeriodCalendar,
    start: Option<&str>,
    now: i64,
) -> Result<Period, ApiError> {
    let Some(start) = start else {
        let first = rules::day_start(calendar.zone, calendar.start);
        let at = instant(now).max(first);
        return Ok(calendar.containing(at).expect("on or after the start date"));
    };
    let day = NaiveDate::parse_from_str(start.trim(), DATE)
        .map_err(|_| FieldErrors::one("start", FieldErrorCode::InvalidFormat, None))?;
    calendar
        .containing(rules::day_start(calendar.zone, day))
        .filter(|period| period.first_day == day)
        .ok_or_else(|| FieldErrors::one("start", FieldErrorCode::InvalidValue, None))
}

/// The response and resolution clock of the terms: business hours when there are any.
fn business_clock(terms: &SlaInput, zone: Tz) -> Clock {
    const WEEKDAYS: [Weekday; 7] = [
        Weekday::Sun,
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
    ];
    let time = |minute: u32| {
        NaiveTime::from_hms_opt(minute / 60 % 24, minute % 60, 0).expect("minute of a day")
    };
    let ranges = terms.hours.iter().filter_map(|range| {
        Some(WeeklyRange {
            weekday: *WEEKDAYS.get(usize::from(range.weekday))?,
            start: time(minute(&range.start, false)?),
            // 24:00 becomes 00:00, which the rules read as the end of the day.
            end: time(minute(&range.end, true)?),
        })
    });
    let holidays = terms
        .holidays
        .iter()
        .filter_map(|day| NaiveDate::parse_from_str(day, DATE).ok());
    BusinessHours::new(zone, ranges, holidays).map_or(Clock::AllHours, Clock::Business)
}

/// How a commitment stands from whether it missed, is at risk (running periods only) and has
/// data.
fn outcome(closed: bool, missed: bool, at_risk: bool, has_data: bool) -> SlaOutcome {
    if missed {
        SlaOutcome::Missed
    } else if !has_data {
        SlaOutcome::NoData
    } else if closed {
        SlaOutcome::Met
    } else if at_risk {
        SlaOutcome::AtRisk
    } else {
        SlaOutcome::OnTrack
    }
}

/// The SLA's outcome from its parts, the worst one: missed, at risk, on track or met. No data
/// when none has data.
pub(crate) fn combined(outcomes: impl IntoIterator<Item = SlaOutcome>, closed: bool) -> SlaOutcome {
    let mut has_data = false;
    let mut at_risk = false;
    for part in outcomes {
        match part {
            SlaOutcome::Missed => return SlaOutcome::Missed,
            SlaOutcome::AtRisk => {
                at_risk = true;
                has_data = true;
            }
            SlaOutcome::NoData => {}
            SlaOutcome::Met | SlaOutcome::OnTrack => has_data = true,
        }
    }
    outcome(closed, false, at_risk, has_data)
}

/// Counted and down minutes of the service over the period up to `now`: the tallies of closed
/// days, and the minute rollups for the days not tallied yet. A tally of 0 counted minutes is
/// a day without minute data and adds nothing.
pub(crate) async fn service_minutes(
    conn: &mut SqliteConnection,
    storage: &Storage,
    sla: &Sla,
    calendar: &PeriodCalendar,
    period: &Period,
    now: i64,
) -> Result<(u64, u64), ApiError> {
    let tallies: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT day, counted_minutes, down_minutes FROM sla_tallies
         WHERE sla_id = ? AND day >= ? AND day < ?",
    )
    .bind(sla.id.get())
    .bind(period.first_day.format(DATE).to_string())
    .bind(period.end_day.format(DATE).to_string())
    .fetch_all(&mut *conn)
    .await?;
    let mut counted = 0;
    let mut down = 0;
    let mut tallied = HashSet::new();
    for (day, day_counted, day_down) in tallies {
        counted += day_counted as u64;
        down += day_down as u64;
        tallied.insert(day);
    }
    // Days without a tally, merged into runs so each run is one read. Minute rollups expire
    // oldest first, so a run covered at its start is covered throughout.
    let mut runs: Vec<(i64, i64)> = Vec::new();
    let mut day = period.first_day;
    while day < period.end_day {
        let from = rules::day_start(calendar.zone, day).timestamp_millis();
        if from >= now {
            break;
        }
        let next = day.succ_opt().expect("date in range");
        if !tallied.contains(&day.format(DATE).to_string()) {
            let to = rules::day_start(calendar.zone, next)
                .timestamp_millis()
                .min(now);
            match runs.last_mut() {
                Some(run) if run.1 == from => run.1 = to,
                _ => runs.push((from, to)),
            }
        }
        day = next;
    }
    let monitors = storage_ids(&sla.terms.monitors);
    for (from, to) in runs {
        let minutes = storage
            .down_minutes(&monitors, from, to)
            .await
            .map_err(read_failed)?
            .value;
        if minutes.covered {
            counted += minutes.counted;
            down += minutes.down;
        }
    }
    Ok((counted, down))
}

pub(crate) fn storage_ids(monitors: &[MonitorId]) -> Vec<storage::MonitorId> {
    monitors
        .iter()
        .map(|m| storage::MonitorId(m.get()))
        .collect()
}

#[derive(sqlx::FromRow)]
struct MonitorRow {
    id: i64,
    name: String,
    #[sqlx(rename = "type")]
    kind: String,
    interval: i64,
}

#[derive(sqlx::FromRow)]
struct IncidentRow {
    id: i64,
    title: String,
    started_at: i64,
    resolved_at: Option<i64>,
    auto_monitor_id: Option<i64>,
}

fn rules_impact(impact: &str) -> Option<rules::Impact> {
    match impact {
        "major_outage" => Some(rules::Impact::MajorOutage),
        "partial_outage" => Some(rules::Impact::PartialOutage),
        "degraded" => Some(rules::Impact::Degraded),
        _ => None,
    }
}

fn wire_impact(impact: Option<rules::Impact>) -> Impact {
    match impact {
        Some(rules::Impact::MajorOutage) => Impact::MajorOutage,
        Some(rules::Impact::PartialOutage) => Impact::PartialOutage,
        Some(rules::Impact::Degraded) => Impact::Degraded,
        None => Impact::Operational,
    }
}

fn target_for(terms: &SlaInput, impact: Option<rules::Impact>) -> SlaTimeTarget {
    match impact {
        Some(rules::Impact::MajorOutage) => terms.times.major_outage,
        Some(rules::Impact::PartialOutage) => terms.times.partial_outage,
        Some(rules::Impact::Degraded) => terms.times.degraded,
        None => SlaTimeTarget::default(),
    }
}

/// Business time from the start to `now`, for a clock that has not stopped.
fn so_far(clock: &Clock, times: &rules::IncidentTimes, now: DateTime<Utc>) -> Option<i64> {
    Some(millis(clock.duration(times.started_at, now)))
}

fn response_clock(
    clock: &Clock,
    times: &rules::IncidentTimes,
    minutes: Option<u32>,
    now: DateTime<Utc>,
) -> SlaClock {
    let silent = |outcome| SlaClock {
        outcome,
        elapsed_ms: None,
        deadline: None,
    };
    let Some(minutes) = minutes else {
        return silent(SlaClockOutcome::NoTarget);
    };
    let target = TimeDelta::minutes(i64::from(minutes));
    match rules::response(times, target, clock, now) {
        rules::Response::Met { took } => SlaClock {
            outcome: SlaClockOutcome::Met,
            elapsed_ms: Some(millis(took)),
            deadline: Some(clock.deadline(times.started_at, target).timestamp_millis()),
        },
        rules::Response::Missed { deadline, took } => SlaClock {
            outcome: SlaClockOutcome::Missed,
            elapsed_ms: match (took, times.resolved_at) {
                (Some(took), _) => Some(millis(took)),
                (None, None) => so_far(clock, times, now),
                (None, Some(_)) => None,
            },
            deadline: Some(deadline.timestamp_millis()),
        },
        rules::Response::Open { deadline } => SlaClock {
            outcome: SlaClockOutcome::Running,
            elapsed_ms: so_far(clock, times, now),
            deadline: Some(deadline.timestamp_millis()),
        },
        rules::Response::NotNeeded => silent(SlaClockOutcome::NotNeeded),
    }
}

fn resolution_clock(
    clock: &Clock,
    times: &rules::IncidentTimes,
    minutes: Option<u32>,
    now: DateTime<Utc>,
) -> SlaClock {
    let Some(minutes) = minutes else {
        return SlaClock {
            outcome: SlaClockOutcome::NoTarget,
            elapsed_ms: None,
            deadline: None,
        };
    };
    let target = TimeDelta::minutes(i64::from(minutes));
    match rules::resolution(times, target, clock, now) {
        rules::Resolution::Met { took } => SlaClock {
            outcome: SlaClockOutcome::Met,
            elapsed_ms: Some(millis(took)),
            deadline: Some(clock.deadline(times.started_at, target).timestamp_millis()),
        },
        rules::Resolution::Missed { deadline, took } => SlaClock {
            outcome: SlaClockOutcome::Missed,
            elapsed_ms: took.map(millis).or_else(|| so_far(clock, times, now)),
            deadline: Some(deadline.timestamp_millis()),
        },
        rules::Resolution::Open { deadline } => SlaClock {
            outcome: SlaClockOutcome::Running,
            elapsed_ms: so_far(clock, times, now),
            deadline: Some(deadline.timestamp_millis()),
        },
    }
}

/// The clocks that met or missed their target, and how the commitment stands. `no_data`: no
/// covered monitor has data and no incident started, so there is nothing to judge.
fn clock_summary<'a>(
    clocks: impl Iterator<Item = &'a SlaClock>,
    closed: bool,
    no_data: bool,
) -> SlaClockSummary {
    let (mut counted, mut met, mut total, mut timed, mut running) = (0, 0, 0, 0, false);
    for clock in clocks {
        match clock.outcome {
            SlaClockOutcome::Met => met += 1,
            SlaClockOutcome::Missed => {}
            SlaClockOutcome::Running => {
                running = true;
                continue;
            }
            SlaClockOutcome::NoTarget | SlaClockOutcome::NotNeeded => continue,
        }
        counted += 1;
        if let Some(elapsed) = clock.elapsed_ms {
            total += elapsed;
            timed += 1;
        }
    }
    let outcome = if met < counted {
        SlaOutcome::Missed
    } else if no_data {
        SlaOutcome::NoData
    } else if closed && !running {
        SlaOutcome::Met
    } else {
        SlaOutcome::OnTrack
    };
    SlaClockSummary {
        outcome,
        counted,
        met,
        average_ms: (timed > 0).then(|| total / timed),
    }
}

async fn incidents(
    conn: &mut SqliteConnection,
    sla: &Sla,
    clock: &Clock,
    period: &Period,
    unsettled_after: Option<i64>,
    now: DateTime<Utc>,
) -> Result<Vec<SlaIncidentStanding>, ApiError> {
    let covered: Vec<i64> = sla.terms.monitors.iter().map(|m| m.get()).collect();
    let covered_json = json_ids(&covered);
    let rows: Vec<IncidentRow> = sqlx::query_as(
        "SELECT i.id, i.title, i.started_at, i.resolved_at, i.auto_monitor_id FROM incidents i
         WHERE i.started_at >= ?2 AND i.started_at < ?3
           AND (?4 IS NULL OR i.resolved_at IS NULL OR i.resolved_at > ?4) AND EXISTS (
             SELECT 1 FROM incident_targets t
             WHERE t.incident_id = i.id AND t.monitor_id IN (SELECT value FROM json_each(?1)))
         ORDER BY i.started_at, i.id",
    )
    .bind(&covered_json)
    .bind(period.starts_at.timestamp_millis())
    .bind(period.ends_at.timestamp_millis())
    .bind(unsettled_after)
    .fetch_all(&mut *conn)
    .await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids = json_ids(&rows.iter().map(|row| row.id).collect::<Vec<_>>());
    let impacts: Vec<(i64, String)> = sqlx::query_as(
        "SELECT u.incident_id, i.impact
         FROM incident_update_impacts i JOIN incident_updates u ON u.id = i.update_id
         WHERE u.incident_id IN (SELECT value FROM json_each(?1))
           AND i.monitor_id IN (SELECT value FROM json_each(?2))",
    )
    .bind(&ids)
    .bind(&covered_json)
    .fetch_all(&mut *conn)
    .await?;
    let mut worst: HashMap<i64, rules::Impact> = HashMap::new();
    for (incident, impact) in impacts {
        if let Some(impact) = rules_impact(&impact) {
            let entry = worst.entry(incident).or_insert(impact);
            *entry = (*entry).max(impact);
        }
    }
    let answered: HashMap<i64, i64> = sqlx::query_as(
        "SELECT incident_id, min(created_at) FROM incident_updates
         WHERE incident_id IN (SELECT value FROM json_each(?)) AND author_id IS NOT NULL
         GROUP BY incident_id",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();
    Ok(rows
        .into_iter()
        .map(|row| {
            let mut impact = worst.get(&row.id).copied();
            // An auto-incident on a covered monitor starts at a major outage.
            if row.auto_monitor_id.is_some_and(|m| covered.contains(&m)) {
                impact = Some(rules::Impact::MajorOutage);
            }
            let times = rules::IncidentTimes {
                started_at: instant(row.started_at),
                first_human_update_at: answered.get(&row.id).copied().map(instant),
                resolved_at: row.resolved_at.map(instant),
            };
            let target = target_for(&sla.terms, impact);
            SlaIncidentStanding {
                incident: IncidentId::new(row.id),
                title: row.title,
                started_at: row.started_at,
                resolved_at: row.resolved_at,
                impact: wire_impact(impact),
                response: response_clock(clock, &times, target.response_minutes, now),
                resolution: resolution_clock(clock, &times, target.resolution_minutes, now),
            }
        })
        .collect())
}

/// `period` of `sla` against its current terms as of `now`. `frozen_at` is left empty.
pub(crate) async fn compute(
    conn: &mut SqliteConnection,
    storage: &Storage,
    sla: &Sla,
    period: &Period,
    now: i64,
) -> Result<SlaPeriod, ApiError> {
    let terms = &sla.terms;
    let calendar = calendar(terms)?;
    let closed = now >= period.ends_at.timestamp_millis();
    let monitors: Vec<MonitorRow> = sqlx::query_as(
        "SELECT id, name, type, interval FROM monitors
         WHERE id IN (SELECT value FROM json_each(?)) ORDER BY name, id",
    )
    .bind(json_ids(
        &terms.monitors.iter().map(|m| m.get()).collect::<Vec<_>>(),
    ))
    .fetch_all(&mut *conn)
    .await?;
    let ids: Vec<storage::MonitorId> = monitors.iter().map(|m| storage::MonitorId(m.id)).collect();
    let totals: BTreeMap<storage::MonitorId, storage::PeriodTotals> = storage
        .period_totals(
            &ids,
            period.starts_at.timestamp_millis(),
            period.ends_at.timestamp_millis(),
        )
        .await
        .map_err(read_failed)?
        .value;
    let target = terms.availability.map(|percent| percent / 100.0);
    let each = terms.combine == SlaCombine::Each;
    let mut standings = Vec::with_capacity(monitors.len());
    for monitor in monitors {
        let totals = totals
            .get(&storage::MonitorId(monitor.id))
            .copied()
            .unwrap_or_default();
        let availability = rules::availability(totals.up, totals.down);
        let (availability_outcome, budget_used) = match target.filter(|_| each) {
            Some(target) => {
                let interval = TimeDelta::seconds(monitor.interval.max(1));
                let budget =
                    DowntimeBudget::new(target, period.length(), BudgetBasis::Beats { interval });
                let missed = budget.state(totals.down) == BudgetState::Exceeded
                    || closed && rules::below_target(availability, target);
                let at_risk = !closed && budget.state(totals.down) == BudgetState::Warning;
                (
                    Some(outcome(closed, missed, at_risk, availability.is_some())),
                    budget.share_used(totals.down),
                )
            }
            None => (None, None),
        };
        let has_latency = LATENCY_TYPES.contains(&monitor.kind.as_str());
        let latency = has_latency
            .then(|| rules::average_latency(totals.latency_sum, totals.latency_count))
            .flatten();
        let latency_outcome = terms.latency_ms.filter(|_| has_latency).map(|target| {
            let missed = closed && rules::latency_breached(latency, f64::from(target));
            let at_risk = !closed && rules::latency_breached(latency, f64::from(target));
            outcome(closed, missed, at_risk, latency.is_some())
        });
        standings.push(SlaMonitorStanding {
            monitor: MonitorId::new(monitor.id),
            name: monitor.name,
            availability: availability.map(|ratio| ratio * 100.0),
            availability_outcome,
            budget_used,
            latency_ms: latency,
            latency_outcome,
        });
    }

    let availability = match target {
        None => None,
        Some(_) if each => Some(SlaAvailabilityStanding {
            outcome: combined(
                standings.iter().filter_map(|m| m.availability_outcome),
                closed,
            ),
            actual: standings
                .iter()
                .filter_map(|m| m.availability)
                .min_by(f64::total_cmp),
            budget_used: standings
                .iter()
                .filter_map(|m| m.budget_used)
                .max_by(f64::total_cmp),
        }),
        Some(target) => {
            let (counted, down) =
                service_minutes(conn, storage, sla, &calendar, period, now).await?;
            let actual = rules::any_down_availability(counted, down);
            let budget = DowntimeBudget::new(target, period.length(), BudgetBasis::Minutes);
            let missed = budget.state(down) == BudgetState::Exceeded
                || closed && rules::below_target(actual, target);
            let at_risk = !closed && budget.state(down) == BudgetState::Warning;
            Some(SlaAvailabilityStanding {
                outcome: outcome(closed, missed, at_risk, actual.is_some()),
                actual: actual.map(|ratio| ratio * 100.0),
                budget_used: budget.share_used(down),
            })
        }
    };
    let latency = terms
        .latency_ms
        .map(|_| combined(standings.iter().filter_map(|m| m.latency_outcome), closed));

    let clock = business_clock(terms, calendar.zone);
    let incidents = incidents(conn, sla, &clock, period, None, instant(now)).await?;
    let has_data = standings.iter().any(|m| m.availability.is_some())
        || availability.is_some_and(|a| a.actual.is_some());
    let no_data = !has_data && incidents.is_empty();
    let impacts = [
        terms.times.major_outage,
        terms.times.partial_outage,
        terms.times.degraded,
    ];
    let response = impacts
        .iter()
        .any(|t| t.response_minutes.is_some())
        .then(|| clock_summary(incidents.iter().map(|i| &i.response), closed, no_data));
    let resolution = impacts
        .iter()
        .any(|t| t.resolution_minutes.is_some())
        .then(|| clock_summary(incidents.iter().map(|i| &i.resolution), closed, no_data));

    Ok(SlaPeriod {
        sla: sla.id,
        start: period.first_day.format(DATE).to_string(),
        starts_at: period.starts_at.timestamp_millis(),
        ends_at: period.ends_at.timestamp_millis(),
        frozen_at: None,
        terms: terms.clone(),
        availability,
        latency,
        response,
        resolution,
        monitors: standings,
        incidents,
    })
}

/// The incidents of the frozen `period` that were open at `frozen_at` or resolved after it,
/// with their clocks at `now`, in a period holding nothing else. Their missed clocks give the
/// signals the frozen result could not, as the same `sla_alerts` keys the running period used.
pub(crate) async fn late_clocks(
    conn: &mut SqliteConnection,
    sla: &Sla,
    period: &Period,
    frozen_at: i64,
    now: i64,
) -> Result<SlaPeriod, ApiError> {
    let terms = &sla.terms;
    let clock = business_clock(terms, calendar(terms)?.zone);
    let incidents = incidents(conn, sla, &clock, period, Some(frozen_at), instant(now)).await?;
    Ok(SlaPeriod {
        sla: sla.id,
        start: period.first_day.format(DATE).to_string(),
        starts_at: period.starts_at.timestamp_millis(),
        ends_at: period.ends_at.timestamp_millis(),
        frozen_at: Some(frozen_at),
        terms: terms.clone(),
        availability: None,
        latency: None,
        response: None,
        resolution: None,
        monitors: Vec::new(),
        incidents,
    })
}

/// An alert a period's standing calls for, as `sla_alerts` keys it: `kind` is the outbox
/// event, `subject` 0 for the whole SLA, a monitor ID for a per-monitor breach, an incident ID
/// for a missed response or resolution. Each holds from the moment it first appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signal {
    pub kind: &'static str,
    pub subject: i64,
}

/// The alerts `period` calls for now. The caller sends those it has not sent for the period.
pub fn signals(period: &SlaPeriod) -> Vec<Signal> {
    let mut signals = Vec::new();
    let mut add = |kind, subject| signals.push(Signal { kind, subject });
    if let Some(availability) = &period.availability {
        if availability
            .budget_used
            .is_some_and(|used| used >= rules::WARNING_SHARE)
        {
            add("sla_budget_warning", 0);
        }
        if period.terms.combine == SlaCombine::AnyDown && availability.outcome == SlaOutcome::Missed
        {
            add("sla_availability_breach", 0);
        }
    }
    for monitor in &period.monitors {
        if monitor.availability_outcome == Some(SlaOutcome::Missed) {
            add("sla_availability_breach", monitor.monitor.get());
        }
        if monitor.latency_outcome == Some(SlaOutcome::Missed) {
            add("sla_latency_breach", monitor.monitor.get());
        }
    }
    for incident in &period.incidents {
        if incident.response.outcome == SlaClockOutcome::Missed {
            add("sla_response_missed", incident.incident.get());
        }
        if incident.resolution.outcome == SlaClockOutcome::Missed {
            add("sla_resolution_missed", incident.incident.get());
        }
    }
    signals
}
