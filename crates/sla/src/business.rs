//! Business time for response and resolution clocks.
//!
//! Hours are real wall clock in the SLA timezone; the whole-hour rule for day edges does not
//! apply to them. On DST days a range covers the real time between its wall clock edges:
//! a start or end inside the spring-forward gap is the instant the gap ends, a repeated
//! start is its first occurrence and a repeated end its second.

use chrono::{
    DateTime, Datelike, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta,
    TimeZone, Timelike, Utc, Weekday,
};
use chrono_tz::Tz;
use std::collections::BTreeSet;

const DAY_SECONDS: u32 = 86_400;

/// Open hours on one weekday. An `end` of 00:00 means midnight at the end of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WeeklyRange {
    pub weekday: Weekday,
    pub start: NaiveTime,
    pub end: NaiveTime,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("business hours on {0:?} end before they start")]
    EmptyRange(Weekday),
    #[error("business hours need at least one range; use Clock::AllHours for 24/7")]
    NoHours,
}

/// A weekly schedule and holiday dates in one timezone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessHours {
    zone: Tz,
    /// Merged, sorted `[start, end)` seconds of the day, indexed from Monday.
    days: [Vec<(u32, u32)>; 7],
    holidays: BTreeSet<NaiveDate>,
}

impl BusinessHours {
    /// Several ranges per weekday are allowed; overlapping or touching ones merge.
    pub fn new(
        zone: Tz,
        ranges: impl IntoIterator<Item = WeeklyRange>,
        holidays: impl IntoIterator<Item = NaiveDate>,
    ) -> Result<Self, ScheduleError> {
        let mut days: [Vec<(u32, u32)>; 7] = Default::default();
        for range in ranges {
            let start = range.start.num_seconds_from_midnight();
            let end = match range.end.num_seconds_from_midnight() {
                0 => DAY_SECONDS,
                end => end,
            };
            if end <= start {
                return Err(ScheduleError::EmptyRange(range.weekday));
            }
            days[range.weekday.num_days_from_monday() as usize].push((start, end));
        }
        if days.iter().all(Vec::is_empty) {
            return Err(ScheduleError::NoHours);
        }
        for ranges in &mut days {
            ranges.sort_unstable();
            let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
            for &(start, end) in ranges.iter() {
                match merged.last_mut() {
                    Some(last) if start <= last.1 => last.1 = last.1.max(end),
                    _ => merged.push((start, end)),
                }
            }
            *ranges = merged;
        }
        Ok(Self {
            zone,
            days,
            holidays: holidays.into_iter().collect(),
        })
    }

    /// The open intervals of one local date, in UTC.
    fn intervals(&self, day: NaiveDate) -> impl Iterator<Item = (DateTime<Utc>, DateTime<Utc>)> {
        let ranges: &[(u32, u32)] = if self.holidays.contains(&day) {
            &[]
        } else {
            &self.days[day.weekday().num_days_from_monday() as usize]
        };
        ranges.iter().filter_map(move |&(start, end)| {
            let start = self.instant(at_second(day, start), Repeat::First);
            let end = self.instant(at_second(day, end), Repeat::Second);
            (start < end).then_some((start, end))
        })
    }

    /// The UTC instant of a wall clock time, by the rules in the module docs.
    fn instant(&self, local: NaiveDateTime, repeat: Repeat) -> DateTime<Utc> {
        match self.zone.from_local_datetime(&local) {
            LocalResult::Single(at) => at.to_utc(),
            LocalResult::Ambiguous(first, second) => match repeat {
                Repeat::First => first.to_utc(),
                Repeat::Second => second.to_utc(),
            },
            LocalResult::None => self.gap_end(local),
        }
    }

    /// The transition instant of the gap that skips `local`.
    fn gap_end(&self, local: NaiveDateTime) -> DateTime<Utc> {
        let offset = |at: DateTime<Utc>| self.zone.offset_from_utc_datetime(&at.naive_utc()).fix();
        let probe = local.and_utc();
        let before = offset(probe - TimeDelta::days(1));
        let after = offset(probe + TimeDelta::days(1));
        // `local` read with the later offset lies before the jump, with the earlier one after.
        let mut low = probe - TimeDelta::seconds(after.local_minus_utc().into());
        let mut high = probe - TimeDelta::seconds(before.local_minus_utc().into());
        while high - low > TimeDelta::seconds(1) {
            let middle = low + (high - low) / 2;
            if offset(middle) == after {
                high = middle;
            } else {
                low = middle;
            }
        }
        high
    }

    fn local_date(&self, at: DateTime<Utc>) -> NaiveDate {
        at.with_timezone(&self.zone).date_naive()
    }
}

#[derive(Clone, Copy)]
enum Repeat {
    First,
    Second,
}

fn at_second(day: NaiveDate, second: u32) -> NaiveDateTime {
    if second == DAY_SECONDS {
        let next = day.succ_opt().expect("date in range");
        return next.and_time(NaiveTime::MIN);
    }
    let time = NaiveTime::from_num_seconds_from_midnight_opt(second, 0).expect("within a day");
    day.and_time(time)
}

/// How response and resolution time is counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Clock {
    /// Plain elapsed time, for SLAs without business hours.
    #[default]
    AllHours,
    Business(BusinessHours),
}

impl Clock {
    /// Counted time in `[from, to)`. Zero when `to` is not after `from`.
    pub fn duration(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> TimeDelta {
        if to <= from {
            return TimeDelta::zero();
        }
        let Self::Business(hours) = self else {
            return to - from;
        };
        let mut total = TimeDelta::zero();
        // A day earlier covers a range that ends at midnight in a repeated hour.
        let mut day = hours.local_date(from).pred_opt().expect("date in range");
        let last = hours.local_date(to);
        while day <= last {
            for (start, end) in hours.intervals(day) {
                let (start, end) = (start.max(from), end.min(to));
                if start < end {
                    total += end - start;
                }
            }
            day = day.succ_opt().expect("date in range");
        }
        total
    }

    /// The instant at which `target` counted time has passed since `from`.
    pub fn deadline(&self, from: DateTime<Utc>, target: TimeDelta) -> DateTime<Utc> {
        if target <= TimeDelta::zero() {
            return from;
        }
        let Self::Business(hours) = self else {
            return from + target;
        };
        let mut remaining = target;
        let mut day = hours.local_date(from).pred_opt().expect("date in range");
        // Ends: the schedule has at least one range and holidays are finite.
        loop {
            for (start, end) in hours.intervals(day) {
                let start = start.max(from);
                if end <= start {
                    continue;
                }
                if remaining <= end - start {
                    return start + remaining;
                }
                remaining -= end - start;
            }
            day = day.succ_opt().expect("date in range");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Vienna;

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().expect("valid instant")
    }

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
    }

    fn range(weekday: Weekday, start: u32, end: u32) -> WeeklyRange {
        WeeklyRange {
            weekday,
            start: time(start, 0),
            end: time(end, 0),
        }
    }

    fn office(holidays: &[NaiveDate]) -> Clock {
        use Weekday::*;
        let ranges = [Mon, Tue, Wed, Thu, Fri].map(|day| range(day, 9, 17));
        Clock::Business(
            BusinessHours::new(Vienna, ranges, holidays.iter().copied()).expect("valid"),
        )
    }

    fn sunday(start: u32, end: u32) -> Clock {
        Clock::Business(
            BusinessHours::new(Vienna, [range(Weekday::Sun, start, end)], []).expect("valid"),
        )
    }

    #[test]
    fn all_hours_is_plain_time() {
        let from = utc("2026-10-23T10:00:00Z");
        assert_eq!(
            Clock::AllHours.duration(from, from + TimeDelta::hours(30)),
            TimeDelta::hours(30)
        );
        assert_eq!(
            Clock::AllHours.deadline(from, TimeDelta::hours(4)),
            from + TimeDelta::hours(4)
        );
    }

    #[test]
    fn counts_only_open_hours() {
        // Friday 16:00 to Monday 10:00 in Vienna (CEST): one hour each side of the weekend.
        let from = utc("2026-10-23T14:00:00Z");
        let to = utc("2026-10-26T09:00:00Z");
        assert_eq!(office(&[]).duration(from, to), TimeDelta::hours(2));
    }

    #[test]
    fn deadline_carries_over_the_weekend() {
        // Friday 16:00 CEST plus two hours is Monday 10:00 CET, after the autumn change.
        let deadline = office(&[]).deadline(utc("2026-10-23T14:00:00Z"), TimeDelta::hours(2));
        assert_eq!(deadline, utc("2026-10-26T09:00:00Z"));
    }

    #[test]
    fn holidays_are_closed() {
        let monday = NaiveDate::from_ymd_opt(2026, 10, 26).expect("valid date");
        let clock = office(&[monday]);
        let deadline = clock.deadline(utc("2026-10-23T14:00:00Z"), TimeDelta::hours(2));
        assert_eq!(deadline, utc("2026-10-27T09:00:00Z"));
    }

    #[test]
    fn spring_forward_shortens_the_day() {
        // 2026-03-29 jumps from 02:00 to 03:00, so 01:00 to 04:00 is two real hours.
        let day = (utc("2026-03-28T12:00:00Z"), utc("2026-03-30T00:00:00Z"));
        assert_eq!(sunday(1, 4).duration(day.0, day.1), TimeDelta::hours(2));
        // A range starting inside the gap opens when the gap ends.
        assert_eq!(sunday(2, 4).duration(day.0, day.1), TimeDelta::hours(1));
        assert_eq!(
            sunday(2, 4).deadline(day.0, TimeDelta::minutes(30)),
            utc("2026-03-29T01:30:00Z")
        );
    }

    #[test]
    fn fall_back_lengthens_the_day() {
        // 2026-10-25 repeats 02:00 to 03:00, so 01:00 to 04:00 is four real hours.
        let day = (utc("2026-10-24T12:00:00Z"), utc("2026-10-26T00:00:00Z"));
        assert_eq!(sunday(1, 4).duration(day.0, day.1), TimeDelta::hours(4));
        // A range starting in the repeated hour counts from its first occurrence.
        assert_eq!(sunday(2, 3).duration(day.0, day.1), TimeDelta::hours(2));
    }

    #[test]
    fn year_long_range_on_a_sparse_schedule() {
        let clock = Clock::Business(
            BusinessHours::new(Vienna, [range(Weekday::Mon, 9, 10)], []).expect("valid"),
        );
        let from = utc("2026-01-01T00:00:00Z");
        // 2026 has 52 Mondays.
        assert_eq!(
            clock.duration(from, utc("2027-01-01T00:00:00Z")),
            TimeDelta::hours(52)
        );
        assert_eq!(
            clock.deadline(from, TimeDelta::hours(52)),
            utc("2026-12-28T09:00:00Z")
        );
    }

    #[test]
    fn ranges_must_be_open() {
        let inverted = range(Weekday::Mon, 17, 9);
        assert_eq!(
            BusinessHours::new(Vienna, [inverted], []),
            Err(ScheduleError::EmptyRange(Weekday::Mon))
        );
        assert_eq!(
            BusinessHours::new(Vienna, [], []),
            Err(ScheduleError::NoHours)
        );
        // 00:00 to 00:00 is the whole day.
        assert!(BusinessHours::new(Vienna, [range(Weekday::Mon, 0, 0)], []).is_ok());
    }
}
