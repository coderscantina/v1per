//! Maintenance windows for Kuma's six strategies. Pure computation: a [`Definition`] and a
//! time range in, windows and statuses out. Semantics are read from Uptime Kuma 2.5.0
//! `server/model/maintenance.js`; line numbers below refer to that file.
//!
//! Kuma keeps the status of a recurring maintenance in memory, driven by croner timers. Here
//! it is derived from the definition alone, so the same answer comes back after a restart.
//!
//! Rules that hold for every strategy:
//! - Windows are half-open `[start, end)` in UTC. Kuma compares the date range with strict
//!   `isBefore`/`isAfter` (`:431`, `:436`), so at the exact end instant it still reports the
//!   old state; here the status changes at the end instant, so `next_change` is exact.
//! - `start_date` and `end_date` bound the windows, which are cut at both ends. A window
//!   that began before `start_date` counts from `start_date` (Kuma's status check lets the
//!   running timer through, `:431` then `:445`); `inferDuration` cuts at `end_date`
//!   (`:366-376`).
//! - Date range and cron times are wall clock in the maintenance timezone. A window's length
//!   is absolute seconds (`start.add(duration, "second")`, `:347`), never wall clock.
//! - Windows from different firings may overlap when the duration exceeds the spacing.
//!   [`windows`] lists every firing; Kuma restarts its timer on each firing (`:250`), which
//!   makes the status the union.
//!
//! DST, for firings of the cron pattern (delegated to croner 4, which follows Vixie cron):
//! - A time in the spring-forward gap fires at the first valid minute after the gap, on the
//!   same calendar day (`30 2 * * *` fires at 03:00 on the day 02:00 jumps to 03:00).
//! - A time in the repeated fall-back hour fires once, at its first occurrence.
//! - Wildcard and step patterns (`*/15 * * * *`) fire in every real minute that matches.
//!
//! Dates in `start_date` and `end_date` resolve like dayjs: a repeated time is the first
//! occurrence, a skipped time is moved forward by the length of the gap.

mod definition;

use chrono::{
    DateTime, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta, TimeZone,
    Timelike, Utc,
};
use chrono_tz::Tz;
use croner::Cron;
use croner::errors::CronError;
use croner::parser::{CronParser, Year};
use serde::Serialize;
use std::collections::BTreeSet;

pub use definition::{DayOfMonth, Definition, Strategy, Timezone};

/// How many back-to-back windows `next_change` follows before it reports where it got to.
/// Only matters for patterns whose windows overlap forever, like `* * * * *` with a 5 minute
/// duration. The caller re-evaluates at the returned time anyway.
const MAX_CHAIN: usize = 1000;

/// Kuma's maintenance statuses (`:421-449`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Inactive,
    Scheduled,
    UnderMaintenance,
    Ended,
    /// Kuma's fallback when no timer state exists, which is what a bad cron or missing
    /// field leaves behind (`:225`, `:448`). See [`Definition::validate`].
    Unknown,
}

/// A span during which the monitors are under maintenance. A `single` maintenance without
/// start or end date uses `DateTime::MIN_UTC` / `MAX_UTC` for the missing side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Window {
    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.start <= at && at < self.end
    }

    fn clip(self, to: Window) -> Option<Window> {
        let window = Window {
            start: self.start.max(to.start),
            end: self.end.min(to.end),
        };
        (window.start < window.end).then_some(window)
    }
}

/// Why a definition produces no windows.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is required for this strategy")]
    Missing(&'static str),
    #[error("{0} has no usable entries")]
    Empty(&'static str),
    #[error("invalid cron expression: {0}")]
    Cron(#[from] CronError),
}

impl Definition {
    /// Checks the fields the strategy needs. A definition that fails here has no windows
    /// and reports [`Status::Unknown`] once its date range has started.
    pub fn validate(&self) -> Result<(), Error> {
        Schedule::new(self).plan.map(|_| ())
    }
}

/// Windows that end after `from` and start before `to`, ascending by start, with the date
/// range applied. Ignores the `active` flag so a preview can show an inactive maintenance.
/// `manual` and invalid definitions have none.
pub fn windows(
    definition: &Definition,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> impl Iterator<Item = Window> + use<> {
    let Schedule { range, plan, .. } = Schedule::new(definition);
    plan.ok()
        .into_iter()
        .flat_map(move |plan| plan.windows(range, from))
        .take_while(move |window| window.start < to)
}

/// The status Kuma would report at `at`. Checks run in Kuma's order (`:421-449`): inactive,
/// manual, before the start date, after the end date, single, then the window state.
pub fn status_at(definition: &Definition, at: DateTime<Utc>) -> Status {
    if !definition.active {
        return Status::Inactive;
    }
    let Schedule { range, plan, .. } = Schedule::new(definition);
    if matches!(plan, Ok(Plan::Manual)) {
        return Status::UnderMaintenance;
    }
    if at < range.start {
        return Status::Scheduled;
    }
    if at >= range.end {
        return Status::Ended;
    }
    match plan {
        Err(_) => Status::Unknown,
        Ok(Plan::Single | Plan::Manual) => Status::UnderMaintenance,
        Ok(plan) => {
            let inside = plan
                .windows(range, at)
                .take_while(|window| window.start <= at)
                .any(|window| window.contains(at));
            if inside {
                Status::UnderMaintenance
            } else {
                Status::Scheduled
            }
        }
    }
}

/// The next instant after `after` at which the status may change, so the server can sleep
/// until then. `None` when nothing changes any more: inactive, manual, or no later window
/// start or end and no end date. The status at the returned instant is the new one.
pub fn next_change(definition: &Definition, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if !definition.active {
        return None;
    }
    let Schedule { range, plan, .. } = Schedule::new(definition);
    let window_boundary = plan
        .ok()
        .and_then(|plan| first_boundary(&mut plan.windows(range, after), after));
    let range_end = Some(range.end).filter(|end| *end > after);
    [window_boundary, range_end]
        .into_iter()
        .flatten()
        .filter(|at| *at < DateTime::<Utc>::MAX_UTC)
        .min()
}

/// The first window edge after `after`: the start of the next window, or the end of the
/// run of overlapping windows that `after` sits in.
fn first_boundary(
    windows: &mut dyn Iterator<Item = Window>,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let first = windows.next()?;
    if first.start > after {
        return Some(first.start);
    }
    let mut end = first.end;
    for window in windows.take(MAX_CHAIN) {
        if window.start > end {
            break;
        }
        end = end.max(window.end);
    }
    Some(end)
}

struct Schedule {
    range: Window,
    plan: Result<Plan, Error>,
}

enum Plan {
    Manual,
    /// The date range is the window (`:233-239`, `:431-442`).
    Single,
    Recurring(Box<Recurring>),
}

/// Kuma's generated cron plus the length of each window.
struct Recurring {
    cron: Cron,
    tz: Tz,
    duration: TimeDelta,
    every: Option<EveryDays>,
}

/// `recurring-interval`: the cron fires daily and only every `days`-th day counts.
///
/// Kuma counts from the previous start (`:292-298`, minus 1.1 hours of slack for DST) and
/// keeps that in memory, so a restart changes the rhythm. Here the rhythm is fixed: day 0 is
/// the date of `start_date`, which is where Kuma's `startAt` begins (`:270-275`).
struct EveryDays {
    anchor: NaiveDate,
    days: u32,
}

impl EveryDays {
    fn counts(&self, date: NaiveDate) -> bool {
        let elapsed = (date - self.anchor).num_days();
        elapsed >= 0 && elapsed % i64::from(self.days) == 0
    }
}

impl Schedule {
    fn new(definition: &Definition) -> Self {
        let tz = definition.timezone.resolve(definition.server_timezone);
        let bound = |date: Option<NaiveDateTime>, unbounded| {
            date.map_or(unbounded, |date| local_to_utc(tz, date))
        };
        Self {
            range: Window {
                start: bound(definition.start_date, DateTime::<Utc>::MIN_UTC),
                end: bound(definition.end_date, DateTime::<Utc>::MAX_UTC),
            },
            plan: Plan::new(definition, tz),
        }
    }
}

impl Plan {
    fn new(definition: &Definition, tz: Tz) -> Result<Self, Error> {
        match definition.strategy {
            Strategy::Manual => Ok(Self::Manual),
            Strategy::Single => Ok(Self::Single),
            Strategy::Cron => {
                let expression = definition
                    .cron
                    .as_deref()
                    .filter(|cron| !cron.trim().is_empty())
                    .ok_or(Error::Missing("cron"))?;
                let seconds = definition.duration.ok_or(Error::Missing("duration"))?;
                Plan::recurring(expression, tz, TimeDelta::seconds(seconds.into()), None)
            }
            strategy => {
                let (start, end) = definition
                    .start_time
                    .zip(definition.end_time)
                    .ok_or(Error::Missing("start_time and end_time"))?;
                let (minute, hour) = (start.minute(), start.hour());
                let duration = wall_clock_duration(start, end);
                match strategy {
                    Strategy::RecurringInterval => {
                        let every = EveryDays {
                            anchor: definition
                                .start_date
                                .map_or(NaiveDate::default(), |d| d.date()),
                            days: definition.interval_day.unwrap_or(1).max(1),
                        };
                        let cron = format!("{minute} {hour} * * *");
                        Plan::recurring(&cron, tz, duration, Some(every))
                    }
                    Strategy::RecurringWeekday => {
                        let days: BTreeSet<_> = definition.weekdays.iter().collect();
                        if days.is_empty() {
                            return Err(Error::Empty("weekdays"));
                        }
                        let list = join(days);
                        Plan::recurring(&format!("{minute} {hour} * * {list}"), tz, duration, None)
                    }
                    _ => {
                        let mut days = BTreeSet::new();
                        let mut last = false;
                        for day in &definition.days_of_month {
                            match day {
                                DayOfMonth::Day(day) => {
                                    days.insert(*day);
                                }
                                DayOfMonth::LastDay(1) => last = true,
                                DayOfMonth::LastDay(_) => {}
                            }
                        }
                        let mut list = join(days);
                        if last {
                            list.push_str(if list.is_empty() { "L" } else { ",L" });
                        }
                        if list.is_empty() {
                            return Err(Error::Empty("days_of_month"));
                        }
                        Plan::recurring(&format!("{minute} {hour} {list} * *"), tz, duration, None)
                    }
                }
            }
        }
    }

    fn recurring(
        expression: &str,
        tz: Tz,
        duration: TimeDelta,
        every: Option<EveryDays>,
    ) -> Result<Self, Error> {
        // croner 8 (Kuma) takes 5 or 6 fields and no year; its default OR of day-of-month and
        // day-of-week is croner 4's default too.
        let cron = CronParser::builder()
            .year(Year::Disallowed)
            .sloppy_ranges(true)
            .build()
            .parse(expression)?;
        Ok(Self::Recurring(Box::new(Recurring {
            cron,
            tz,
            duration,
            every,
        })))
    }

    /// Windows ending after `from`, ascending, cut to `range`.
    fn windows(self, range: Window, from: DateTime<Utc>) -> Box<dyn Iterator<Item = Window>> {
        match self {
            Self::Manual => Box::new(std::iter::empty()),
            Self::Single => Box::new(
                Some(range)
                    .filter(|window| window.start < window.end && window.end > from)
                    .into_iter(),
            ),
            Self::Recurring(recurring) => {
                // A firing that started up to one duration ago can still be running
                // (Kuma's `getRunningTimeslot`, `:346-361`).
                let search_from = from.max(range.start);
                let search_from = search_from
                    .checked_sub_signed(recurring.duration)
                    .unwrap_or(DateTime::<Utc>::MIN_UTC);
                Box::new(
                    recurring
                        .firings_after(search_from)
                        .take_while(move |window| window.start < range.end)
                        .filter_map(move |window| window.clip(range))
                        .filter(move |window| window.end > from),
                )
            }
        }
    }
}

impl Recurring {
    /// Unclipped windows of the cron firings that start after `after`.
    fn firings_after(self, after: DateTime<Utc>) -> impl Iterator<Item = Window> {
        let mut cursor = after.with_timezone(&self.tz);
        std::iter::from_fn(move || {
            loop {
                // Feeding back croner's own result keeps a search inside the right half of a
                // repeated DST hour.
                let next = self.cron.find_next_occurrence(&cursor, false).ok()?;
                cursor = next;
                if let Some(every) = &self.every
                    && !every.counts(next.date_naive())
                {
                    continue;
                }
                let start = next.with_timezone(&Utc);
                return Some(Window {
                    start,
                    end: start + self.duration,
                });
            }
        })
    }
}

fn join<T: ToString>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Kuma's `calcDuration` (`:131-138`): end minus start on the clock, plus a day when
/// negative. Equal times give an empty window.
fn wall_clock_duration(start: NaiveTime, end: NaiveTime) -> TimeDelta {
    let duration = end - start;
    if duration < TimeDelta::zero() {
        duration + TimeDelta::days(1)
    } else {
        duration
    }
}

fn local_to_utc(tz: Tz, local: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(time) | LocalResult::Ambiguous(time, _) => time.with_timezone(&Utc),
        LocalResult::None => {
            let offset_before_gap = tz
                .from_local_datetime(&(local - TimeDelta::days(1)))
                .earliest()
                .map_or(0, |time| time.offset().fix().local_minus_utc());
            local.and_utc() - TimeDelta::seconds(offset_before_gap.into())
        }
    }
}
