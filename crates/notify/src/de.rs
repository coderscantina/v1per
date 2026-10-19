//! Lenient field readers for Kuma's `notification.config` JSON.
//!
//! Kuma stores whatever its Vue form held, so a field may be missing, `null`, an empty string,
//! or a number where a string is expected. Kuma's providers read them with JavaScript
//! truthiness; these readers normalise that once so the senders can stay typed.

use std::str::FromStr;

use serde::de::Error;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::secret::Secret;

/// A non-empty string. Numbers become their decimal text; `null` and `""` become `None`.
pub fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null | Value::Bool(_)) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(other) => Err(D::Error::custom(format!(
            "expected a string, found {}",
            kind(&other)
        ))),
    }
}

pub fn secret<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Secret>, D::Error> {
    text(deserializer).map(|value| value.map(Secret::new))
}

/// JavaScript truthiness, so `"0"`-style leftovers behave as they did in Kuma.
pub fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    opt_flag(deserializer).map(|value| value.unwrap_or(false))
}

/// Like [`flag`], but keeps "never set" apart for Kuma's `?? true` defaults.
pub fn opt_flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(b),
        Some(Value::Number(n)) => Some(n.as_f64().is_some_and(|f| f != 0.0)),
        Some(Value::String(s)) => Some(!s.is_empty()),
        Some(Value::Array(_) | Value::Object(_)) => Some(true),
    })
}

/// A number stored as JSON number or numeric string; `null` and `""` become `None`.
pub fn number<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
{
    let Some(raw) = text(deserializer)? else {
        return Ok(None);
    };
    raw.trim()
        .parse()
        .map(Some)
        .map_err(|_| D::Error::custom(format!("expected a number, found {raw:?}")))
}

/// One of Kuma's option strings. Unknown values read as unset, which Kuma treats as its default.
pub fn choice<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
{
    Ok(text(deserializer)?.and_then(|value| value.parse().ok()))
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}
