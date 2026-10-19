//! Typed check definitions. Field names follow Uptime Kuma 2.5.0's `monitor` columns so the
//! importer maps them 1:1. Deviations are noted on the field.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

/// One check as a probe runs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckDefinition {
    /// Hard limit for the whole check in seconds. Kuma replaces a zero timeout with
    /// `interval * 0.8` at runtime; the importer applies that, so this must be positive.
    /// Kuma's port check ignores its timeout (tcp-ping uses 5 s); here every type honours it.
    pub timeout: f64,
    /// Inverts the result before retries, as Kuma does.
    #[serde(default)]
    pub upside_down: bool,
    #[serde(flatten)]
    pub check: Check,
}

/// The check per Kuma monitor type, tagged by Kuma's `type` value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Check {
    Http(HttpRequest),
    Keyword(KeywordCheck),
    JsonQuery(JsonQueryCheck),
    Port(PortCheck),
    Ping(PingCheck),
    Dns(DnsCheck),
}

/// Request options shared by the http, keyword and json-query types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub url: Url,
    #[serde(default = "default_method")]
    pub method: String,
    /// Kuma stores a JSON object string; here it is the parsed object. Values are [`Secret`]
    /// because some carry credentials.
    #[serde(default)]
    pub headers: BTreeMap<String, Secret>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub http_body_encoding: HttpBodyEncoding,
    /// Kuma keeps these as flat `auth_method` and credential columns.
    #[serde(default)]
    pub auth: HttpAuth,
    /// Kuma's `accepted_statuscodes_json`.
    #[serde(default = "default_accepted_statuscodes")]
    pub accepted_statuscodes: Vec<StatusCodeRange>,
    #[serde(default = "default_maxredirects")]
    pub maxredirects: u32,
    #[serde(default)]
    pub ignore_tls: bool,
    #[serde(default)]
    pub ip_family: Option<IpFamily>,
    #[serde(default)]
    pub cache_bust: bool,
}

fn default_method() -> String {
    "GET".to_owned()
}

fn default_accepted_statuscodes() -> Vec<StatusCodeRange> {
    vec![StatusCodeRange { min: 200, max: 299 }]
}

fn default_maxredirects() -> u32 {
    10
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpBodyEncoding {
    #[default]
    Json,
    Form,
    Xml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IpFamily {
    Ipv4,
    Ipv6,
}

/// Kuma's `auth_method` with the columns each method uses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "auth_method", rename_all = "kebab-case")]
pub enum HttpAuth {
    #[default]
    None,
    Basic {
        basic_auth_user: String,
        basic_auth_pass: Secret,
    },
    Bearer {
        bearer_token: Secret,
    },
    #[serde(rename = "oauth2-cc")]
    OAuth2ClientCredentials {
        oauth_token_url: Url,
        oauth_client_id: String,
        oauth_client_secret: Secret,
        #[serde(default)]
        oauth_scopes: Option<String>,
        #[serde(default)]
        oauth_audience: Option<String>,
        #[serde(default)]
        oauth_auth_method: OAuthClientAuth,
    },
    /// Not supported: checks return [`crate::Unsupported::NtlmAuth`].
    Ntlm {
        basic_auth_user: String,
        basic_auth_pass: Secret,
        #[serde(default)]
        auth_domain: Option<String>,
        #[serde(default)]
        auth_workstation: Option<String>,
    },
    /// Not supported: checks return [`crate::Unsupported::MtlsAuth`].
    Mtls {
        tls_cert: String,
        tls_key: Secret,
        #[serde(default)]
        tls_ca: Option<String>,
    },
}

/// How the client authenticates at the OAuth token endpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthClientAuth {
    #[default]
    ClientSecretBasic,
    ClientSecretPost,
}

/// An accepted status code or inclusive range, written as Kuma writes it: `"200"` or `"200-299"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StatusCodeRange {
    pub min: u16,
    pub max: u16,
}

impl StatusCodeRange {
    pub fn contains(&self, code: u16) -> bool {
        (self.min..=self.max).contains(&code)
    }
}

impl FromStr for StatusCodeRange {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parse = |part: &str| {
            part.trim()
                .parse::<u16>()
                .map_err(|_| format!("{value} is not a valid status code range"))
        };
        match value.split_once('-') {
            Some((min, max)) => Ok(Self {
                min: parse(min)?,
                max: parse(max)?,
            }),
            None => {
                let code = parse(value)?;
                Ok(Self {
                    min: code,
                    max: code,
                })
            }
        }
    }
}

impl TryFrom<String> for StatusCodeRange {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<StatusCodeRange> for String {
    fn from(range: StatusCodeRange) -> Self {
        if range.min == range.max {
            range.min.to_string()
        } else {
            format!("{}-{}", range.min, range.max)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeywordCheck {
    #[serde(flatten)]
    pub request: HttpRequest,
    pub keyword: String,
    #[serde(default)]
    pub invert_keyword: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonQueryCheck {
    #[serde(flatten)]
    pub request: HttpRequest,
    /// A JSONata expression; empty compares the whole response.
    #[serde(default)]
    pub json_path: String,
    #[serde(default)]
    pub json_path_operator: JsonPathOperator,
    pub expected_value: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JsonPathOperator {
    #[default]
    #[serde(rename = "==")]
    Equals,
    #[serde(rename = "!=")]
    NotEquals,
    #[serde(rename = "<")]
    Less,
    #[serde(rename = "<=")]
    LessOrEqual,
    #[serde(rename = ">")]
    Greater,
    #[serde(rename = ">=")]
    GreaterOrEqual,
    #[serde(rename = "contains")]
    Contains,
}

impl fmt::Display for JsonPathOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Equals => "==",
            Self::NotEquals => "!=",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
            Self::Contains => "contains",
        })
    }
}

/// Plain TCP connect. Kuma's `smtp_security` and `expected_tls_alert` options are not modelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortCheck {
    pub hostname: String,
    pub port: u16,
}

/// ICMP echo. Kuma's `ping_numeric` only changed the ping command's output and is dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingCheck {
    pub hostname: String,
    #[serde(default = "default_ping_count")]
    pub ping_count: u16,
    #[serde(default = "default_packet_size")]
    pub packet_size: u16,
    /// Seconds to wait for each reply.
    #[serde(default = "default_ping_per_request_timeout")]
    pub ping_per_request_timeout: f64,
}

fn default_ping_count() -> u16 {
    1
}

fn default_packet_size() -> u16 {
    56
}

fn default_ping_per_request_timeout() -> f64 {
    2.0
}

/// DNS lookup. Kuma's `conditions` on the record are not modelled; without them a check is up
/// whenever the lookup returns records, as in Kuma.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsCheck {
    /// The name to resolve, or the IP address for PTR.
    pub hostname: String,
    pub dns_resolve_type: DnsRecordType,
    /// Comma-separated resolver IPs or hostnames, as in Kuma.
    #[serde(default = "default_dns_resolve_server")]
    pub dns_resolve_server: String,
    /// Resolver port, Kuma's `port` column.
    #[serde(default = "default_dns_port")]
    pub port: u16,
}

fn default_dns_resolve_server() -> String {
    "1.1.1.1".to_owned()
}

fn default_dns_port() -> u16 {
    53
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsRecordType {
    A,
    Aaaa,
    Cname,
    Mx,
    Ns,
    Txt,
    Srv,
    Ptr,
    Caa,
    Soa,
}

/// A credential that arrives unsealed. Its `Debug` output is redacted so it never reaches logs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}
