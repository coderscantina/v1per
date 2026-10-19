//! The http, keyword and json-query types, following Kuma's axios request in `monitor.js`.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use reqwest::{Method, StatusCode, redirect};
use tokio::time::Instant;

use crate::context::CheckContext;
use crate::definition::{HttpAuth, HttpBodyEncoding, HttpRequest, IpFamily, JsonQueryCheck};
use crate::json_query;
use crate::oauth::OAuthRequest;
use crate::result::{CheckResult, Failure, Unsupported};
use crate::tls::{self, CaptureSlot};

const KUMA_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.9";
const USER_AGENT_VALUE: &str = concat!("v1per/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy)]
pub(crate) enum Expectation<'a> {
    Status,
    Keyword { keyword: &'a str, invert: bool },
    JsonQuery(&'a JsonQueryCheck),
}

pub(crate) fn timeout_message(timeout: Duration) -> String {
    format!("timeout of {}ms exceeded", timeout.as_millis())
}

pub(crate) async fn check(
    request: &HttpRequest,
    expectation: Expectation<'_>,
    ctx: &CheckContext,
    deadline: Instant,
) -> CheckResult {
    match &request.auth {
        HttpAuth::Ntlm { .. } => return CheckResult::unsupported(Unsupported::NtlmAuth),
        HttpAuth::Mtls { .. } => return CheckResult::unsupported(Unsupported::MtlsAuth),
        _ => {}
    }
    if let Expectation::JsonQuery(query) = expectation
        && let Err(reason) = json_query::validate(&query.json_path)
    {
        return CheckResult::unsupported(Unsupported::JsonQuery { reason });
    }

    let started = Instant::now();
    let slot = CaptureSlot::default();
    let sent = send(request, ctx, &slot).await;
    let host = request.url.host_str().unwrap_or_default();
    let capture = (request.url.scheme() == "https")
        .then(|| slot.take())
        .flatten();
    let tls = capture.as_ref().map(|capture| capture.info(host));

    let response = match sent {
        Ok(response) => response,
        Err(SendError::Config(message)) => return CheckResult::down(Failure::Check, message),
        Err(SendError::Http(error)) => {
            let rejection = match (&capture, &tls) {
                (Some(capture), Some(info)) if !request.ignore_tls => capture.rejection(host, info),
                _ => None,
            };
            let mut result = describe(&error, request, rejection);
            result.tls = tls;
            return result;
        }
    };

    let status = response.status();
    let body = read_body(response, ctx.max_body_bytes).await;
    let latency = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut result = evaluate(request, expectation, status, body, deadline, latency).await;
    result.http_status = Some(status.as_u16());
    result.tls = tls;
    result
}

async fn evaluate(
    request: &HttpRequest,
    expectation: Expectation<'_>,
    status: StatusCode,
    body: Result<Body, reqwest::Error>,
    deadline: Instant,
    latency: u64,
) -> CheckResult {
    let body = match body {
        Ok(body) => body,
        Err(error) => return describe(&error, request, None),
    };
    if !request
        .accepted_statuscodes
        .iter()
        .any(|range| range.contains(status.as_u16()))
    {
        return CheckResult::down(
            Failure::Check,
            format!("Request failed with status code {}", status.as_u16()),
        );
    }
    let status_line = format!(
        "{} - {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or_default()
    );

    match expectation {
        Expectation::Status => CheckResult::up(status_line).with_latency(latency),
        Expectation::Keyword { keyword, invert } => {
            let text = keyword_text(&body.bytes);
            let found = text.contains(keyword);
            if found != invert {
                let state = if found { "is" } else { "not" };
                CheckResult::up(format!("{status_line}, keyword {state} found"))
                    .with_latency(latency)
            } else {
                let state = if found { "present" } else { "not" };
                CheckResult::down(
                    Failure::Check,
                    format!(
                        "{status_line}, but keyword is {state} in [{}]",
                        excerpt(&text)
                    ),
                )
                .with_latency(latency)
            }
        }
        Expectation::JsonQuery(query) => {
            if body.truncated {
                return CheckResult::down(
                    Failure::Check,
                    format!("Response body is larger than {} bytes", body.bytes.len()),
                )
                .with_latency(latency);
            }
            let data = json_query::response_value(&body.bytes);
            let budget = deadline.saturating_duration_since(Instant::now());
            match json_query::compare(query, data, budget).await {
                Ok(comparison) => {
                    let summary = format!(
                        "(comparing {} {} {})",
                        comparison.value, query.json_path_operator, query.expected_value
                    );
                    if comparison.passed {
                        CheckResult::up(format!("JSON query passes {summary}"))
                            .with_latency(latency)
                    } else {
                        CheckResult::down(
                            Failure::JsonQueryMismatch,
                            format!("JSON query does not pass {summary}"),
                        )
                        .with_latency(latency)
                    }
                }
                Err(message) => CheckResult::down(Failure::Check, message).with_latency(latency),
            }
        }
    }
}

enum SendError {
    /// The definition cannot produce a request; Kuma throws before sending.
    Config(String),
    Http(reqwest::Error),
}

async fn send(
    request: &HttpRequest,
    ctx: &CheckContext,
    slot: &CaptureSlot,
) -> Result<reqwest::Response, SendError> {
    let oauth = match &request.auth {
        HttpAuth::OAuth2ClientCredentials {
            oauth_token_url,
            oauth_client_id,
            oauth_client_secret,
            oauth_scopes,
            oauth_audience,
            oauth_auth_method,
        } => Some(OAuthRequest {
            token_url: oauth_token_url,
            client_id: oauth_client_id,
            client_secret: oauth_client_secret,
            scopes: oauth_scopes.as_deref(),
            audience: oauth_audience.as_deref(),
            client_auth: *oauth_auth_method,
        }),
        _ => None,
    };
    let client = client(request, ctx, slot).map_err(SendError::Http)?;
    let authorization = authorization_header(ctx, oauth.as_ref(), false).await?;
    let response = client
        .execute(build(&client, request, authorization.as_deref())?)
        .await
        .map_err(SendError::Http)?;
    if response.status() == StatusCode::UNAUTHORIZED && authorization.is_some() {
        let authorization = authorization_header(ctx, oauth.as_ref(), true).await?;
        return client
            .execute(build(&client, request, authorization.as_deref())?)
            .await
            .map_err(SendError::Http);
    }
    Ok(response)
}

async fn authorization_header(
    ctx: &CheckContext,
    oauth: Option<&OAuthRequest<'_>>,
    refresh: bool,
) -> Result<Option<String>, SendError> {
    let Some(oauth) = oauth else {
        return Ok(None);
    };
    ctx.oauth_tokens
        .authorization(&ctx.oauth_client, oauth, refresh)
        .await
        .map(Some)
        .map_err(|error| SendError::Config(format!("The oauth config is invalid. {error}")))
}

fn client(
    request: &HttpRequest,
    ctx: &CheckContext,
    slot: &CaptureSlot,
) -> Result<reqwest::Client, reqwest::Error> {
    let max = request.maxredirects as usize;
    let policy = match max {
        0 => redirect::Policy::none(),
        _ => redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > max {
                attempt.error(TooManyRedirects)
            } else {
                attempt.follow()
            }
        }),
    };
    let mut builder = reqwest::Client::builder()
        .tls_backend_preconfigured(tls::client_config(ctx, request.ignore_tls, slot))
        .redirect(policy)
        .pool_max_idle_per_host(0);
    // Binding the unspecified address limits connections to that family.
    builder = match request.ip_family {
        Some(IpFamily::Ipv4) => builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        Some(IpFamily::Ipv6) => builder.local_address(IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
        None => builder,
    };
    builder.build()
}

fn build(
    client: &reqwest::Client,
    request: &HttpRequest,
    authorization: Option<&str>,
) -> Result<reqwest::Request, SendError> {
    let method = Method::from_bytes(request.method.to_uppercase().as_bytes())
        .map_err(|_| SendError::Config(format!("Invalid HTTP method {}", request.method)))?;
    let mut url = request.url.clone();
    if request.cache_bust {
        let buster = format!("{:x}", rand::random::<u64>());
        url.query_pairs_mut()
            .append_pair("uptime_kuma_cachebuster", &buster);
    }

    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers.insert(ACCEPT, HeaderValue::from_static(KUMA_ACCEPT));
    let body = encode_body(request)?;
    if let Some((content_type, _)) = &body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    }
    let authorization = match (&request.auth, authorization) {
        (_, Some(token)) => Some(token.to_owned()),
        (
            HttpAuth::Basic {
                basic_auth_user,
                basic_auth_pass,
            },
            None,
        ) => Some(format!(
            "Basic {}",
            STANDARD.encode(format!("{basic_auth_user}:{}", basic_auth_pass.expose()))
        )),
        (HttpAuth::Bearer { bearer_token }, None) => {
            Some(format!("Bearer {}", bearer_token.expose()))
        }
        _ => None,
    };
    if let Some(authorization) = authorization {
        headers.insert(AUTHORIZATION, sensitive(&authorization, "Authorization")?);
    }
    // Monitor headers come last and win, as in Kuma.
    for (name, value) in &request.headers {
        let header = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| SendError::Config(format!("Invalid header name {name}")))?;
        headers.insert(header, sensitive(value.expose(), name)?);
    }

    let mut builder = client.request(method, url).headers(headers);
    if let Some((_, body)) = body {
        builder = builder.body(body);
    }
    builder.build().map_err(SendError::Http)
}

fn sensitive(value: &str, name: &str) -> Result<HeaderValue, SendError> {
    let mut value = HeaderValue::from_str(value)
        .map_err(|_| SendError::Config(format!("Invalid value for header {name}")))?;
    value.set_sensitive(true);
    Ok(value)
}

fn encode_body(request: &HttpRequest) -> Result<Option<(&'static str, String)>, SendError> {
    let Some(body) = request
        .body
        .as_deref()
        .filter(|body| !body.trim().is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(match request.http_body_encoding {
        HttpBodyEncoding::Json => {
            serde_json::from_str::<serde_json::Value>(body).map_err(|error| {
                SendError::Config(format!("Your JSON body is invalid. {error}"))
            })?;
            ("application/json", body.to_owned())
        }
        HttpBodyEncoding::Form => ("application/x-www-form-urlencoded", body.to_owned()),
        HttpBodyEncoding::Xml => ("text/xml; charset=utf-8", body.to_owned()),
    }))
}

struct Body {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_body(mut response: reqwest::Response, cap: usize) -> Result<Body, reqwest::Error> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let room = cap - bytes.len();
        if chunk.len() > room {
            bytes.extend_from_slice(&chunk[..room]);
            return Ok(Body {
                bytes,
                truncated: true,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Body {
        bytes,
        truncated: false,
    })
}

/// The text Kuma searches: axios re-serialises bodies that parse as JSON.
fn keyword_text(body: &[u8]) -> String {
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(serde_json::Value::String(text)) => text,
        Ok(value) => value.to_string(),
        Err(_) => String::from_utf8_lossy(body).into_owned(),
    }
}

/// Kuma's excerpt: tags and line breaks become spaces, whitespace runs collapse, 50 characters.
fn excerpt(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                }
                out.push(' ');
            }
            '\n' | '\r' => out.push(' '),
            ch if ch.is_whitespace() => {
                while chars.next_if(|next| next.is_whitespace()).is_some() {}
                out.push(' ');
            }
            ch => out.push(ch),
        }
    }
    let out = out.trim();
    if out.chars().count() > 50 {
        format!("{}...", out.chars().take(47).collect::<String>())
    } else {
        out.to_owned()
    }
}

#[derive(Debug)]
struct TooManyRedirects;

impl fmt::Display for TooManyRedirects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Maximum number of redirects exceeded")
    }
}

impl std::error::Error for TooManyRedirects {}

/// Node-style wording for transport errors.
fn describe(
    error: &reqwest::Error,
    request: &HttpRequest,
    rejection: Option<String>,
) -> CheckResult {
    let host = request.url.host_str().unwrap_or_default();
    let port = request.url.port_or_known_default().unwrap_or_default();
    let message = if error.is_redirect() {
        TooManyRedirects.to_string()
    } else if let Some(rejection) = rejection {
        rejection
    } else {
        let refused = causes(error).any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::ConnectionRefused)
        });
        if refused {
            format!("connect ECONNREFUSED {host}:{port}")
        } else if causes(error).any(|cause| cause.to_string() == "dns error") {
            format!("getaddrinfo ENOTFOUND {host}")
        } else {
            causes(error)
                .last()
                .map(ToString::to_string)
                .unwrap_or_else(|| error.to_string())
        }
    };
    CheckResult::down(Failure::Check, message)
}

fn causes<'a>(
    error: &'a (dyn std::error::Error + 'static),
) -> impl Iterator<Item = &'a (dyn std::error::Error + 'static)> {
    std::iter::successors(Some(error), |&cause| cause.source())
}
