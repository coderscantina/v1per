//! Check execution shared by the built-in probe and remote agents. Behaviour follows Uptime
//! Kuma 2.5.0 for the http, keyword, json-query, port, ping and dns types. This crate never
//! touches the server, the control database or DuckDB.

mod context;
mod definition;
mod dns;
mod http;
mod json_query;
mod oauth;
mod ping;
mod port;
mod result;
mod tls;

use std::time::Duration;

use tokio::time::Instant;

pub use context::{CheckContext, DEFAULT_MAX_BODY_BYTES, InvalidRootCert};
pub use definition::{
    Check, CheckDefinition, DnsCheck, DnsRecordType, HttpAuth, HttpBodyEncoding, HttpRequest,
    IpFamily, JsonPathOperator, JsonQueryCheck, KeywordCheck, OAuthClientAuth, PingCheck,
    PortCheck, Secret, StatusCodeRange,
};
pub use json_query::validate as validate_json_query;
pub use result::{CertInfo, CertType, CheckResult, Failure, Status, TlsInfo, Unsupported};

use http::Expectation;

/// Lets ping stop at the deadline with its partial result before the hard limit cuts it off.
const HARD_LIMIT_MARGIN: Duration = Duration::from_millis(250);

/// Runs one check. It never takes longer than the definition's timeout plus a 250 ms margin.
pub async fn run(definition: &CheckDefinition, ctx: &CheckContext) -> CheckResult {
    let Some(timeout) = Duration::try_from_secs_f64(definition.timeout)
        .ok()
        .filter(|timeout| !timeout.is_zero())
    else {
        return CheckResult::down(
            Failure::Check,
            format!("Invalid timeout {}", definition.timeout),
        );
    };
    let deadline = Instant::now() + timeout;
    let check = &definition.check;
    let result = match tokio::time::timeout_at(
        deadline + HARD_LIMIT_MARGIN,
        run_check(check, ctx, deadline),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => CheckResult::down(Failure::Timeout, timeout_message(check, timeout)),
    };
    if definition.upside_down {
        result.invert()
    } else {
        result
    }
}

async fn run_check(check: &Check, ctx: &CheckContext, deadline: Instant) -> CheckResult {
    let http = |request, expectation| http::check(request, expectation, ctx, deadline);
    match check {
        Check::Http(request) => http(request, Expectation::Status).await,
        Check::Keyword(keyword) => {
            http(
                &keyword.request,
                Expectation::Keyword {
                    keyword: &keyword.keyword,
                    invert: keyword.invert_keyword,
                },
            )
            .await
        }
        Check::JsonQuery(query) => http(&query.request, Expectation::JsonQuery(query)).await,
        Check::Port(port) => port::check(port).await,
        Check::Ping(ping) => ping::check(ping, deadline).await,
        Check::Dns(dns) => dns::check(dns).await,
    }
}

fn timeout_message(check: &Check, timeout: Duration) -> String {
    match check {
        Check::Http(_) | Check::Keyword(_) | Check::JsonQuery(_) => http::timeout_message(timeout),
        Check::Port(_) => "Connection failed".to_owned(),
        Check::Ping(ping) => format!("ping: {} timed out after {timeout:?}", ping.hostname),
        Check::Dns(dns) => dns::error_message(dns, "ETIMEOUT"),
    }
}
