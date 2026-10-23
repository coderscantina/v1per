//! Days and periods in the SLA timezone.
//!
//! Day and period edges follow the status page bars (decision 0008 item 11): a zone's UTC
//! offset counts in whole hours, truncated toward zero, so India +05:30 and Nepal +05:45 use
//! +05:00 and Newfoundland -03:30 uses -03:00. Within that rule an edge moves with DST.

use chrono::{DateTime, Datelike, Months, NaiveDate, NaiveTime, Offset, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;

/// The zone's offset at `at`, with its minutes dropped.
fn whole_hours(zone: Tz, at: DateTime<Utc>) -> TimeDelta {
    let offset = zone
        .offset_from_utc_datetime(&at.naive_utc())
        .fix()
        .local_minus_utc();
    // Integer division truncates toward zero, which drops the minutes on either side.
    TimeDelta::seconds(i64::from(offset / 3600 * 3600))
}

/// The local day `at` falls on in `zone`, as the status page bars count it.
pub fn local_day(zone: Tz, at: DateTime<Utc>) -> NaiveDate {
    (at + whole_hours(zone, at)).date_naive()
}

/// The first instant whose [`local_day`] is `day`.
pub fn day_start(zone: Tz, day: NaiveDate) -> DateTime<Utc> {
    let midnight = day.and_time(NaiveTime::MIN).and_utc();
    // The offsets a day before and after cover any transition near this midnight.
    let candidates = [midnight - TimeDelta::days(1), midnight + TimeDelta::days(1)].map(|probe| {
        let offset = whole_hours(zone, probe);
        (midnight - offset, offset)
    });
    candidates
        .iter()
        .filter(|(at, offset)| whole_hours(zone, *at) == *offset)
        .map(|(at, _)| *at)
        .min()
        // No candidate holds when a transition skips midnight; the day starts at the jump.
        .unwrap_or_else(|| {
            candidates
                .iter()
                .map(|(at, _)| *at)
                .max()
                .expect("two candidates")
        })
}

/// A calendar period length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeriodKind {
    Month,
    Quarter,
    Year,
}

impl PeriodKind {
    /// The first day of the calendar period containing `day`.
    fn first_day(self, day: NaiveDate) -> NaiveDate {
        let month = match self {
            Self::Month => day.month(),
            Self::Quarter => day.month0() / 3 * 3 + 1,
            Self::Year => 1,
        };
        NaiveDate::from_ymd_opt(day.year(), month, 1).expect("first of a month exists")
    }

    fn months(self) -> Months {
        Months::new(match self {
            Self::Month => 1,
            Self::Quarter => 3,
            Self::Year => 12,
        })
    }
}

/// One SLA period: local days `[first_day, end_day)` and the instants `[starts_at, ends_at)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Period {
    /// The calendar first day, or the SLA start date for the period it falls in.
    pub first_day: NaiveDate,
    /// The first day of the next period.
    pub end_day: NaiveDate,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
}

impl Period {
    /// Real elapsed time from start to end, DST and the edge rule included.
    pub fn length(&self) -> TimeDelta {
        self.ends_at - self.starts_at
    }

    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        (self.starts_at..self.ends_at).contains(&at)
    }
}

/// The periods of one SLA: a kind in a timezone, from the contract start date on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodCalendar {
    pub kind: PeriodKind,
    pub zone: Tz,
    /// The contract start. The period it falls in begins on this day, not the calendar one.
    pub start: NaiveDate,
}

impl PeriodCalendar {
    /// The period containing `at`, or `None` before the start date.
    pub fn containing(&self, at: DateTime<Utc>) -> Option<Period> {
        let day = local_day(self.zone, at);
        (day >= self.start).then(|| self.period(self.kind.first_day(day)))
    }

    /// Every period from the start date through the one containing `at`, oldest first.
    /// The last one is still open unless `at` is its end.
    pub fn through(&self, at: DateTime<Utc>) -> Vec<Period> {
        let last = local_day(self.zone, at);
        let mut first = self.kind.first_day(self.start);
        let mut periods = Vec::new();
        while first <= last {
            let period = self.period(first);
            first = period.end_day;
            periods.push(period);
        }
        periods
    }

    fn period(&self, calendar_first: NaiveDate) -> Period {
        let first_day = calendar_first.max(self.start);
        let end_day = calendar_first + self.kind.months();
        Period {
            first_day,
            end_day,
            starts_at: day_start(self.zone, first_day),
            ends_at: day_start(self.zone, end_day),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::{Asia::Kolkata, Europe::Vienna};

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().expect("valid instant")
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    #[test]
    fn non_hour_offsets_drop_their_minutes() {
        // 00:15 on the 23rd in real IST, but the bars use +05:00, which makes it 23:15.
        assert_eq!(
            local_day(Kolkata, utc("2026-10-22T18:45:00Z")),
            date(2026, 10, 22)
        );
        assert_eq!(
            local_day(Kolkata, utc("2026-10-22T19:00:00Z")),
            date(2026, 10, 23)
        );
        assert_eq!(
            day_start(Kolkata, date(2026, 10, 23)),
            utc("2026-10-22T19:00:00Z")
        );
    }

    #[test]
    fn day_start_follows_dst() {
        assert_eq!(
            day_start(Vienna, date(2026, 3, 29)),
            utc("2026-03-28T23:00:00Z")
        );
        assert_eq!(
            day_start(Vienna, date(2026, 3, 30)),
            utc("2026-03-29T22:00:00Z")
        );
    }

    #[test]
    fn quarter_boundary_uses_local_midnight() {
        let calendar = PeriodCalendar {
            kind: PeriodKind::Quarter,
            zone: Vienna,
            start: date(2026, 1, 1),
        };
        // 00:30 on October 1st in Vienna.
        let period = calendar
            .containing(utc("2026-09-30T22:30:00Z"))
            .expect("after start");
        assert_eq!(
            (period.first_day, period.end_day),
            (date(2026, 10, 1), date(2027, 1, 1))
        );
        assert_eq!(period.starts_at, utc("2026-09-30T22:00:00Z"));
        assert_eq!(period.ends_at, utc("2026-12-31T23:00:00Z"));
        // The autumn DST change adds an hour.
        assert_eq!(period.length(), TimeDelta::days(92) + TimeDelta::hours(1));
    }

    #[test]
    fn year_periods_start_at_the_contract_date() {
        let calendar = PeriodCalendar {
            kind: PeriodKind::Year,
            zone: Vienna,
            start: date(2025, 6, 15),
        };
        // 01:00 on January 1st 2027 in Vienna.
        let periods = calendar.through(utc("2027-01-01T00:00:00Z"));
        let days: Vec<_> = periods.iter().map(|p| (p.first_day, p.end_day)).collect();
        assert_eq!(
            days,
            [
                (date(2025, 6, 15), date(2026, 1, 1)),
                (date(2026, 1, 1), date(2027, 1, 1)),
                (date(2027, 1, 1), date(2028, 1, 1)),
            ]
        );
        assert_eq!(periods[0].starts_at, utc("2025-06-14T22:00:00Z"));
        assert_eq!(periods[1].ends_at, periods[2].starts_at);
    }

    #[test]
    fn nothing_before_the_start_date() {
        let calendar = PeriodCalendar {
            kind: PeriodKind::Month,
            zone: Vienna,
            start: date(2026, 11, 1),
        };
        assert_eq!(calendar.containing(utc("2026-10-23T12:00:00Z")), None);
        assert!(calendar.through(utc("2026-10-23T12:00:00Z")).is_empty());
    }
}
