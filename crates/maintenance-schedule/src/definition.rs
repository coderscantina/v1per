use chrono::{NaiveDateTime, NaiveTime};
use chrono_tz::Tz;
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};

/// Kuma's `maintenance.strategy` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    Manual,
    Single,
    RecurringInterval,
    RecurringWeekday,
    RecurringDayOfMonth,
    Cron,
}

/// Kuma's `maintenance.timezone` column. An empty value and `SAME_AS_SERVER` both mean the
/// server timezone (`maintenance.js:402-407`); the migration default `UTC` is a plain name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Timezone {
    #[default]
    SameAsServer,
    Named(Tz),
}

impl Timezone {
    pub(crate) fn resolve(self, server: Tz) -> Tz {
        match self {
            Self::SameAsServer => server,
            Self::Named(tz) => tz,
        }
    }
}

impl<'de> Deserialize<'de> for Timezone {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Option::<String>::deserialize(deserializer)?.as_deref() {
            None | Some("" | "SAME_AS_SERVER") => Ok(Self::SameAsServer),
            Some(name) => name.parse().map(Self::Named).map_err(D::Error::custom),
        }
    }
}

/// One entry of Kuma's `days_of_month` JSON list: a day number, or `lastDay1` to `lastDay4`.
/// Kuma only turns `lastDay1` into a cron `L`; `lastDay2` to `lastDay4` are silently dropped
/// (`maintenance.js:486-493`) and so are here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayOfMonth {
    Day(u8),
    LastDay(u8),
}

impl<'de> Deserialize<'de> for DayOfMonth {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Number(u8),
            Text(String),
        }
        match Stored::deserialize(deserializer)? {
            Stored::Number(day) => Ok(Self::Day(day)),
            Stored::Text(text) => match text.strip_prefix("lastDay").map(str::parse) {
                Some(Ok(n @ 1..=4)) => Ok(Self::LastDay(n)),
                _ => text
                    .parse()
                    .map(Self::Day)
                    .map_err(|_| D::Error::custom(format!("invalid day of month {text:?}"))),
            },
        }
    }
}

/// A maintenance as Kuma stores it, one field per `maintenance` column that affects timing.
///
/// Mapping from a Kuma row (field names equal column names):
/// - `strategy`, `timezone`, `interval_day`, `cron`, `duration` (seconds), `active` map directly.
/// - `start_date` and `end_date` are naive local datetimes in the maintenance timezone, as
///   `YYYY-MM-DD HH:mm[:ss]` or with a `T`; `null` means unbounded.
/// - `start_time` and `end_time` are `HH:mm` (a seconds part is ignored, as Kuma does).
/// - `weekdays` and `days_of_month` are the stored JSON text (or an already parsed array);
///   `null`, an empty string and `[]` are all an empty list.
/// - `server_timezone` is not a column: set it from the server's timezone setting. It only
///   matters when `timezone` is `SameAsServer`.
#[derive(Debug, Clone, Deserialize)]
pub struct Definition {
    pub strategy: Strategy,
    pub active: bool,
    #[serde(default, deserialize_with = "local_datetime")]
    pub start_date: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "local_datetime")]
    pub end_date: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "time_of_day")]
    pub start_time: Option<NaiveTime>,
    #[serde(default, deserialize_with = "time_of_day")]
    pub end_time: Option<NaiveTime>,
    /// 0 or 7 is Sunday, 1 is Monday.
    #[serde(default, deserialize_with = "stored_list")]
    pub weekdays: Vec<u8>,
    #[serde(default, deserialize_with = "stored_list")]
    pub days_of_month: Vec<DayOfMonth>,
    #[serde(default)]
    pub interval_day: Option<u32>,
    #[serde(default)]
    pub cron: Option<String>,
    /// Window length in seconds. Used by `cron`; the recurring strategies derive it from
    /// `start_time` and `end_time` like Kuma's `generateCron`.
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub timezone: Timezone,
    #[serde(skip, default = "utc")]
    pub server_timezone: Tz,
}

fn utc() -> Tz {
    Tz::UTC
}

fn stored_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored<T> {
        List(Vec<T>),
        Text(String),
    }
    match Option::<Stored<T>>::deserialize(deserializer)? {
        None => Ok(Vec::new()),
        Some(Stored::List(list)) => Ok(list),
        Some(Stored::Text(text)) if text.trim().is_empty() => Ok(Vec::new()),
        Some(Stored::Text(text)) => serde_json::from_str(&text).map_err(D::Error::custom),
    }
}

fn local_datetime<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NaiveDateTime>, D::Error> {
    const FORMATS: [&str; 4] = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ];
    let Some(text) = Option::<String>::deserialize(deserializer)?.filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(&text, format).ok())
        .map(Some)
        .ok_or_else(|| D::Error::custom(format!("invalid datetime {text:?}")))
}

fn time_of_day<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<NaiveTime>, D::Error> {
    let Some(text) = Option::<String>::deserialize(deserializer)?.filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let mut parts = text.split(':');
    let hour_minute = parts.next().zip(parts.next()).and_then(|(h, m)| {
        NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)
    });
    hour_minute
        .map(Some)
        .ok_or_else(|| D::Error::custom(format!("invalid time {text:?}")))
}
