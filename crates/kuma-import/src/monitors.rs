//! Kuma `monitor` rows to monitor kinds and checks.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde_json::Value as Json;
use url::Url;
use v1per_checks::{
    Check, CheckDefinition, DnsCheck, HttpAuth, HttpRequest, JsonQueryCheck, KeywordCheck,
    PingCheck, PortCheck, Secret, StatusCodeRange,
};

use crate::plan::{ManualStatus, MonitorKind};
use crate::record::Record;

/// Monitor columns that hold credentials.
const SECRET_COLUMNS: [&str; 12] = [
    "basic_auth_pass",
    "bearer_token",
    "oauth_client_secret",
    "tls_key",
    "headers",
    "mqtt_password",
    "radius_password",
    "radius_secret",
    "rabbitmq_password",
    "database_connection_string",
    "kafka_producer_sasl_options",
    "gamedig_token",
];

/// Credentials the import seals for one monitor: those its check carries, or for a monitor
/// imported paused, the credential columns of its inventoried row. Leftovers in columns its
/// type does not use are neither imported nor counted.
pub(crate) fn credential_count(record: &Record, kind: &MonitorKind, inventoried: bool) -> u64 {
    if inventoried {
        return SECRET_COLUMNS
            .iter()
            .filter(|column| holds_credential(record, column))
            .count() as u64;
    }
    let MonitorKind::Check(definition) = kind else {
        return 0;
    };
    let request = match &definition.check {
        Check::Http(request) => request,
        Check::Keyword(keyword) => &keyword.request,
        Check::JsonQuery(query) => &query.request,
        Check::Port(_) | Check::Ping(_) | Check::Dns(_) => return 0,
    };
    let auth = match &request.auth {
        HttpAuth::None => None,
        HttpAuth::Basic {
            basic_auth_pass: secret,
            ..
        }
        | HttpAuth::Ntlm {
            basic_auth_pass: secret,
            ..
        }
        | HttpAuth::Bearer {
            bearer_token: secret,
        }
        | HttpAuth::OAuth2ClientCredentials {
            oauth_client_secret: secret,
            ..
        }
        | HttpAuth::Mtls {
            tls_key: secret, ..
        } => Some(secret),
    };
    request.headers.len() as u64 + u64::from(auth.is_some_and(|secret| !secret.expose().is_empty()))
}

/// Kuma's form writes `{"mechanism":"None"}` and `{}` defaults; a JSON object only holds a
/// credential when some other key has a value.
fn holds_credential(record: &Record, column: &str) -> bool {
    let Some(value) = record.filled(column) else {
        return false;
    };
    match serde_json::from_str::<Json>(&value) {
        Ok(Json::Object(map)) => map.iter().any(|(key, value)| {
            key != "mechanism" && !matches!(value, Json::Null) && value.as_str() != Some("")
        }),
        _ => true,
    }
}

/// V1Per's interval bounds, which are Kuma's form limits too.
const INTERVAL: std::ops::RangeInclusive<i64> = 20..=2_073_600;

/// The timeout Kuma uses: the stored one, or 80 % of the interval when it is not positive,
/// capped at the interval.
pub(crate) fn timeout(record: &Record, interval: u32) -> f64 {
    let interval = f64::from(interval);
    match record.real("timeout") {
        Some(timeout) if timeout > 0.0 => timeout.min(interval),
        _ => interval * 0.8,
    }
}

pub(crate) fn interval(record: &Record) -> Result<u32, String> {
    let interval = record.int("interval").unwrap_or(20);
    if INTERVAL.contains(&interval) {
        Ok(interval as u32)
    } else {
        Err(format!(
            "interval {interval} s is outside {} to {} s",
            INTERVAL.start(),
            INTERVAL.end()
        ))
    }
}

/// Maps the type-specific columns. `Err` carries the reason the monitor imports paused.
pub(crate) fn kind(
    record: &Record,
    timeout: f64,
    upside_down: bool,
) -> Result<MonitorKind, String> {
    let kuma_type = record.text("type").unwrap_or_default();
    let check = |check| {
        Ok(MonitorKind::Check(Box::new(CheckDefinition {
            timeout,
            upside_down,
            check,
        })))
    };
    match kuma_type.as_str() {
        "http" => check(Check::Http(http_request(record)?)),
        "keyword" => check(Check::Keyword(KeywordCheck {
            request: http_request(record)?,
            keyword: record.text("keyword").unwrap_or_default(),
            invert_keyword: record.flag("invert_keyword"),
        })),
        "json-query" => {
            let json_path = record.text("json_path").unwrap_or_default();
            validate_json_path(&json_path)?;
            check(Check::JsonQuery(JsonQueryCheck {
                request: http_request(record)?,
                json_path,
                json_path_operator: choice(record, "json_path_operator")?.unwrap_or_default(),
                expected_value: record.text("expected_value").unwrap_or_default(),
            }))
        }
        "port" => check(Check::Port(PortCheck {
            hostname: required(record, "hostname")?,
            port: number(record, "port")?.ok_or("no port")?,
        })),
        "ping" => check(Check::Ping(PingCheck {
            hostname: required(record, "hostname")?,
            ping_count: number(record, "ping_count")?.unwrap_or(1),
            packet_size: number(record, "packet_size")?.unwrap_or(56),
            ping_per_request_timeout: record.real("ping_per_request_timeout").unwrap_or(2.0),
        })),
        "dns" => check(Check::Dns(DnsCheck {
            hostname: required(record, "hostname")?,
            dns_resolve_type: choice(record, "dns_resolve_type")?
                .unwrap_or(v1per_checks::DnsRecordType::A),
            dns_resolve_server: record
                .filled("dns_resolve_server")
                .unwrap_or_else(|| "1.1.1.1".to_owned()),
            port: number(record, "port")?.unwrap_or(53),
        })),
        "push" => Ok(MonitorKind::Push {
            token: record.filled("push_token").map(Secret::new),
        }),
        "group" => Ok(MonitorKind::Group),
        "manual" => Ok(MonitorKind::Manual {
            status: match record.int("manual_status") {
                None => None,
                Some(0) => Some(ManualStatus::Down),
                Some(1) => Some(ManualStatus::Up),
                Some(2) => Some(ManualStatus::Pending),
                Some(other) => {
                    return Err(format!("manual status {other} is not up, down or pending"));
                }
            },
        }),
        "" => Err("the monitor has no type".to_owned()),
        _ => Err("monitor type not supported by V1Per".to_owned()),
    }
}

/// Why a monitor that maps to a check cannot run as configured.
pub(crate) fn hold_reason(
    record: &Record,
    kind: &MonitorKind,
    active_proxies: &BTreeSet<i64>,
) -> Option<String> {
    let MonitorKind::Check(definition) = kind else {
        return None;
    };
    let request = match &definition.check {
        Check::Http(request) => Some(request),
        Check::Keyword(keyword) => Some(&keyword.request),
        Check::JsonQuery(query) => Some(&query.request),
        _ => None,
    };
    if let Some(request) = request {
        match request.auth {
            HttpAuth::Ntlm { .. } => {
                return Some("NTLM authentication is not supported".to_owned());
            }
            HttpAuth::Mtls { .. } => {
                return Some(
                    "client certificate (mTLS) authentication is not supported".to_owned(),
                );
            }
            _ => {}
        }
        // Kuma ignores a proxy that is switched off.
        if record
            .int("proxy_id")
            .is_some_and(|id| active_proxies.contains(&id))
        {
            return Some("uses a Kuma proxy, which V1Per does not support yet".to_owned());
        }
    }
    let conditions = record.filled("conditions");
    let has_conditions = conditions.is_some_and(|text| {
        serde_json::from_str::<Vec<Json>>(&text).map_or(true, |list| !list.is_empty())
    });
    has_conditions.then(|| "Kuma conditions are not supported".to_owned())
}

/// Parses a JSONata expression the way the json-query check will.
fn validate_json_path(json_path: &str) -> Result<(), String> {
    v1per_checks::validate_json_query(json_path)
        .map_err(|error| format!("JSON query does not parse: {error}"))
}

fn http_request(record: &Record) -> Result<HttpRequest, String> {
    let url = record.filled("url").ok_or("no URL")?;
    let url = Url::parse(url.trim()).map_err(|error| format!("URL does not parse: {error}"))?;
    Ok(HttpRequest {
        url,
        method: record.filled("method").unwrap_or_else(|| "GET".to_owned()),
        headers: headers(record)?,
        body: record.filled("body"),
        http_body_encoding: choice(record, "http_body_encoding")?.unwrap_or_default(),
        auth: auth(record)?,
        accepted_statuscodes: accepted_statuscodes(record)?,
        maxredirects: number(record, "maxredirects")?.unwrap_or(10),
        ignore_tls: record.flag("ignore_tls"),
        ip_family: choice(record, "ip_family")?,
        cache_bust: record.flag("cache_bust"),
    })
}

/// Kuma's `headers` JSON object. Values are secrets: some carry credentials.
fn headers(record: &Record) -> Result<BTreeMap<String, Secret>, String> {
    let Some(text) = record.filled("headers") else {
        return Ok(BTreeMap::new());
    };
    let Ok(Json::Object(map)) = serde_json::from_str::<Json>(&text) else {
        return Err("headers are not a JSON object".to_owned());
    };
    Ok(map
        .into_iter()
        .map(|(name, value)| {
            let value = match value {
                Json::String(text) => text,
                other => other.to_string(),
            };
            (name, Secret::new(value))
        })
        .collect())
}

fn auth(record: &Record) -> Result<HttpAuth, String> {
    let text = |column| record.text(column).unwrap_or_default();
    let secret = |column| Secret::new(text(column));
    Ok(match record.filled("auth_method").as_deref() {
        None => HttpAuth::None,
        Some("basic") => HttpAuth::Basic {
            basic_auth_user: text("basic_auth_user"),
            basic_auth_pass: secret("basic_auth_pass"),
        },
        Some("bearer") => HttpAuth::Bearer {
            bearer_token: secret("bearer_token"),
        },
        Some("oauth2-cc") => HttpAuth::OAuth2ClientCredentials {
            oauth_token_url: Url::parse(&required(record, "oauth_token_url")?)
                .map_err(|error| format!("OAuth token URL does not parse: {error}"))?,
            oauth_client_id: text("oauth_client_id"),
            oauth_client_secret: secret("oauth_client_secret"),
            oauth_scopes: record.filled("oauth_scopes"),
            oauth_audience: record.filled("oauth_audience"),
            oauth_auth_method: choice(record, "oauth_auth_method")?.unwrap_or_default(),
        },
        Some("ntlm") => HttpAuth::Ntlm {
            basic_auth_user: text("basic_auth_user"),
            basic_auth_pass: secret("basic_auth_pass"),
            auth_domain: record.filled("auth_domain"),
            auth_workstation: record.filled("auth_workstation"),
        },
        Some("mtls") => HttpAuth::Mtls {
            tls_cert: text("tls_cert"),
            tls_key: secret("tls_key"),
            tls_ca: record.filled("tls_ca"),
        },
        Some(other) => return Err(format!("authentication method `{other}` is not known")),
    })
}

fn accepted_statuscodes(record: &Record) -> Result<Vec<StatusCodeRange>, String> {
    let Some(text) = record.filled("accepted_statuscodes_json") else {
        return Ok(vec![StatusCodeRange { min: 200, max: 299 }]);
    };
    serde_json::from_str::<Vec<String>>(&text)
        .map_err(|_| "accepted status codes are not a JSON list of strings".to_owned())?
        .iter()
        .map(|code| code.parse())
        .collect()
}

fn required(record: &Record, column: &str) -> Result<String, String> {
    record
        .filled(column)
        .map(|text| text.trim().to_owned())
        .ok_or_else(|| format!("no {column}"))
}

fn number<T: TryFrom<i64>>(record: &Record, column: &str) -> Result<Option<T>, String> {
    record
        .int(column)
        .map(|n| T::try_from(n).map_err(|_| format!("{column} {n} is out of range")))
        .transpose()
}

/// One of the option strings a library type deserializes, such as `"ipv4"` or `"=="`.
fn choice<T: DeserializeOwned>(record: &Record, column: &str) -> Result<Option<T>, String> {
    record
        .filled(column)
        .map(|value| {
            // Kuma stores record types upper case and everything else lower case.
            let normalized = if column == "dns_resolve_type" {
                value.to_uppercase()
            } else {
                value.to_lowercase()
            };
            serde_json::from_value(Json::String(normalized))
                .map_err(|_| format!("{column} `{value}` is not known"))
        })
        .transpose()
}
