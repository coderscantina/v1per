pub(crate) mod discord;
pub(crate) mod ntfy;
pub(crate) mod pushover;
pub(crate) mod slack;
pub(crate) mod smtp;
pub(crate) mod teams;
pub(crate) mod telegram;
pub(crate) mod webhook;

use serde_json::Value;

use crate::error::SendError;
use crate::secret::Secret;

/// Kuma's `{...JSON.parse(additionalHeaders)}`: string values as is, others as JSON text.
pub(crate) fn additional_headers(raw: Option<&Secret>) -> Result<Vec<(String, String)>, SendError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let parsed: Value = serde_json::from_str(raw.expose())
        .map_err(|_| SendError::Config("Additional Headers is not a valid JSON".into()))?;
    let Value::Object(headers) = parsed else {
        return Ok(Vec::new());
    };
    Ok(headers
        .into_iter()
        .map(|(name, value)| match value {
            Value::String(text) => (name, text),
            other => (name, other.to_string()),
        })
        .collect())
}

/// Kuma's `template?.trim() || ""`, as `None` when nothing is left.
pub(crate) fn trimmed(template: Option<&str>) -> Option<&str> {
    template.map(str::trim).filter(|t| !t.is_empty())
}
