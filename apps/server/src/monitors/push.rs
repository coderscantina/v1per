//! `ALL /api/push/{token}`, as Uptime Kuma 2.5.0 answers it (`server/routers/api-router.js`).
//!
//! Query parameters only, whatever the method: `status` is up when absent, empty or `up` and
//! down otherwise; `msg` defaults to `OK`; `ping` is read like JavaScript's `parseFloat`, and
//! 0 or unreadable means none. Success is `200 {"ok":true}`; Kuma's refusals are
//! `404 {"ok":false,"msg":...}`. Kuma has no limits: here a token takes 120 pushes a minute and
//! an address 30 unknown tokens a minute, then `429`. A push that could not be recorded is
//! `503`, so senders retry.

use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::evaluator::{Push, PushRefusal};
use crate::http::client_address;
use crate::identity::token_digest;
use crate::state::AppState;

/// Kuma's bound on `ping`, in milliseconds (about 3.17 years).
const MAX_PING_MS: f64 = 100_000_000_000.0;

pub async fn push(
    State(state): State<AppState>,
    Path(token): Path<String>,
    request: Request,
) -> Response {
    let (parts, _) = request.into_parts();
    let address = client_address(&parts);
    if state.push_misses.exhausted(&address) || state.push_tokens.check(&token).is_err() {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "Too many requests.");
    }
    let mut status = Vec::new();
    let mut msg = Vec::new();
    let mut ping = Vec::new();
    for (name, value) in form_urlencoded::parse(parts.uri.query().unwrap_or_default().as_bytes()) {
        match name.as_ref() {
            "status" => status.push(value),
            "msg" => msg.push(value),
            "ping" => ping.push(value),
            _ => {}
        }
    }
    // A repeated parameter is an array in Express: never "up", and joined as text.
    let up = match status.as_slice() {
        [] => true,
        [one] => one.is_empty() || one == "up",
        _ => false,
    };
    let message = match msg.join(",") {
        text if text.is_empty() => "OK".to_owned(),
        text => text,
    };
    let ping = ping
        .first()
        .map(|value| parse_float(value))
        .filter(|ping| *ping != 0.0 && !ping.is_nan());
    if let Some(ping) = ping
        && !(0.0..=MAX_PING_MS).contains(&ping)
    {
        return refuse(
            StatusCode::NOT_FOUND,
            "Invalid ping value. Must be between 0 and 100000000000 ms.",
        );
    }
    let push = Push {
        up,
        message,
        latency_ms: ping.map(|ms| ms.round().min(f64::from(u32::MAX)) as u32),
    };
    let at = state.clock.now_ms();
    match state.evaluator.push(token_digest(&token), push, at).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(PushRefusal::NotFound) => {
            let _ = state.push_misses.check(&address);
            refuse(StatusCode::NOT_FOUND, "Monitor not found or not active.")
        }
        Err(PushRefusal::Unavailable) => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "The push could not be recorded. Try again.",
        ),
    }
}

fn refuse(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "ok": false, "msg": msg }))).into_response()
}

/// JavaScript's `parseFloat`: the longest leading decimal number after whitespace, or NaN.
fn parse_float(text: &str) -> f64 {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        from + bytes[from.min(bytes.len())..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let signed = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    if text[signed..].starts_with("Infinity") {
        return if bytes[0] == b'-' {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let mut end = digits(signed);
    let mut number = end > signed;
    if bytes.get(end) == Some(&b'.') {
        let fraction = digits(end + 1);
        if fraction > end + 1 || number {
            end = fraction;
            number = true;
        }
    }
    if !number {
        return f64::NAN;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + sign);
        if exponent > end + 1 + sign {
            end = exponent;
        }
    }
    text[..end].parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::parse_float;

    #[test]
    fn ping_reads_like_javascript_parse_float() {
        assert_eq!(parse_float("12"), 12.0);
        assert_eq!(parse_float(" 12.5ms"), 12.5);
        assert_eq!(parse_float(".5"), 0.5);
        assert_eq!(parse_float("-3"), -3.0);
        assert_eq!(parse_float("1e3x"), 1000.0);
        assert_eq!(parse_float("2e"), 2.0);
        assert_eq!(parse_float("Infinity"), f64::INFINITY);
        assert!(parse_float("abc").is_nan());
        assert!(parse_float(".").is_nan());
        assert!(parse_float("").is_nan());
    }
}
