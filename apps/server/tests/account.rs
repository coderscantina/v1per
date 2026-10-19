//! Account security over HTTP: second factors, recovery codes, the node 2FA requirement,
//! password change, reauthentication and scoped API keys.

mod common;

use axum::http::StatusCode;
use common::{Answer, Harness, PASSWORD};
use serde_json::{Value, json};
use v1per_protocol::ids::UserId;
use v1per_server::db::now_ms;
use v1per_server::identity::totp;

const NEW_PASSWORD: &str = "a much longer new password";

/// `name=value` of a cookie the answer sets, unless it clears it.
fn set_cookie(answer: &Answer, name: &str) -> Option<String> {
    answer
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find(|pair| pair.starts_with(&format!("{name}=")) && pair.len() > name.len() + 1)
        .map(str::to_owned)
}

async fn login(harness: &Harness, email: &str, password: &str) -> Answer {
    harness
        .run(
            None,
            "auth.login",
            json!({ "email": email, "password": password }),
        )
        .await
}

fn base32_decode(input: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut buffer, mut bits, mut out) = (0u32, 0, Vec::new());
    for c in input.bytes() {
        let value = ALPHABET.iter().position(|a| *a == c).unwrap() as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

/// A code one step ahead, which stays acceptable after the current step was spent.
fn next_code(secret: &[u8]) -> String {
    totp::code(secret, totp::step(now_ms()) + 1)
}

/// Enrolls the person behind `cookie` through the API. Returns the TOTP secret and the
/// recovery codes.
async fn enroll(harness: &Harness, cookie: &str) -> (Vec<u8>, Vec<String>) {
    let begin = harness.ok(cookie, "twoFactor.begin", json!({})).await;
    assert!(
        begin["secret"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/Test%20instance:")
    );
    let secret = base32_decode(begin["secret"]["key"].as_str().unwrap());
    let code = totp::code(&secret, totp::step(now_ms()));
    let confirm = harness
        .ok(cookie, "twoFactor.confirm", json!({ "code": code }))
        .await;
    let codes = confirm["secret"]
        .as_array()
        .unwrap()
        .iter()
        .map(|code| code.as_str().unwrap().to_owned())
        .collect();
    (secret, codes)
}

async fn status(harness: &Harness, cookie: &str, name: &str, query: &str) -> StatusCode {
    harness.query(Some(cookie), name, query).await.status
}

/// A correct password of an enrolled account yields a challenge, never a session, and the
/// second factor turns it into one. Codes are accepted once.
#[tokio::test]
async fn enrolled_login_needs_the_second_factor() {
    let harness = Harness::new().await;
    let (secret, _) = enroll(&harness, &harness.session(&harness.member).await).await;

    let password = login(&harness, "member@example.test", PASSWORD).await;
    assert_eq!(password.json()["status"], "second_factor_required");
    assert!(set_cookie(&password, "v1per_session").is_none());
    let challenge = set_cookie(&password, "v1per_challenge").unwrap();
    assert_eq!(
        harness
            .query(Some(&challenge), "nodes.tree", "")
            .await
            .code(),
        "unauthenticated"
    );

    let wrong = harness
        .run(
            Some(&challenge),
            "auth.verifySecondFactor",
            json!({ "kind": "totp", "code": "000000" }),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "second_factor_invalid");

    let code = next_code(&secret);
    let verified = harness
        .run(
            Some(&challenge),
            "auth.verifySecondFactor",
            json!({ "kind": "totp", "code": code }),
        )
        .await;
    assert_eq!(
        verified.json()["status"],
        "signed_in",
        "{}",
        verified.json()
    );
    let session = set_cookie(&verified, "v1per_session").unwrap();
    assert_eq!(
        status(&harness, &session, "nodes.tree", "").await,
        StatusCode::OK
    );

    let again = login(&harness, "member@example.test", PASSWORD).await;
    let challenge = set_cookie(&again, "v1per_challenge").unwrap();
    let replayed = harness
        .run(
            Some(&challenge),
            "auth.verifySecondFactor",
            json!({ "kind": "totp", "code": code }),
        )
        .await;
    assert_eq!(replayed.code(), "second_factor_invalid");
}

/// After 5 wrong codes the challenge ends, even for a right one. Guesses sent at the same time
/// cannot pass the limit.
#[tokio::test(flavor = "multi_thread")]
async fn a_challenge_ends_after_five_wrong_codes() {
    let harness = std::sync::Arc::new(Harness::new().await);
    let (secret, _) = enroll(&harness, &harness.session(&harness.member).await).await;
    let challenge = set_cookie(
        &login(&harness, "member@example.test", PASSWORD).await,
        "v1per_challenge",
    )
    .unwrap();

    let guesses: Vec<_> = (0..8)
        .map(|_| {
            let harness = std::sync::Arc::clone(&harness);
            let challenge = challenge.clone();
            tokio::spawn(async move {
                let wrong = json!({ "kind": "totp", "code": "000000" });
                harness
                    .run(Some(&challenge), "auth.verifySecondFactor", wrong)
                    .await
                    .code()
            })
        })
        .collect();
    let mut codes = Vec::new();
    for guess in guesses {
        codes.push(guess.await.unwrap());
    }
    codes.sort();
    assert_eq!(
        codes,
        [vec!["second_factor_invalid"; 5], vec!["unauthenticated"; 3]].concat()
    );
    let right = harness
        .run(
            Some(&challenge),
            "auth.verifySecondFactor",
            json!({ "kind": "totp", "code": next_code(&secret) }),
        )
        .await;
    assert_eq!(right.code(), "unauthenticated");
}

/// A recovery code signs in once, typed in any case and with spaces.
#[tokio::test]
async fn a_recovery_code_works_once() {
    let harness = Harness::new().await;
    let (_, codes) = enroll(&harness, &harness.session(&harness.member).await).await;
    let typed = codes[0].replace('-', " ").to_lowercase();

    let challenge = set_cookie(
        &login(&harness, "member@example.test", PASSWORD).await,
        "v1per_challenge",
    )
    .unwrap();
    let first = harness
        .run(
            Some(&challenge),
            "auth.verifySecondFactor",
            json!({ "kind": "recovery_code", "code": typed }),
        )
        .await;
    assert_eq!(first.json()["status"], "signed_in", "{}", first.json());
    let session = set_cookie(&first, "v1per_session").unwrap();
    let left = harness
        .query(Some(&session), "twoFactor.status", "")
        .await
        .json();
    assert_eq!(left["recovery_codes_left"], 9);

    let challenge = set_cookie(
        &login(&harness, "member@example.test", PASSWORD).await,
        "v1per_challenge",
    )
    .unwrap();
    let second = harness
        .run(
            Some(&challenge),
            "auth.verifySecondFactor",
            json!({ "kind": "recovery_code", "code": codes[0] }),
        )
        .await;
    assert_eq!(second.code(), "second_factor_invalid");
}

/// A node's requirement reaches everyone holding a grant on it or below, at their next request
/// and without a grace period; an enrollment-only session can only enroll, which makes it full.
/// It does not reach a grant elsewhere in the tree.
#[tokio::test]
async fn a_node_requirement_reaches_grants_below_it() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let autohaus = harness.node(&admin, &root, "autohaus").await;
    let schmidt = harness.node(&admin, &autohaus, "schmidt").await;
    let products = harness.node(&admin, &root, "products").await;
    harness.grant(&harness.member, &schmidt, "member").await;
    let elsewhere = harness.person("elsewhere").await;
    harness.grant(&elsewhere, &products, "admin").await;
    let session = harness.session(&harness.member).await;

    harness
        .ok(
            &admin,
            "nodes.requireTwoFactor",
            json!({ "node": autohaus, "required": true }),
        )
        .await;
    let refused = harness.query(Some(&session), "nodes.tree", "").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.code(), "second_factor_enrollment_required");
    let state = harness
        .query(Some(&session), "auth.session", "")
        .await
        .json();
    assert_eq!(state["enrollment_only"], true);
    assert_eq!(
        login(&harness, "member@example.test", PASSWORD)
            .await
            .json()["status"],
        "enrollment_required"
    );
    let other = harness.session(&elsewhere).await;
    assert_eq!(
        status(&harness, &other, "nodes.tree", "").await,
        StatusCode::OK
    );

    enroll(&harness, &session).await;
    assert_eq!(
        status(&harness, &session, "nodes.tree", "").await,
        StatusCode::OK
    );
    let detail = harness
        .query(Some(&session), "nodes.get", &format!("node={schmidt}"))
        .await
        .json();
    assert_eq!(detail["node"]["two_factor_inherited"], true);
}

/// A password change checks the current password and ends every other session.
#[tokio::test]
async fn a_password_change_needs_the_current_one_and_ends_other_sessions() {
    let harness = Harness::new().await;
    let other = harness.session(&harness.member).await;
    let current = harness.session(&harness.member).await;

    let wrong = harness
        .run(
            Some(&current),
            "account.changePassword",
            json!({ "current_password": "not it at all", "new_password": NEW_PASSWORD }),
        )
        .await;
    assert_eq!(wrong.code(), "invalid_credentials");
    let short = harness
        .run(
            Some(&current),
            "account.changePassword",
            json!({ "current_password": PASSWORD, "new_password": "short" }),
        )
        .await;
    assert_eq!(
        short.json()["errors"]["new_password"][0]["code"],
        "too_short"
    );
    harness
        .ok(
            &current,
            "account.changePassword",
            json!({ "current_password": PASSWORD, "new_password": NEW_PASSWORD }),
        )
        .await;

    assert_eq!(
        harness.query(Some(&other), "account.get", "").await.code(),
        "unauthenticated"
    );
    assert_eq!(
        status(&harness, &current, "account.get", "").await,
        StatusCode::OK
    );
    assert_eq!(
        login(&harness, "member@example.test", PASSWORD)
            .await
            .code(),
        "invalid_credentials"
    );
    assert_eq!(
        login(&harness, "member@example.test", NEW_PASSWORD)
            .await
            .json()["status"],
        "signed_in"
    );
}

/// Reauthentication checks the password, and the second factor when enrolled; sensitive
/// commands pass only within its window.
#[tokio::test]
async fn reauthentication_needs_the_second_factor_when_enrolled() {
    let harness = Harness::new().await;
    let session = harness.session(&harness.member).await;
    let (_, codes) = enroll(&harness, &session).await;
    sqlx::query("UPDATE sessions SET reauthenticated_at = NULL")
        .execute(&harness.state.db.writer)
        .await
        .unwrap();

    let refused = harness
        .run(
            Some(&session),
            "twoFactor.regenerateRecoveryCodes",
            json!({}),
        )
        .await;
    assert_eq!(refused.code(), "reauthentication_required");
    let password_only = harness
        .run(
            Some(&session),
            "auth.reauthenticate",
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(password_only.code(), "second_factor_required");
    let wrong = harness
        .run(
            Some(&session),
            "auth.reauthenticate",
            json!({ "password": PASSWORD, "second_factor": { "kind": "totp", "code": "000000" } }),
        )
        .await;
    assert_eq!(wrong.code(), "second_factor_invalid");

    let passed = harness
        .ok(&session, "auth.reauthenticate", json!({ "password": PASSWORD, "second_factor": { "kind": "recovery_code", "code": codes[1] } }))
        .await;
    assert!(passed["reauthenticated_until"].as_i64().unwrap() > now_ms());
    let fresh = harness
        .ok(&session, "twoFactor.regenerateRecoveryCodes", json!({}))
        .await;
    assert_eq!(fresh["secret"].as_array().unwrap().len(), 10);
}

/// Creates an API key of `owner` on `node`.
async fn key(harness: &Harness, owner: &UserId, node: &str, capabilities: Value) -> Answer {
    let cookie = harness.session(owner).await;
    harness
        .run(
            Some(&cookie),
            "apiKeys.create",
            json!({ "name": "ci", "node": node, "capabilities": capabilities }),
        )
        .await
}

/// A key secret is disclosed once; a replay returns the same key without it, and stored
/// answers never hold it. Creating one needs a fresh login.
#[tokio::test]
async fn key_secrets_are_withheld_on_replay() {
    let harness = Harness::new().await;
    let root = harness.root().await;
    let stale = harness.session_reauthenticated(&harness.admin, None).await;
    let body = json!({ "idempotency_key": "K6", "name": "ci", "node": root, "capabilities": ["node.read"] });
    let refused = harness
        .command(Some(&stale), "apiKeys.create", body.clone())
        .await;
    assert_eq!(refused.code(), "reauthentication_required");

    let admin = harness.session(&harness.admin).await;
    let first = harness
        .command(Some(&admin), "apiKeys.create", body.clone())
        .await
        .json();
    let secret = first["secret"].as_str().unwrap().to_owned();
    let replay = harness
        .command(Some(&admin), "apiKeys.create", body)
        .await
        .json();
    assert_eq!(replay["key"]["id"], first["key"]["id"]);
    assert_eq!(
        (&replay["secret"], &replay["secret_withheld"]),
        (&Value::Null, &json!(true))
    );

    let keys = harness.query(Some(&admin), "apiKeys.list", "").await.json();
    assert_eq!(keys["keys"].as_array().unwrap().len(), 1);
    assert!(!keys.to_string().contains(&secret));
    let stored: Vec<String> = sqlx::query_scalar("SELECT response FROM idempotency_keys")
        .fetch_all(&harness.state.db.reader)
        .await
        .unwrap();
    assert!(stored.iter().all(|response| !response.contains(&secret)));
}

/// A key never holds more than its owner on its node, nor what only people hold.
#[tokio::test]
async fn key_grants_never_exceed_the_owner() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let mine = harness.node(&admin, &root, "mine").await;
    let theirs = harness.node(&admin, &root, "theirs").await;
    harness.grant(&harness.member, &mine, "member").await;
    let member = &harness.member;

    assert_eq!(
        key(
            &harness,
            member,
            &mine,
            json!(["node.read", "monitors.manage"])
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        key(&harness, member, &mine, json!(["pages.manage"]))
            .await
            .code(),
        "grant_exceeds_principal"
    );
    assert_eq!(
        key(&harness, &harness.admin, &mine, json!(["members.manage"]))
            .await
            .code(),
        "grant_exceeds_principal"
    );
    assert_eq!(
        key(&harness, member, &theirs, json!(["node.read"]))
            .await
            .code(),
        "not_found"
    );
    assert_eq!(
        key(&harness, member, &mine, json!(["no.such"]))
            .await
            .code(),
        "malformed_request"
    );
    assert_eq!(
        key(&harness, member, &mine, json!([])).await.code(),
        "invalid_arguments"
    );
}

/// A key reaches its node's subtree with its capabilities, nothing above or beside it, and no
/// operation outside the key operations. Revoked, it answers like an unknown key.
#[tokio::test]
async fn a_key_reaches_only_its_subtree_until_revoked() {
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let autohaus = harness.node(&admin, &root, "autohaus").await;
    let schmidt = harness.node(&admin, &autohaus, "schmidt").await;
    let sibling = harness.node(&admin, &root, "kaessbohrer").await;
    let created = key(&harness, &harness.admin, &autohaus, json!(["node.read"]))
        .await
        .json();
    let bearer = format!("Bearer {}", created["secret"].as_str().unwrap());

    let tree = harness.query_with(&bearer, "nodes.tree", "").await.json();
    let names: Vec<Value> = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].clone())
        .collect();
    assert_eq!(names, [Value::Null, json!("autohaus"), json!("schmidt")]);
    assert_eq!(tree["nodes"][1]["capabilities"], json!(["node.read"]));
    assert_eq!(
        harness
            .query_with(&bearer, "nodes.get", &format!("node={schmidt}"))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        harness
            .query_with(&bearer, "nodes.get", &format!("node={sibling}"))
            .await
            .code(),
        "not_found"
    );
    assert_eq!(
        harness
            .query_with(&bearer, "nodes.get", &format!("node={root}"))
            .await
            .code(),
        "not_found"
    );
    let rename = harness
        .command_with(
            &bearer,
            "nodes.rename",
            json!({ "node": schmidt, "name": "x" }),
        )
        .await;
    assert_eq!(rename.code(), "forbidden");
    assert_eq!(
        harness.query_with(&bearer, "account.get", "").await.code(),
        "operation_not_granted"
    );

    harness
        .ok(
            &admin,
            "apiKeys.revoke",
            json!({ "key": created["key"]["id"] }),
        )
        .await;
    let revoked = harness.query_with(&bearer, "nodes.tree", "").await;
    let unknown = harness.query_with("Bearer unknown", "nodes.tree", "").await;
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED);
    assert_eq!(revoked.bytes, unknown.bytes);
}

/// Preferences follow the person, not the browser: saved by one session, read by another.
#[tokio::test]
async fn preferences_are_kept_per_person() {
    let harness = Harness::new().await;
    let first = harness.session(&harness.member).await;
    let second = harness.session(&harness.member).await;
    assert_eq!(
        harness
            .query(Some(&second), "account.preferences", "")
            .await
            .json()["accent"],
        Value::Null
    );

    harness
        .ok(
            &first,
            "account.updatePreferences",
            json!({ "accent": "volt" }),
        )
        .await;
    assert_eq!(
        harness
            .query(Some(&second), "account.preferences", "")
            .await
            .json()["accent"],
        "volt"
    );
    let unknown = harness
        .run(
            Some(&first),
            "account.updatePreferences",
            json!({ "accent": "beige" }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
}
