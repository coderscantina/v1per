mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;
use support::{Pki, Reply, definition, serve};
use v1per_checks::{CertType, CheckContext, Failure, Status, Unsupported, run};

fn http(url: &str) -> serde_json::Value {
    json!({ "type": "http", "timeout": 5, "url": url })
}

#[tokio::test]
async fn http_passes_on_an_accepted_status() {
    let base = serve(None, |_| Reply::new(200, "hello")).await;
    let result = run(&definition(http(&base)), &CheckContext::new()).await;

    assert_eq!(result.status, Status::Up);
    assert_eq!(result.message, "200 - OK");
    assert_eq!(result.http_status, Some(200));
    assert!(result.latency_ms.is_some());
    assert_eq!(result.failure, None);
}

#[tokio::test]
async fn http_fails_on_a_rejected_status() {
    let base = serve(None, |_| Reply::new(503, "")).await;
    let result = run(&definition(http(&base)), &CheckContext::new()).await;

    assert_eq!(result.status, Status::Down);
    assert_eq!(result.message, "Request failed with status code 503");
    assert_eq!(result.http_status, Some(503));
    assert_eq!(result.failure, Some(Failure::Check));
}

#[tokio::test]
async fn accepted_status_codes_take_codes_and_ranges() {
    let base = serve(None, |request| match request.path.as_str() {
        "/missing" => Reply::new(404, ""),
        "/teapot" => Reply::new(418, ""),
        _ => Reply::new(500, ""),
    })
    .await;
    let check = |path: &str| {
        definition(json!({
            "type": "http", "timeout": 5, "url": format!("{base}{path}"),
            "accepted_statuscodes": ["200-299", "404", "410-420"],
        }))
    };
    let ctx = CheckContext::new();

    assert_eq!(run(&check("/missing"), &ctx).await.status, Status::Up);
    assert_eq!(run(&check("/teapot"), &ctx).await.status, Status::Up);
    assert_eq!(run(&check("/broken"), &ctx).await.status, Status::Down);
}

#[tokio::test]
async fn http_times_out_within_the_timeout() {
    let base = serve(None, |_| Reply::new(200, "").delay(Duration::from_secs(5))).await;
    let started = Instant::now();
    let result = run(
        &definition(json!({ "type": "http", "timeout": 0.5, "url": base })),
        &CheckContext::new(),
    )
    .await;

    assert!(started.elapsed() < Duration::from_millis(900));
    assert_eq!(result.status, Status::Down);
    assert_eq!(result.message, "timeout of 500ms exceeded");
    assert_eq!(result.failure, Some(Failure::Timeout));
}

#[tokio::test]
async fn redirects_stop_at_maxredirects() {
    // /hop/3 redirects three times before answering.
    let base = serve(None, |request| {
        match request
            .path
            .strip_prefix("/hop/")
            .and_then(|n| n.parse::<u32>().ok())
        {
            Some(0) | None => Reply::new(200, "arrived"),
            Some(n) => Reply::new(302, "").header("location", format!("/hop/{}", n - 1)),
        }
    })
    .await;
    let check = |maxredirects: u32| {
        definition(json!({
            "type": "http", "timeout": 5, "url": format!("{base}/hop/3"), "maxredirects": maxredirects,
        }))
    };
    let ctx = CheckContext::new();

    assert_eq!(run(&check(3), &ctx).await.status, Status::Up);
    let limited = run(&check(2), &ctx).await;
    assert_eq!(limited.status, Status::Down);
    assert_eq!(limited.message, "Maximum number of redirects exceeded");
    let unfollowed = run(&check(0), &ctx).await;
    assert_eq!(unfollowed.message, "Request failed with status code 302");
}

#[tokio::test]
async fn connection_refused_is_reported_like_node() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let result = run(
        &definition(http(&format!("http://127.0.0.1:{port}"))),
        &CheckContext::new(),
    )
    .await;

    assert_eq!(result.status, Status::Down);
    assert_eq!(
        result.message,
        format!("connect ECONNREFUSED 127.0.0.1:{port}")
    );
}

#[tokio::test]
async fn trusted_certificate_reports_tls_info() {
    let pki = Pki::new();
    let base = serve(Some(pki.valid.clone()), |_| Reply::new(200, "")).await;
    let result = run(&definition(http(&base)), &pki.context()).await;

    assert_eq!(result.status, Status::Up);
    let tls = result.tls.expect("tls info");
    assert!(tls.valid);
    assert!(tls.hostname_match);
    let leaf = &tls.chain[0];
    assert_eq!(leaf.subject, "localhost");
    assert_eq!(leaf.issuer, "v1per test CA");
    assert_eq!(leaf.cert_type, CertType::Server);
    assert_eq!(leaf.valid_for, ["localhost"]);
    assert!(leaf.days_remaining > 0);
}

#[tokio::test]
async fn expired_certificate_fails_unless_tls_is_ignored() {
    let pki = Pki::new();
    let base = serve(Some(pki.expired.clone()), |_| Reply::new(200, "")).await;
    let ctx = pki.context();

    let strict = run(&definition(http(&base)), &ctx).await;
    assert_eq!(strict.status, Status::Down);
    assert_eq!(strict.message, "certificate has expired");
    assert!(strict.tls.as_ref().is_some_and(|tls| !tls.valid));

    let mut ignoring = http(&base);
    ignoring["ignore_tls"] = json!(true);
    let ignored = run(&definition(ignoring), &ctx).await;
    assert_eq!(ignored.status, Status::Up);
    let tls = ignored.tls.expect("tls info");
    assert!(!tls.valid);
    assert!(tls.chain[0].days_remaining < 0);
}

#[tokio::test]
async fn self_signed_certificate_fails_unless_tls_is_ignored() {
    let pki = Pki::new();
    let base = serve(Some(pki.self_signed.clone()), |_| Reply::new(200, "")).await;
    let ctx = pki.context();

    let strict = run(&definition(http(&base)), &ctx).await;
    assert_eq!(strict.status, Status::Down);
    assert_eq!(strict.message, "self-signed certificate");

    let mut ignoring = http(&base);
    ignoring["ignore_tls"] = json!(true);
    let ignored = run(&definition(ignoring), &ctx).await;
    assert_eq!(ignored.status, Status::Up);
    assert_eq!(
        ignored.tls.expect("tls info").chain[0].cert_type,
        CertType::SelfSigned
    );
}

#[tokio::test]
async fn hostname_mismatch_fails_unless_tls_is_ignored() {
    let pki = Pki::new();
    let base = serve(Some(pki.valid.clone()), |_| Reply::new(200, "")).await;
    let by_ip = base.replace("localhost", "127.0.0.1");
    let ctx = pki.context();

    let strict = run(&definition(http(&by_ip)), &ctx).await;
    assert_eq!(strict.status, Status::Down);
    assert_eq!(
        strict.message,
        "Hostname/IP does not match certificate's altnames: Host: 127.0.0.1. is not in the cert's altnames: DNS:localhost"
    );

    let mut ignoring = http(&by_ip);
    ignoring["ignore_tls"] = json!(true);
    let tls = run(&definition(ignoring), &ctx)
        .await
        .tls
        .expect("tls info");
    assert!(tls.valid);
    assert!(!tls.hostname_match);
}

fn keyword(base: &str, keyword: &str, invert: bool) -> serde_json::Value {
    json!({
        "type": "keyword", "timeout": 5, "url": base, "keyword": keyword, "invert_keyword": invert,
    })
}

#[tokio::test]
async fn keyword_found_passes() {
    let base = serve(None, |_| Reply::new(200, "<h1>All systems go</h1>")).await;
    let result = run(
        &definition(keyword(&base, "systems go", false)),
        &CheckContext::new(),
    )
    .await;

    assert_eq!(result.status, Status::Up);
    assert_eq!(result.message, "200 - OK, keyword is found");
}

#[tokio::test]
async fn keyword_missing_fails_with_an_excerpt() {
    let body = "<html>\n<body><p>Maintenance   in progress, back soon. Thank you for your patience!</p></body></html>";
    let base = serve(None, move |_| Reply::new(200, body)).await;
    let result = run(
        &definition(keyword(&base, "systems go", false)),
        &CheckContext::new(),
    )
    .await;

    assert_eq!(result.status, Status::Down);
    assert_eq!(
        result.message,
        "200 - OK, but keyword is not in [Maintenance in progress, back soon. Thank you f...]"
    );
    assert!(result.latency_ms.is_some());
}

#[tokio::test]
async fn inverted_keyword_fails_when_present() {
    let base = serve(None, |_| Reply::new(200, "fatal error")).await;
    let ctx = CheckContext::new();

    let present = run(&definition(keyword(&base, "error", true)), &ctx).await;
    assert_eq!(present.status, Status::Down);
    assert_eq!(
        present.message,
        "200 - OK, but keyword is present in [fatal error]"
    );

    let absent = run(&definition(keyword(&base, "panic", true)), &ctx).await;
    assert_eq!(absent.status, Status::Up);
    assert_eq!(absent.message, "200 - OK, keyword not found");
}

fn json_query(base: &str, path: &str, operator: &str, expected: &str) -> serde_json::Value {
    json!({
        "type": "json-query", "timeout": 5, "url": base,
        "json_path": path, "json_path_operator": operator, "expected_value": expected,
    })
}

#[tokio::test]
async fn json_query_match_passes() {
    let base = serve(None, |_| {
        Reply::new(
            200,
            r#"{"status":"ok","checks":[{"name":"db","latency":12}]}"#,
        )
    })
    .await;
    let ctx = CheckContext::new();

    let equals = run(&definition(json_query(&base, "status", "==", "ok")), &ctx).await;
    assert_eq!(equals.status, Status::Up);
    assert_eq!(equals.message, "JSON query passes (comparing ok == ok)");

    let numeric = run(
        &definition(json_query(&base, "checks[name='db'].latency", "<", "100")),
        &ctx,
    )
    .await;
    assert_eq!(numeric.message, "JSON query passes (comparing 12 < 100)");
}

#[tokio::test]
async fn json_query_mismatch_fails_with_its_own_failure() {
    let base = serve(None, |_| Reply::new(200, r#"{"status":"degraded"}"#)).await;
    let result = run(
        &definition(json_query(&base, "status", "==", "ok")),
        &CheckContext::new(),
    )
    .await;

    assert_eq!(result.status, Status::Down);
    assert_eq!(
        result.message,
        "JSON query does not pass (comparing degraded == ok)"
    );
    assert_eq!(result.failure, Some(Failure::JsonQueryMismatch));
}

#[tokio::test]
async fn json_query_rejects_arrays_like_kuma() {
    let base = serve(None, |_| Reply::new(200, r#"{"items":[1,2,3]}"#)).await;
    let result = run(
        &definition(json_query(&base, "items", "==", "1")),
        &CheckContext::new(),
    )
    .await;

    assert_eq!(result.failure, Some(Failure::Check));
    assert!(result.message.starts_with(
        "Error evaluating JSON query: JSON query returned the array [1,2,3], but a primitive value is required."
    ));
}

#[tokio::test]
async fn unparsable_json_query_is_unsupported() {
    let result = run(
        &definition(json_query("http://127.0.0.1:9", "status ==", "==", "ok")),
        &CheckContext::new(),
    )
    .await;

    assert!(matches!(
        result.failure,
        Some(Failure::Unsupported {
            option: Unsupported::JsonQuery { .. }
        })
    ));
}

#[tokio::test]
async fn upside_down_inverts_the_result() {
    let base = serve(None, |request| match request.path.as_str() {
        "/up" => Reply::new(200, ""),
        _ => Reply::new(500, ""),
    })
    .await;
    let ctx = CheckContext::new();
    let check = |path: &str| {
        definition(json!({
            "type": "http", "timeout": 5, "upside_down": true, "url": format!("{base}{path}"),
        }))
    };

    let passing = run(&check("/up"), &ctx).await;
    assert_eq!(passing.status, Status::Down);
    assert_eq!(passing.message, "Flip UP to DOWN");
    assert_eq!(passing.failure, None);

    let failing = run(&check("/down"), &ctx).await;
    assert_eq!(failing.status, Status::Up);
    assert_eq!(failing.message, "Request failed with status code 500");
}

#[tokio::test]
async fn oauth_token_is_fetched_once_and_sent() {
    let token_requests = Arc::new(AtomicUsize::new(0));
    let counter = token_requests.clone();
    // RFC 6749 form-encodes the client id before base64: "id:x" becomes "id%3Ax".
    let client_basic =
        format!("authorization: basic {}", STANDARD.encode("id%3Ax:secret")).to_lowercase();
    let base = serve(None, move |request| match request.path.as_str() {
        "/token" => {
            counter.fetch_add(1, Ordering::SeqCst);
            assert!(request.head.contains(&client_basic));
            Reply::new(
                200,
                r#"{"access_token":"abc","token_type":"Bearer","expires_in":3600}"#,
            )
            .header("content-type", "application/json")
        }
        _ if request.head.contains("authorization: bearer abc") => Reply::new(200, ""),
        _ => Reply::new(401, ""),
    })
    .await;
    let check = definition(json!({
        "type": "http", "timeout": 5, "url": format!("{base}/api"),
        "auth": {
            "auth_method": "oauth2-cc",
            "oauth_token_url": format!("{base}/token"),
            "oauth_client_id": "id:x",
            "oauth_client_secret": "secret",
        },
    }));
    let ctx = CheckContext::new();

    assert_eq!(run(&check, &ctx).await.status, Status::Up);
    assert_eq!(run(&check, &ctx).await.status, Status::Up);
    assert_eq!(token_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ntlm_is_unsupported() {
    let check = definition(json!({
        "type": "http", "timeout": 5, "url": "http://127.0.0.1:9",
        "auth": { "auth_method": "ntlm", "basic_auth_user": "u", "basic_auth_pass": "p" },
    }));
    let result = run(&check, &CheckContext::new()).await;

    assert_eq!(
        result.failure,
        Some(Failure::Unsupported {
            option: Unsupported::NtlmAuth
        })
    );
}

#[test]
fn run_future_is_send() {
    fn assert_send<T: Send>(_: T) {}
    let definition: v1per_checks::CheckDefinition =
        serde_json::from_str(r#"{"type":"port","hostname":"127.0.0.1","port":1,"timeout":1}"#)
            .unwrap();
    let context = v1per_checks::CheckContext::new();
    assert_send(v1per_checks::run(&definition, &context));
}
