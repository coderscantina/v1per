//! What a person sends for a monitor: validation, sealing of secret fields (decision 0006) and
//! the check definition a probe runs from the stored settings.
//!
//! Stored settings are the protocol's `MonitorSettings` JSON. Secret values never reach it:
//! `SecretField::value` does not serialize, and the push token is taken out before storing.
//! Each secret is sealed in `monitor_secrets` under the context `monitor:<id>:<field>`.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::http::{HeaderName, HeaderValue, Method};
use sqlx::SqliteConnection;
use url::Url;
use v1per_checks as checks;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{MonitorId, ProbeId};
use v1per_protocol::limits::{
    MONITOR_DESCRIPTION_MAX_CHARS, MONITOR_HEADERS_MAX, MONITOR_INTERVAL_MAX_SECONDS,
    MONITOR_INTERVAL_MIN_SECONDS, MONITOR_PROBES_MAX, MONITOR_RETRIES_MAX, MONITOR_TAG_MAX_CHARS,
    MONITOR_TAGS_MAX, NAME_MAX_CHARS, PUSH_TOKEN_MAX_CHARS, PUSH_TOKEN_MIN_CHARS,
};
use v1per_protocol::operations::monitors::{
    DnsRecordType, HttpAuth, HttpBodyEncoding, HttpSettings, IpFamily, JsonPathOperator,
    MonitorInput, MonitorSettings, OAuthClientAuth, Quorum, SecretField,
};

use crate::error::{ApiError, FieldErrors};
use crate::identity::secrets::SecretKey;
use crate::identity::token_digest;

/// The sealed push token's field in `monitor_secrets`.
const PUSH_TOKEN: &str = "push_token";
/// Kuma's push token length and alphabet.
const PUSH_TOKEN_CHARS: usize = 32;
const PUSH_TOKEN_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const MAX_REDIRECTS: u32 = 100;
const PING_COUNT_MAX: u16 = 100;
const PING_PACKET_MAX: u16 = 65_500;
const PING_REQUEST_TIMEOUT_MAX: f64 = 60.0;
const HOSTNAME_MAX_CHARS: usize = 255;

/// A monitor input that passed validation. Secret values are still in `settings`.
pub struct Validated {
    pub name: String,
    pub description: String,
    pub settings: MonitorSettings,
    pub interval: u32,
    pub retry_interval: u32,
    pub max_retries: u32,
    pub resend_interval: u32,
    pub timeout: f64,
    pub upside_down: bool,
    pub quorum: Quorum,
    pub tags: Vec<String>,
    /// As sent; the caller checks them against the tree.
    pub probes: Vec<ProbeId>,
}

/// Checks every field and collects every failure in one `invalid_arguments`. A secret field
/// without a value is required unless `stored` holds it. Group membership is checked by the
/// caller, which knows the tree.
pub fn validate(input: MonitorInput, stored: &HashSet<String>) -> Result<Validated, ApiError> {
    let mut errors = FieldErrors::default();
    let name = text(&mut errors, "name", &input.name, true, NAME_MAX_CHARS);
    let description = text(
        &mut errors,
        "description",
        &input.description,
        false,
        MONITOR_DESCRIPTION_MAX_CHARS,
    );
    let interval = input.interval;
    let retry_interval = input.retry_interval.unwrap_or(interval);
    for (field, seconds) in [("interval", interval), ("retry_interval", retry_interval)] {
        if seconds < MONITOR_INTERVAL_MIN_SECONDS {
            errors.add(
                field,
                FieldErrorCode::InvalidValue,
                Some(MONITOR_INTERVAL_MIN_SECONDS),
            );
        } else if seconds > MONITOR_INTERVAL_MAX_SECONDS {
            errors.add(
                field,
                FieldErrorCode::InvalidValue,
                Some(MONITOR_INTERVAL_MAX_SECONDS),
            );
        }
    }
    for (field, count) in [
        ("max_retries", input.max_retries),
        ("resend_interval", input.resend_interval),
    ] {
        if count > MONITOR_RETRIES_MAX {
            errors.add(
                field,
                FieldErrorCode::InvalidValue,
                Some(MONITOR_RETRIES_MAX),
            );
        }
    }
    // Kuma's default when none is given.
    let timeout = input.timeout.unwrap_or(f64::from(interval) * 0.8);
    if !(timeout > 0.0 && timeout <= f64::from(interval)) {
        errors.add("timeout", FieldErrorCode::InvalidValue, None);
    }
    let tags = tags(&mut errors, &input.tags);
    if input.probes.len() > MONITOR_PROBES_MAX {
        errors.add(
            "probes",
            FieldErrorCode::TooMany,
            Some(MONITOR_PROBES_MAX as u32),
        );
    }
    let mut settings = input.settings;
    check_settings(&mut errors, &settings);
    for slot in secret_slots(&mut settings) {
        match (&slot.secret.value, slot.secret.clear) {
            (Some(_), true) => errors.add(slot.path, FieldErrorCode::InvalidValue, None),
            (None, false) if !stored.contains(&slot.field) => {
                errors.add(slot.path, FieldErrorCode::Required, None)
            }
            _ => {}
        }
    }
    errors.finish()?;
    Ok(Validated {
        name,
        description,
        settings,
        interval,
        retry_interval,
        max_retries: input.max_retries,
        resend_interval: input.resend_interval,
        timeout,
        upside_down: input.upside_down,
        quorum: input.quorum,
        tags,
        probes: input.probes,
    })
}

fn text(errors: &mut FieldErrors, field: &str, value: &str, required: bool, max: usize) -> String {
    let value = value.trim();
    let chars = value.chars().count();
    if required && chars == 0 {
        errors.add(field, FieldErrorCode::Required, None);
    } else if chars > max {
        errors.add(field, FieldErrorCode::TooLong, Some(max as u32));
    }
    value.to_owned()
}

/// Trimmed, without empty or repeated tags.
fn tags(errors: &mut FieldErrors, tags: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for tag in tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        if tag.chars().count() > MONITOR_TAG_MAX_CHARS {
            errors.add(
                "tags",
                FieldErrorCode::TooLong,
                Some(MONITOR_TAG_MAX_CHARS as u32),
            );
        } else if !kept.iter().any(|known| known == tag) {
            kept.push(tag.to_owned());
        }
    }
    if kept.len() > MONITOR_TAGS_MAX {
        errors.add(
            "tags",
            FieldErrorCode::TooMany,
            Some(MONITOR_TAGS_MAX as u32),
        );
    }
    kept
}

fn check_settings(errors: &mut FieldErrors, settings: &MonitorSettings) {
    let invalid = |errors: &mut FieldErrors, field: &str| {
        errors.add(field, FieldErrorCode::InvalidValue, None);
    };
    match settings {
        MonitorSettings::Http(request) => check_request(errors, request),
        MonitorSettings::Keyword(keyword) => {
            check_request(errors, &keyword.request);
            if keyword.keyword.is_empty() {
                errors.add("settings.keyword", FieldErrorCode::Required, None);
            }
        }
        MonitorSettings::JsonQuery(query) => check_request(errors, &query.request),
        MonitorSettings::Port(port) => {
            hostname(errors, &port.hostname);
            if port.port == 0 {
                invalid(errors, "settings.port");
            }
        }
        MonitorSettings::Ping(ping) => {
            hostname(errors, &ping.hostname);
            if !(1..=PING_COUNT_MAX).contains(&ping.ping_count) {
                invalid(errors, "settings.ping_count");
            }
            if !(1..=PING_PACKET_MAX).contains(&ping.packet_size) {
                invalid(errors, "settings.packet_size");
            }
            let wait = ping.ping_per_request_timeout;
            if !(wait > 0.0 && wait <= PING_REQUEST_TIMEOUT_MAX) {
                invalid(errors, "settings.ping_per_request_timeout");
            }
        }
        MonitorSettings::Dns(dns) => {
            hostname(errors, &dns.hostname);
            if dns.dns_resolve_server.trim().is_empty() {
                errors.add(
                    "settings.dns_resolve_server",
                    FieldErrorCode::Required,
                    None,
                );
            }
            if dns.port == 0 {
                invalid(errors, "settings.port");
            }
        }
        MonitorSettings::Push(push) => {
            if let Some(token) = &push.push_token {
                let usable = token
                    .chars()
                    .all(|c| c.is_ascii_graphic() && !"/?#%".contains(c));
                if token.is_empty() || !usable {
                    invalid(errors, "settings.push_token");
                } else if token.chars().count() > PUSH_TOKEN_MAX_CHARS {
                    errors.add(
                        "settings.push_token",
                        FieldErrorCode::TooLong,
                        Some(PUSH_TOKEN_MAX_CHARS as u32),
                    );
                }
            }
        }
        MonitorSettings::Group | MonitorSettings::Manual => {}
        // Only the Kuma import creates these.
        MonitorSettings::Unsupported(_) => invalid(errors, "settings"),
    }
}

fn hostname(errors: &mut FieldErrors, hostname: &str) {
    let hostname = hostname.trim();
    if hostname.is_empty() {
        errors.add("settings.hostname", FieldErrorCode::Required, None);
    } else if hostname.len() > HOSTNAME_MAX_CHARS {
        errors.add(
            "settings.hostname",
            FieldErrorCode::TooLong,
            Some(HOSTNAME_MAX_CHARS as u32),
        );
    }
}

fn check_request(errors: &mut FieldErrors, request: &HttpSettings) {
    let invalid = |errors: &mut FieldErrors, field: &str| {
        errors.add(field, FieldErrorCode::InvalidValue, None);
    };
    if http_url(&request.url).is_none() {
        invalid(errors, "settings.url");
    }
    if Method::from_bytes(request.method.as_bytes()).is_err() {
        invalid(errors, "settings.method");
    }
    if request.headers.len() > MONITOR_HEADERS_MAX {
        errors.add(
            "settings.headers",
            FieldErrorCode::TooMany,
            Some(MONITOR_HEADERS_MAX as u32),
        );
    }
    let mut names = HashSet::new();
    for header in &request.headers {
        let valid_name = HeaderName::from_bytes(header.name.as_bytes()).is_ok()
            && names.insert(header.name.to_ascii_lowercase());
        let valid_value = match (&header.value, &header.secret) {
            (Some(value), None) => HeaderValue::from_str(value).is_ok(),
            (None, Some(secret)) => secret
                .value
                .as_deref()
                .is_none_or(|value| HeaderValue::from_str(value).is_ok()),
            _ => false,
        };
        if !valid_name || !valid_value {
            invalid(errors, "settings.headers");
        }
    }
    let valid_range = |range: &String| {
        range
            .parse::<checks::StatusCodeRange>()
            .is_ok_and(|range| range.min <= range.max)
    };
    if request.accepted_statuscodes.is_empty()
        || !request.accepted_statuscodes.iter().all(valid_range)
    {
        invalid(errors, "settings.accepted_statuscodes");
    }
    if request.maxredirects > MAX_REDIRECTS {
        errors.add(
            "settings.maxredirects",
            FieldErrorCode::InvalidValue,
            Some(MAX_REDIRECTS),
        );
    }
    if let HttpAuth::OAuth2ClientCredentials {
        oauth_token_url,
        oauth_client_id,
        ..
    } = &request.auth
    {
        if http_url(oauth_token_url).is_none() {
            invalid(errors, "settings.auth.oauth_token_url");
        }
        if oauth_client_id.trim().is_empty() {
            errors.add(
                "settings.auth.oauth_client_id",
                FieldErrorCode::Required,
                None,
            );
        }
    }
}

fn http_url(url: &str) -> Option<Url> {
    Url::parse(url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

/// One secret field of the settings: its name in `monitor_secrets` and its argument path.
struct Slot<'a> {
    field: String,
    path: &'static str,
    secret: &'a mut SecretField,
}

fn secret_slots(settings: &mut MonitorSettings) -> Vec<Slot<'_>> {
    let request = match settings {
        MonitorSettings::Http(request) => request,
        MonitorSettings::Keyword(keyword) => &mut keyword.request,
        MonitorSettings::JsonQuery(query) => &mut query.request,
        _ => return Vec::new(),
    };
    let mut slots = Vec::new();
    let mut slot = |field: &str, path, secret| {
        slots.push(Slot {
            field: field.to_owned(),
            path,
            secret,
        })
    };
    match &mut request.auth {
        HttpAuth::None => {}
        HttpAuth::Basic {
            basic_auth_pass, ..
        } => slot(
            "basic_auth_pass",
            "settings.auth.basic_auth_pass",
            basic_auth_pass,
        ),
        HttpAuth::Bearer { bearer_token } => {
            slot("bearer_token", "settings.auth.bearer_token", bearer_token)
        }
        HttpAuth::OAuth2ClientCredentials {
            oauth_client_secret,
            ..
        } => slot(
            "oauth_client_secret",
            "settings.auth.oauth_client_secret",
            oauth_client_secret,
        ),
    }
    for header in &mut request.headers {
        if let Some(secret) = &mut header.secret {
            let field = format!("header:{}", header.name.to_ascii_lowercase());
            slot(&field, "settings.headers", secret);
        }
    }
    slots
}

fn context(monitor: MonitorId, field: &str) -> String {
    format!("monitor:{monitor}:{field}")
}

/// Field names of the secrets stored for `monitor`.
pub async fn stored_fields(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
) -> Result<HashSet<String>, sqlx::Error> {
    let fields: Vec<String> =
        sqlx::query_scalar("SELECT field FROM monitor_secrets WHERE monitor_id = ?")
            .bind(monitor.get())
            .fetch_all(conn)
            .await?;
    Ok(fields.into_iter().collect())
}

/// Where a push token comes from. A person's new token needs [`PUSH_TOKEN_MIN_CHARS`];
/// Kuma's are kept as they are, so imported push URLs keep working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Person,
    Kuma,
}

/// Seals new secret values and the push token, drops secrets the settings no longer use or
/// clear, and stores the settings without them.
pub async fn store(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    monitor: MonitorId,
    mut settings: MonitorSettings,
    source: TokenSource,
) -> Result<(), ApiError> {
    let mut keep = Vec::new();
    for slot in secret_slots(&mut settings) {
        if let Some(value) = slot.secret.value.take() {
            seal(conn, key, monitor, &slot.field, &value).await?;
        }
        slot.secret.set = !slot.secret.clear;
        if slot.secret.set {
            keep.push(slot.field);
        }
    }
    store_push_token(conn, key, monitor, &mut settings, source).await?;
    if matches!(settings, MonitorSettings::Push(_)) {
        keep.push(PUSH_TOKEN.to_owned());
    }
    for field in stored_fields(conn, monitor).await? {
        if !keep.contains(&field) {
            sqlx::query("DELETE FROM monitor_secrets WHERE monitor_id = ? AND field = ?")
                .bind(monitor.get())
                .bind(&field)
                .execute(&mut *conn)
                .await?;
        }
    }
    let json = serde_json::to_string(&settings).map_err(ApiError::internal)?;
    sqlx::query("UPDATE monitors SET type = ?, settings = ? WHERE id = ?")
        .bind(super::type_text(&settings))
        .bind(json)
        .bind(monitor.get())
        .execute(conn)
        .await?;
    Ok(())
}

async fn seal(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    monitor: MonitorId,
    field: &str,
    value: &str,
) -> Result<(), sqlx::Error> {
    let sealed = key.seal(&context(monitor, field), value.as_bytes());
    sqlx::query(
        "INSERT INTO monitor_secrets (monitor_id, field, sealed) VALUES (?, ?, ?)
         ON CONFLICT (monitor_id, field) DO UPDATE SET sealed = excluded.sealed",
    )
    .bind(monitor.get())
    .bind(field)
    .bind(sealed)
    .execute(conn)
    .await?;
    Ok(())
}

/// Push monitors keep their token unless a new one is given; one without a token gets a new
/// one. Other types lose theirs. A token another monitor uses, or a person's new token shorter
/// than [`PUSH_TOKEN_MIN_CHARS`], is `invalid_value`; the current one, such as a shorter
/// imported token, may be sent back unchanged.
async fn store_push_token(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    monitor: MonitorId,
    settings: &mut MonitorSettings,
    source: TokenSource,
) -> Result<(), ApiError> {
    let MonitorSettings::Push(push) = settings else {
        sqlx::query("UPDATE monitors SET push_token_hash = NULL WHERE id = ?")
            .bind(monitor.get())
            .execute(conn)
            .await?;
        return Ok(());
    };
    let current: Option<String> =
        sqlx::query_scalar("SELECT push_token_hash FROM monitors WHERE id = ?")
            .bind(monitor.get())
            .fetch_one(&mut *conn)
            .await?;
    let token = match push.push_token.take() {
        Some(token) => token,
        None if current.is_some() => return Ok(()),
        None => new_push_token(),
    };
    let digest = token_digest(&token);
    if current.as_deref() == Some(digest.as_str()) {
        return Ok(());
    }
    let taken: Option<i64> =
        sqlx::query_scalar("SELECT id FROM monitors WHERE push_token_hash = ? AND id <> ?")
            .bind(&digest)
            .bind(monitor.get())
            .fetch_optional(&mut *conn)
            .await?;
    // One refusal for a short and a taken token, the same as for one with bad characters, so
    // the answer tells no more than the push URL's rate-limited misses do.
    let short = source == TokenSource::Person && token.chars().count() < PUSH_TOKEN_MIN_CHARS;
    if taken.is_some() || short {
        return Err(FieldErrors::one(
            "settings.push_token",
            FieldErrorCode::InvalidValue,
            None,
        ));
    }
    sqlx::query("UPDATE monitors SET push_token_hash = ? WHERE id = ?")
        .bind(&digest)
        .bind(monitor.get())
        .execute(&mut *conn)
        .await?;
    seal(conn, key, monitor, PUSH_TOKEN, &token).await?;
    Ok(())
}

fn new_push_token() -> String {
    // Bytes from 248 up are dropped so every symbol is equally likely (248 = 4 * 62).
    let fair = PUSH_TOKEN_ALPHABET.len() * (256 / PUSH_TOKEN_ALPHABET.len());
    let mut token = String::with_capacity(PUSH_TOKEN_CHARS);
    while token.len() < PUSH_TOKEN_CHARS {
        let mut bytes = [0u8; PUSH_TOKEN_CHARS];
        getrandom::fill(&mut bytes).expect("the OS provides randomness");
        for byte in bytes
            .map(usize::from)
            .into_iter()
            .filter(|byte| *byte < fair)
        {
            if token.len() < PUSH_TOKEN_CHARS {
                token.push(PUSH_TOKEN_ALPHABET[byte % PUSH_TOKEN_ALPHABET.len()] as char);
            }
        }
    }
    token
}

/// The unsealed secrets of `monitor` by field. A secret that does not open under this key
/// is left out, so the check runs without it and fails visibly.
pub async fn open(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    monitor: MonitorId,
) -> Result<HashMap<String, String>, sqlx::Error> {
    let rows: Vec<(String, Vec<u8>)> =
        sqlx::query_as("SELECT field, sealed FROM monitor_secrets WHERE monitor_id = ?")
            .bind(monitor.get())
            .fetch_all(conn)
            .await?;
    let mut secrets = HashMap::new();
    for (field, sealed) in rows {
        match key
            .open(&context(monitor, &field), &sealed)
            .and_then(|plain| String::from_utf8(plain).ok())
        {
            Some(value) => {
                secrets.insert(field, value);
            }
            None => tracing::error!(%monitor, field, "a monitor secret does not open"),
        }
    }
    Ok(secrets)
}

/// Puts the push token back into push settings, for callers who manage the monitor.
pub fn reveal_push_token(settings: &mut MonitorSettings, secrets: &HashMap<String, String>) {
    if let MonitorSettings::Push(push) = settings {
        push.push_token = secrets.get(PUSH_TOKEN).cloned();
    }
}

/// What a probe runs for these settings; `None` for the types the server evaluates itself.
pub fn check_definition(
    settings: &MonitorSettings,
    secrets: &HashMap<String, String>,
    timeout: f64,
    upside_down: bool,
) -> Option<checks::CheckDefinition> {
    let check = match settings {
        MonitorSettings::Http(request) => checks::Check::Http(http_request(request, secrets)?),
        MonitorSettings::Keyword(keyword) => checks::Check::Keyword(checks::KeywordCheck {
            request: http_request(&keyword.request, secrets)?,
            keyword: keyword.keyword.clone(),
            invert_keyword: keyword.invert_keyword,
        }),
        MonitorSettings::JsonQuery(query) => checks::Check::JsonQuery(checks::JsonQueryCheck {
            request: http_request(&query.request, secrets)?,
            json_path: query.json_path.clone(),
            json_path_operator: json_path_operator(query.json_path_operator),
            expected_value: query.expected_value.clone(),
        }),
        MonitorSettings::Port(port) => checks::Check::Port(checks::PortCheck {
            hostname: port.hostname.trim().to_owned(),
            port: port.port,
        }),
        MonitorSettings::Ping(ping) => checks::Check::Ping(checks::PingCheck {
            hostname: ping.hostname.trim().to_owned(),
            ping_count: ping.ping_count,
            packet_size: ping.packet_size,
            ping_per_request_timeout: ping.ping_per_request_timeout,
        }),
        MonitorSettings::Dns(dns) => checks::Check::Dns(checks::DnsCheck {
            hostname: dns.hostname.trim().to_owned(),
            dns_resolve_type: dns_record_type(dns.dns_resolve_type),
            dns_resolve_server: dns.dns_resolve_server.clone(),
            port: dns.port,
        }),
        MonitorSettings::Push(_)
        | MonitorSettings::Group
        | MonitorSettings::Manual
        | MonitorSettings::Unsupported(_) => return None,
    };
    Some(checks::CheckDefinition {
        timeout,
        upside_down,
        check,
    })
}

fn http_request(
    request: &HttpSettings,
    secrets: &HashMap<String, String>,
) -> Option<checks::HttpRequest> {
    // A cleared secret is sent empty; a set one that does not open builds no check.
    let secret = |slot: &SecretField, field: &str| {
        if slot.set {
            secrets.get(field).map(checks::Secret::new)
        } else {
            Some(checks::Secret::new(""))
        }
    };
    let mut headers = BTreeMap::new();
    for header in &request.headers {
        let value = match (&header.value, &header.secret) {
            (Some(value), _) => checks::Secret::new(value),
            (None, Some(slot)) => secret(
                slot,
                &format!("header:{}", header.name.to_ascii_lowercase()),
            )?,
            (None, None) => return None,
        };
        headers.insert(header.name.clone(), value);
    }
    let auth = match &request.auth {
        HttpAuth::None => checks::HttpAuth::None,
        HttpAuth::Basic {
            basic_auth_user,
            basic_auth_pass,
        } => checks::HttpAuth::Basic {
            basic_auth_user: basic_auth_user.clone(),
            basic_auth_pass: secret(basic_auth_pass, "basic_auth_pass")?,
        },
        HttpAuth::Bearer { bearer_token } => checks::HttpAuth::Bearer {
            bearer_token: secret(bearer_token, "bearer_token")?,
        },
        HttpAuth::OAuth2ClientCredentials {
            oauth_token_url,
            oauth_client_id,
            oauth_client_secret,
            oauth_scopes,
            oauth_audience,
            oauth_auth_method,
        } => checks::HttpAuth::OAuth2ClientCredentials {
            oauth_token_url: http_url(oauth_token_url)?,
            oauth_client_id: oauth_client_id.clone(),
            oauth_client_secret: secret(oauth_client_secret, "oauth_client_secret")?,
            oauth_scopes: oauth_scopes.clone(),
            oauth_audience: oauth_audience.clone(),
            oauth_auth_method: match oauth_auth_method {
                OAuthClientAuth::ClientSecretBasic => checks::OAuthClientAuth::ClientSecretBasic,
                OAuthClientAuth::ClientSecretPost => checks::OAuthClientAuth::ClientSecretPost,
            },
        },
    };
    Some(checks::HttpRequest {
        url: http_url(&request.url)?,
        method: request.method.clone(),
        headers,
        body: request.body.clone(),
        http_body_encoding: match request.http_body_encoding {
            HttpBodyEncoding::Json => checks::HttpBodyEncoding::Json,
            HttpBodyEncoding::Form => checks::HttpBodyEncoding::Form,
            HttpBodyEncoding::Xml => checks::HttpBodyEncoding::Xml,
        },
        auth,
        accepted_statuscodes: request
            .accepted_statuscodes
            .iter()
            .map(|range| range.parse().ok())
            .collect::<Option<_>>()?,
        maxredirects: request.maxredirects,
        ignore_tls: request.ignore_tls,
        ip_family: request.ip_family.map(|family| match family {
            IpFamily::Ipv4 => checks::IpFamily::Ipv4,
            IpFamily::Ipv6 => checks::IpFamily::Ipv6,
        }),
        cache_bust: request.cache_bust,
    })
}

fn json_path_operator(operator: JsonPathOperator) -> checks::JsonPathOperator {
    use checks::JsonPathOperator as Op;
    match operator {
        JsonPathOperator::Equals => Op::Equals,
        JsonPathOperator::NotEquals => Op::NotEquals,
        JsonPathOperator::Less => Op::Less,
        JsonPathOperator::LessOrEqual => Op::LessOrEqual,
        JsonPathOperator::Greater => Op::Greater,
        JsonPathOperator::GreaterOrEqual => Op::GreaterOrEqual,
        JsonPathOperator::Contains => Op::Contains,
    }
}

fn dns_record_type(record: DnsRecordType) -> checks::DnsRecordType {
    use checks::DnsRecordType as Rr;
    match record {
        DnsRecordType::A => Rr::A,
        DnsRecordType::Aaaa => Rr::Aaaa,
        DnsRecordType::Cname => Rr::Cname,
        DnsRecordType::Mx => Rr::Mx,
        DnsRecordType::Ns => Rr::Ns,
        DnsRecordType::Txt => Rr::Txt,
        DnsRecordType::Srv => Rr::Srv,
        DnsRecordType::Ptr => Rr::Ptr,
        DnsRecordType::Caa => Rr::Caa,
        DnsRecordType::Soa => Rr::Soa,
    }
}
