//! Status pages end to end: access per mode, what public responses may contain, hostname
//! routing, the injected boot JSON, the page stream, auto-publication and badges.

mod common;

use std::future::Future;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::fakes::SmtpSink;
use common::{Answer, Frames, HOST, Harness, ORIGIN};
use serde_json::{Value, json};
use tower::ServiceExt;
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{NodeId, PageId};
use v1per_server::db::now_ms;
use v1per_server::events;
use v1per_storage::{Heartbeat, MonitorId, Status};

async fn eventually<F, Fut>(what: &str, holds: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..1_000 {
        if holds().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

/// A GET as a browser on `host` sends it, with an optional cookie.
async fn visit(harness: &Harness, path: &str, host: &str, cookie: Option<&str>) -> Answer {
    let mut request = Request::get(path).header(header::HOST, host);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    harness.send(request.body(Body::empty()).unwrap()).await
}

async fn get(harness: &Harness, path: &str, cookie: Option<&str>) -> Answer {
    visit(harness, path, HOST, cookie).await
}

/// A POST from the page itself.
async fn post(harness: &Harness, path: &str, body: Value) -> Answer {
    let request = Request::post(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json");
    harness
        .send(request.body(Body::from(body.to_string())).unwrap())
        .await
}

/// The boot JSON injected into a page's HTML.
fn boot(answer: &Answer) -> Value {
    let html = String::from_utf8_lossy(&answer.bytes);
    let start = html
        .find(r#"<script id="v1per-page" type="application/json">"#)
        .unwrap_or_else(|| panic!("no boot JSON in {html}"));
    let json = &html[start + 48..];
    serde_json::from_str(&json[..json.find("</script>").unwrap()]).unwrap()
}

/// `name=value` of a `Set-Cookie` header.
fn cookie_of(answer: &Answer) -> String {
    answer
        .header("set-cookie")
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

fn component_names(snapshot: &Value) -> Vec<String> {
    snapshot["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["components"].as_array().unwrap())
        .map(|component| component["name"].as_str().unwrap().to_owned())
        .collect()
}

/// Keys every component of `snapshot` has, merged.
fn component_keys(snapshot: &Value) -> Vec<String> {
    let mut keys: Vec<String> = snapshot["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["components"].as_array().unwrap())
        .flat_map(|component| component.as_object().unwrap().keys().cloned())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// A client tree: `acme` with a page showing `api` (as "API"), the static component "Email"
/// and the internal `worker`, and `globex` beside it with a monitor of its own.
struct Tree {
    harness: Harness,
    admin: String,
    acme: String,
    api: String,
    worker: String,
    globex_db: String,
    page: Value,
}

impl Tree {
    async fn new(harness: Harness) -> Self {
        let admin = harness.session(&harness.admin).await;
        let root = harness.root().await;
        let clients = harness.node(&admin, &root, "clients").await;
        let acme = harness.node(&admin, &clients, "acme").await;
        let globex = harness.node(&admin, &clients, "globex").await;
        let monitor = |node: &str, name: &str, settings: Value| {
            let (harness, admin) = (&harness, &admin);
            let body = json!({ "node": node, "name": name, "interval": 60, "settings": settings });
            async move {
                harness.ok(admin, "monitors.create", body).await["monitor"]["id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            }
        };
        let api = monitor(&acme, "api", json!({ "type": "manual" })).await;
        let worker = monitor(
            &acme,
            "worker",
            json!({ "type": "http", "url": "https://secret-db.invalid/health" }),
        )
        .await;
        let globex_db = monitor(&globex, "globex-db", json!({ "type": "manual" })).await;
        let email = harness
            .ok(
                &admin,
                "components.create",
                json!({ "node": acme, "name": "Email" }),
            )
            .await["component"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let created = harness
            .ok(
                &admin,
                "pages.create",
                json!({ "node": acme, "slug": "acme", "title": "Acme status" }),
            )
            .await;
        let page = harness
            .ok(
                &admin,
                "pages.updateLayout",
                json!({
                    "page": created["page"]["id"],
                    "expected_revision": created["page"]["revision"],
                    "groups": [
                        { "name": "Services", "components": [
                            { "target": { "type": "monitor", "monitor": api }, "display_name": "API" },
                            { "target": { "type": "component", "component": email } },
                        ] },
                        { "name": "Backend", "components": [
                            { "target": { "type": "monitor", "monitor": worker }, "internal": true },
                        ] },
                    ],
                }),
            )
            .await;
        let tree = Self {
            harness,
            admin,
            acme,
            api,
            worker,
            globex_db,
            page,
        };
        tree.access(json!({ "public": true })).await;
        tree
    }

    fn page_id(&self) -> &str {
        self.page["page"]["id"].as_str().unwrap()
    }

    async fn access(&self, access: Value) -> Value {
        let mut body = access;
        body["page"] = json!(self.page_id());
        self.harness
            .ok(&self.admin, "pages.updateAccess", body)
            .await
    }

    async fn set_manual(&self, monitor: &str, status: &str) {
        self.harness
            .ok(
                &self.admin,
                "monitors.setManualStatus",
                json!({ "monitor": monitor, "status": status }),
            )
            .await;
    }
}

/// Each access mode admits exactly its visitors, with exactly the fields of their audience.
#[tokio::test]
async fn every_access_mode_admits_its_audience_with_its_fields() {
    let tree = Tree::new(Harness::new().await).await;
    let harness = &tree.harness;

    let public = boot(&get(harness, "/s/acme", None).await);
    assert_eq!(public["kind"], "page");
    assert_eq!(public["base"], "/s/acme");
    let snapshot = &public["snapshot"];
    assert_eq!(snapshot["audience"], "public");
    assert_eq!(component_names(snapshot), ["API", "Email"]);
    assert_eq!(
        component_keys(snapshot),
        ["bars", "description", "id", "name", "status"]
    );
    assert_eq!(
        snapshot["groups"][0]["components"][0]["bars"]
            .as_array()
            .unwrap()
            .len(),
        90
    );

    let viewer_id = harness.person("viewer").await;
    harness.grant(&viewer_id, &tree.acme, "viewer").await;
    let viewer = harness.session(&viewer_id).await;
    let internal = get(harness, "/s/acme/snapshot.json", Some(&viewer))
        .await
        .json();
    assert_eq!(internal["audience"], "internal");
    assert_eq!(component_names(&internal), ["API", "Email", "worker"]);
    assert!(component_keys(&internal).contains(&"detail".to_owned()));
    let outsider_id = harness.person("outsider").await;
    let globex_viewer = harness.session(&outsider_id).await;
    assert_eq!(
        get(harness, "/s/acme/snapshot.json", Some(&globex_viewer))
            .await
            .json()["audience"],
        "public"
    );

    let sink = SmtpSink::start().await;
    harness
        .ok(
            &tree.admin,
            "instance.updateSmtp",
            json!({ "smtp": { "host": "127.0.0.1", "port": sink.port, "from": "v1per <noreply@example.test>" } }),
        )
        .await;
    harness
        .ok(
            &tree.admin,
            "instance.updateSettings",
            json!({ "name": "Test instance", "public_url": ORIGIN }),
        )
        .await;
    tree.access(json!({
        "public": false,
        "password": { "value": "page secret" },
        "emails": ["guest@client.test"],
        "domains": ["acme.test"],
    }))
    .await;

    let gate = boot(&get(harness, "/s/acme", None).await);
    assert_eq!(gate["kind"], "gate");
    assert_eq!(gate["gate"]["password"], true);
    assert_eq!(gate["gate"]["email"], true);
    assert_eq!(gate["gate"]["sign_in_url"], "/login?redirect=/s/acme");
    assert!(gate.get("snapshot").is_none());
    assert_eq!(
        get(harness, "/s/acme/snapshot.json", None).await.code(),
        "unauthenticated"
    );
    assert_eq!(
        get(harness, "/s/acme/snapshot.json", Some(&viewer))
            .await
            .json()["audience"],
        "internal"
    );

    // Password.
    let wrong = post(
        harness,
        "/s/acme/access/password",
        json!({ "password": "nope" }),
    )
    .await;
    assert_eq!(wrong.code(), "invalid_credentials");
    let right = post(
        harness,
        "/s/acme/access/password",
        json!({ "password": "page secret" }),
    )
    .await;
    assert_eq!(right.status, StatusCode::NO_CONTENT);
    assert!(
        right
            .header("set-cookie")
            .contains("Path=/s/acme; HttpOnly")
    );
    let by_password = cookie_of(&right);
    let snapshot = get(harness, "/s/acme/snapshot.json", Some(&by_password))
        .await
        .json();
    assert_eq!(snapshot["audience"], "public");
    assert_eq!(component_names(&snapshot), ["API", "Email"]);
    assert!(!component_keys(&snapshot).contains(&"detail".to_owned()));
    let foreign = Request::post("/s/acme/access/password")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "https://evil.test")
        .body(Body::from(json!({ "password": "page secret" }).to_string()))
        .unwrap();
    assert_eq!(harness.send(foreign).await.code(), "origin_rejected");

    // Sign-in link: allowed addresses get one, others the same answer and no mail.
    for email in ["stranger@evil.test", "Someone@ACME.test"] {
        let asked = post(harness, "/s/acme/access/email", json!({ "email": email })).await;
        assert_eq!(asked.status, StatusCode::ACCEPTED);
    }
    eventually("the sign-in mail", || async { sink.mails().len() == 1 }).await;
    let mail = &sink.mails()[0];
    assert_eq!(mail.to, ["<someone@acme.test>"]);
    let body = mail.body();
    let link = body
        .split_whitespace()
        .find(|word| word.starts_with("http://v1per.test/s/acme/access/email?code="))
        .unwrap_or_else(|| panic!("no link in {body}"));
    let code = link.rsplit_once("code=").unwrap().1;
    // Opening the link, as a mail scanner does, never uses the code up; only the POST does.
    for _ in 0..2 {
        assert_eq!(
            boot(&get(harness, &link[ORIGIN.len()..], None).await)["kind"],
            "gate"
        );
    }
    let codes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_email_codes")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(codes, 1);
    let redeemed = post(
        harness,
        "/s/acme/access/email/redeem",
        json!({ "code": code }),
    )
    .await;
    assert_eq!(redeemed.status, StatusCode::NO_CONTENT);
    let by_email = cookie_of(&redeemed);
    assert_eq!(
        get(harness, "/s/acme/snapshot.json", Some(&by_email))
            .await
            .json()["audience"],
        "public"
    );
    let again = post(
        harness,
        "/s/acme/access/email/redeem",
        json!({ "code": code }),
    )
    .await;
    assert_eq!(again.code(), "invalid_credentials");

    // Share tokens, by query and by path; expired and revoked ones open nothing.
    let page = tree.page_id();
    let created = harness
        .ok(
            &tree.admin,
            "pages.createShareToken",
            json!({ "page": page, "name": "Client board" }),
        )
        .await;
    let secret = created["secret"].as_str().unwrap();
    let by_query = get(harness, &format!("/s/acme?token={secret}"), None).await;
    assert_eq!(boot(&by_query)["snapshot"]["audience"], "public");
    let by_token = cookie_of(&by_query);
    assert_eq!(
        boot(&get(harness, &format!("/s/acme/t/{secret}"), None).await)["kind"],
        "page"
    );
    assert_eq!(
        get(
            harness,
            &format!("/s/acme/snapshot.json?token={secret}"),
            None
        )
        .await
        .status,
        StatusCode::OK
    );
    let expiring = harness
        .ok(
            &tree.admin,
            "pages.createShareToken",
            json!({ "page": page, "name": "Short", "expires_at": harness.state.clock.now_ms() + 60_000 }),
        )
        .await;
    let expiring_secret = expiring["secret"].as_str().unwrap();
    sqlx::query("UPDATE page_share_tokens SET expires_at = 1 WHERE id = ?")
        .bind(expiring["token"]["id"].as_str().unwrap())
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    assert_eq!(
        get(
            harness,
            &format!("/s/acme/snapshot.json?token={expiring_secret}"),
            None
        )
        .await
        .code(),
        "unauthenticated"
    );
    harness
        .ok(
            &tree.admin,
            "pages.revokeShareToken",
            json!({ "token": created["token"]["id"] }),
        )
        .await;
    assert_eq!(
        get(
            harness,
            &format!("/s/acme/snapshot.json?token={secret}"),
            None
        )
        .await
        .code(),
        "unauthenticated"
    );
    assert_eq!(
        get(harness, "/s/acme/snapshot.json", Some(&by_token))
            .await
            .code(),
        "unauthenticated"
    );

    // A new password signs out the old one; dropping the address its link session.
    tree.access(json!({ "public": false, "password": { "value": "new secret" } }))
        .await;
    for cookie in [&by_password, &by_email] {
        assert_eq!(
            get(harness, "/s/acme/snapshot.json", Some(cookie))
                .await
                .code(),
            "unauthenticated"
        );
    }
}

/// No public response names nodes, monitors, targets, people, internal components, other
/// nodes' data, check messages or draft incidents.
#[tokio::test]
async fn public_responses_carry_nothing_internal() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let tree = Tree::new(harness).await;
    let harness = &tree.harness;
    let monitor = |id: &str| json!({ "type": "monitor", "monitor": id });

    // A cache monitor whose auto-incident stays a draft on this page.
    let cache = harness
        .ok(
            &tree.admin,
            "monitors.create",
            json!({ "node": tree.acme, "name": "cache", "interval": 60, "settings": { "type": "manual" } }),
        )
        .await["monitor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let detail = harness
        .query(
            Some(&tree.admin),
            "pages.get",
            &format!("page={}", tree.page_id()),
        )
        .await
        .json();
    let mut groups = detail["groups"].clone();
    for group in groups.as_array_mut().unwrap() {
        for component in group["components"].as_array_mut().unwrap() {
            component.as_object_mut().unwrap().remove("id");
            component.as_object_mut().unwrap().remove("name");
        }
    }
    groups[0]["components"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "target": monitor(&cache), "display_name": "Cache layer" }));
    harness
        .ok(
            &tree.admin,
            "pages.updateLayout",
            json!({ "page": tree.page_id(), "expected_revision": detail["page"]["revision"], "groups": groups }),
        )
        .await;

    // A manual incident on the page, one on the internal worker only, and one on globex.
    harness
        .ok(
            &tree.admin,
            "incidents.create",
            json!({ "node": tree.acme, "title": "API slow", "message": "Looking into it",
                    "impacts": [{ "target": monitor(&tree.api), "impact": "degraded" }] }),
        )
        .await;
    harness
        .ok(
            &tree.admin,
            "incidents.create",
            json!({ "node": tree.acme, "title": "Worker queue stuck",
                    "impacts": [{ "target": monitor(&tree.worker), "impact": "major_outage" }] }),
        )
        .await;
    let refused = harness
        .run(
            Some(&tree.admin),
            "incidents.create",
            json!({ "node": tree.acme, "title": "x", "impacts": [{ "target": monitor(&tree.globex_db), "impact": "degraded" }] }),
        )
        .await;
    assert_eq!(refused.code(), "invalid_arguments");

    // Auto-incidents: the api one is published by a person, the cache one stays a draft. The
    // check message of the api one names a host.
    tree.set_manual(&tree.api, "down").await;
    tree.set_manual(&cache, "down").await;
    let auto = |monitor: String| {
        let harness = &tree.harness;
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT id FROM incidents WHERE auto_monitor_id = ? AND resolved_at IS NULL",
            )
            .bind(monitor.parse::<i64>().unwrap())
            .fetch_optional(&harness.state.db.reader)
            .await
            .unwrap()
        }
    };
    eventually("both auto-incidents", || async {
        auto(tree.api.clone()).await.is_some() && auto(cache.clone()).await.is_some()
    })
    .await;
    let api_incident = auto(tree.api.clone()).await.unwrap();
    let cache_id = auto(cache.clone()).await.unwrap().to_string();
    sqlx::query("UPDATE incident_updates SET message = 'connect ECONNREFUSED 10.9.8.7:5432' WHERE incident_id = ?")
        .bind(api_incident)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    harness
        .ok(
            &tree.admin,
            "incidents.publish",
            json!({ "incident": api_incident.to_string(), "page": tree.page_id() }),
        )
        .await;

    let snapshot = get(harness, "/s/acme/snapshot.json", None).await.json();
    let titles: Vec<&str> = snapshot["incidents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|incident| incident["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["API is down", "API slow"]);
    assert_eq!(snapshot["status"], "major_outage");
    let generic = &snapshot["incidents"][0]["updates"][0]["message"];
    assert_eq!(generic, "API is not reachable. We are looking into it.");
    assert!(
        !snapshot["incidents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|incident| incident["id"].as_str() == Some(cache_id.as_str()))
    );

    let mut everything = String::new();
    for path in [
        "/s/acme",
        "/s/acme/snapshot.json",
        "/s/acme/badge/status.svg",
        "/s/acme/badge/uptime-24h.svg",
        "/s/acme/badge/uptime-30d.svg",
        "/s/acme/badge/uptime-90d.svg",
    ] {
        let answer = get(harness, path, None).await;
        assert_eq!(answer.status, StatusCode::OK, "{path}");
        everything.push_str(&String::from_utf8_lossy(&answer.bytes));
    }
    let mut frames = Frames::new(
        harness
            .router()
            .oneshot(
                Request::get(format!("/s/acme/events?cursor=page:{}@0", tree.page_id()))
                    .header(header::HOST, HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    );
    for frame in frames.next(2).await {
        everything.push_str(&frame.to_string());
    }
    tree.access(json!({ "public": false })).await;
    everything.push_str(&String::from_utf8_lossy(
        &get(harness, "/s/acme", None).await.bytes,
    ));

    for leak in [
        "\"node\"",
        "\"monitor\"",
        "auto_monitor",
        "\"edited\"",
        "\"author\"",
        "\"revision\"",
        "\"settings\"",
        "\"detail\"",
        "\"probes\"",
        "\"latency\"",
        "\"internal\"",
        "secret-db.invalid",
        "10.9.8.7",
        "globex",
        "worker",
        "Worker queue stuck",
        "cache is down",
        "Admin",
        "admin@example.test",
    ] {
        assert!(
            !everything.contains(leak),
            "a public response contains {leak}"
        );
    }
    background.stop().await;
}

/// A claimed hostname serves its page and its assets, never the admin app or API, and only
/// while claimed. The page JSON is escaped so a title cannot close its script element.
#[tokio::test]
async fn claimed_hostnames_route_to_their_page() {
    let tree = Tree::new(Harness::new().await).await;
    let harness = &tree.harness;
    let page = tree.page_id();
    let detail = harness
        .query(Some(&tree.admin), "pages.get", &format!("page={page}"))
        .await
        .json();
    let mut settings = detail["page"].clone();
    for key in [
        "id",
        "node",
        "access",
        "hostnames",
        "path",
        "revision",
        "created_at",
        "updated_at",
    ] {
        settings.as_object_mut().unwrap().remove(key);
    }
    settings["page"] = json!(page);
    settings["title"] = json!("Acme </script><!-- status");
    harness.ok(&tree.admin, "pages.update", settings).await;

    let claim = |hostname: &str| {
        let body = json!({ "page": page, "hostname": hostname });
        harness.run(Some(&tree.admin), "pages.claimHostname", body)
    };
    assert_eq!(claim("Status.Acme.test").await.status, StatusCode::OK);
    assert_eq!(claim("not a host").await.code(), "invalid_arguments");

    let html = visit(harness, "/", "status.acme.test:8080", None).await;
    assert_eq!(html.status, StatusCode::OK);
    let text = String::from_utf8_lossy(&html.bytes);
    assert_eq!(text.matches("</script>").count(), 1, "{text}");
    assert!(text.contains("<title>Acme &lt;/script&gt;&lt;!-- status</title>"));
    let boot = boot(&html);
    assert_eq!(boot["base"], "");
    assert_eq!(
        boot["snapshot"]["appearance"]["title"],
        "Acme </script><!-- status"
    );
    let snapshot = visit(harness, "/snapshot.json", "status.acme.test", None).await;
    assert_eq!(snapshot.json()["audience"], "public");
    assert_eq!(
        visit(harness, "/assets/app-3f9a1c.js", "status.acme.test", None)
            .await
            .status,
        StatusCode::OK
    );
    for admin_path in [
        "/api/queries/nodes.tree",
        "/login",
        "/s/acme",
        "/config.json",
    ] {
        let answer = visit(harness, admin_path, "status.acme.test", Some(&tree.admin)).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{admin_path}");
    }

    // Another page cannot take the hostname.
    let other = harness
        .ok(
            &tree.admin,
            "pages.create",
            json!({ "node": tree.acme, "slug": "other", "title": "Other" }),
        )
        .await;
    let taken = harness
        .run(
            Some(&tree.admin),
            "pages.claimHostname",
            json!({ "page": other["page"]["id"], "hostname": "status.acme.test" }),
        )
        .await;
    assert_eq!(taken.code(), "invalid_arguments");

    harness
        .ok(
            &tree.admin,
            "pages.releaseHostname",
            json!({ "page": page, "hostname": "status.acme.test" }),
        )
        .await;
    let released = visit(harness, "/", "status.acme.test", None).await;
    assert!(String::from_utf8_lossy(&released.bytes).contains("<div id=app>"));
}

/// A status change reaches an open page stream within the 2 second target; the stream carries
/// no IDs and ends once the visitor loses access.
#[tokio::test]
async fn page_stream_follows_status_and_rechecks_access() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let tree = Tree::new(harness).await;
    let harness = &tree.harness;
    tree.set_manual(&tree.api, "up").await;
    eventually("the first status", || async {
        get(harness, "/s/acme/snapshot.json", None).await.json()["groups"][0]["components"][0]
            ["status"]
            == "operational"
    })
    .await;

    let snapshot = get(harness, "/s/acme/snapshot.json", None).await.json();
    let cursor = snapshot["cursor"].as_str().unwrap();
    let response = harness
        .router()
        .oneshot(
            Request::get(format!("/s/acme/events?cursor={cursor}"))
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut frames = Frames::new(response);
    assert_eq!(frames.next(1).await[0]["kind"], "subscribed");

    // Each frame makes the page read its snapshot again, as the status page app does.
    let changed = Instant::now();
    tree.set_manual(&tree.api, "down").await;
    loop {
        let frame = &frames.next(1).await[0];
        assert_eq!(frame["kind"], "events");
        for event in frame["events"].as_array().unwrap() {
            let mut keys: Vec<&String> = event.as_object().unwrap().keys().collect();
            keys.sort();
            assert_eq!(keys, ["kind", "seq"]);
        }
        let after = get(harness, "/s/acme/snapshot.json", None).await.json();
        if after["groups"][0]["components"][0]["status"] == "major_outage" {
            assert_eq!(after["status"], "major_outage");
            break;
        }
    }
    assert!(
        changed.elapsed() < Duration::from_secs(2),
        "{:?}",
        changed.elapsed()
    );

    tree.access(json!({ "public": false })).await;
    let frame = loop {
        let frame = frames.next(1).await.remove(0);
        if frame["kind"] != "events" {
            break frame;
        }
    };
    assert_eq!(frame["kind"], "resync");
    assert_eq!(frame["reason"], "unavailable");
    assert!(frames.ends().await);
    background.stop().await;
}

/// `auto_publish` publishes an auto-incident once its grace ran out; `draft` pages wait for a
/// person. Titles follow the page's locale and display name until a person edits them.
#[tokio::test]
async fn auto_incidents_publish_after_grace_with_localized_titles() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let tree = Tree::new(harness).await;
    let harness = &tree.harness;
    let detail = harness
        .query(
            Some(&tree.admin),
            "pages.get",
            &format!("page={}", tree.page_id()),
        )
        .await
        .json();
    let mut settings = detail["page"].clone();
    for key in [
        "id",
        "node",
        "access",
        "hostnames",
        "path",
        "revision",
        "created_at",
        "updated_at",
    ] {
        settings.as_object_mut().unwrap().remove(key);
    }
    settings["page"] = json!(tree.page_id());
    settings["default_locale"] = json!("de");
    settings["locales"] = json!(["en"]);
    settings["auto_incidents"] = json!("auto_publish");
    settings["grace_minutes"] = json!(5);
    harness.ok(&tree.admin, "pages.update", settings).await;
    let draft = harness
        .ok(
            &tree.admin,
            "pages.create",
            json!({ "node": tree.acme, "slug": "draft", "title": "Draft" }),
        )
        .await;
    harness
        .ok(
            &tree.admin,
            "pages.updateLayout",
            json!({ "page": draft["page"]["id"], "expected_revision": draft["page"]["revision"],
                    "groups": [{ "components": [{ "target": { "type": "monitor", "monitor": tree.api } }] }] }),
        )
        .await;
    harness
        .ok(
            &tree.admin,
            "pages.updateAccess",
            json!({ "page": draft["page"]["id"], "public": true }),
        )
        .await;

    tree.set_manual(&tree.api, "down").await;
    let incidents = |path: &'static str| async move {
        get(harness, path, None).await.json()["incidents"]
            .as_array()
            .unwrap()
            .clone()
    };
    eventually("the auto-incident", || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM incident_auto_publications a
             JOIN incidents i ON i.id = a.incident_id WHERE i.auto_monitor_id = ?",
        )
        .bind(tree.api.parse::<i64>().unwrap())
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap()
            == 1
    })
    .await;
    assert!(incidents("/s/acme/snapshot.json").await.is_empty());

    harness.state.clock.advance(Duration::from_secs(5 * 60));
    eventually("the publication", || async {
        incidents("/s/acme/snapshot.json").await.len() == 1
    })
    .await;
    let shown = &incidents("/s/acme/snapshot.json").await[0];
    assert_eq!(shown["title"], "API ist ausgefallen");
    assert_eq!(
        shown["updates"][0]["message"],
        "API ist nicht erreichbar. Wir sehen uns das an."
    );
    let english = get(harness, "/s/acme/snapshot.json?locale=en", None)
        .await
        .json();
    assert_eq!(english["incidents"][0]["title"], "API is down");
    assert!(incidents("/s/draft/snapshot.json").await.is_empty());

    let incident = shown["id"].as_str().unwrap();
    let publications = harness
        .ok(
            &tree.admin,
            "incidents.publish",
            json!({ "incident": incident, "page": draft["page"]["id"] }),
        )
        .await;
    assert_eq!(publications["pages"].as_array().unwrap().len(), 2);
    let on_draft = &incidents("/s/draft/snapshot.json").await[0];
    assert_eq!(on_draft["title"], "api is down");

    harness
        .ok(
            &tree.admin,
            "incidents.update",
            json!({ "incident": incident, "title": "Checkout outage",
                    "affected": [{ "type": "monitor", "monitor": tree.api }] }),
        )
        .await;
    assert_eq!(
        incidents("/s/acme/snapshot.json").await[0]["title"],
        "Checkout outage"
    );
    background.stop().await;
}

/// Badges exist for public pages and share tokens, per page or per public component.
#[tokio::test]
async fn badges_show_status_and_uptime_for_public_pages_and_tokens() {
    let tree = Tree::new(Harness::new().await).await;
    let harness = &tree.harness;
    let badge = get(harness, "/s/acme/badge/status.svg", None).await;
    assert_eq!(badge.header("content-type"), "image/svg+xml");
    let svg = String::from_utf8_lossy(&badge.bytes);
    assert!(svg.starts_with("<svg") && svg.contains(">status<"), "{svg}");
    let uptime = get(harness, "/s/acme/badge/uptime-90d.svg", None).await;
    assert!(String::from_utf8_lossy(&uptime.bytes).contains("uptime 90d"));
    assert_eq!(
        get(harness, "/s/acme/badge/other.svg", None).await.status,
        StatusCode::NOT_FOUND
    );

    let components = &tree.page["groups"];
    let api = components[0]["components"][0]["id"].as_str().unwrap();
    let worker = components[1]["components"][0]["id"].as_str().unwrap();
    let path = |id: &str| format!("/s/acme/badge/status.svg?component={id}");
    assert_eq!(get(harness, &path(api), None).await.status, StatusCode::OK);
    assert_eq!(
        get(harness, &path(worker), None).await.status,
        StatusCode::NOT_FOUND
    );

    tree.access(json!({ "public": false })).await;
    assert_eq!(
        get(harness, "/s/acme/badge/status.svg", None).await.status,
        StatusCode::NOT_FOUND
    );
    let token = harness
        .ok(
            &tree.admin,
            "pages.createShareToken",
            json!({ "page": tree.page_id(), "name": "Readme" }),
        )
        .await;
    let secret = token["secret"].as_str().unwrap();
    let with_token = get(
        harness,
        &format!("/s/acme/badge/status.svg?token={secret}"),
        None,
    )
    .await;
    assert_eq!(with_token.status, StatusCode::OK);
    assert!(with_token.header("cache-control").starts_with("private"));
}

/// Today's bar counts what was checked today; nothing is shown before a monitor or a static
/// component existed, and a manual monitor set right after creation is not down for a moment.
#[tokio::test]
async fn bars_start_when_their_component_does() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let tree = Tree::new(harness).await;
    let harness = &tree.harness;
    tree.set_manual(&tree.api, "up").await;
    let monitor = format!("monitor={}", tree.api);
    eventually("the first status", || async {
        harness
            .query(Some(&tree.admin), "monitors.get", &monitor)
            .await
            .json()["state"]["status"]
            == "up"
    })
    .await;
    // The snapshot cache would keep bars read before the beat was readable.
    harness.state.storage.drain().await;

    let snapshot = get(harness, "/s/acme/snapshot.json", Some(&tree.admin))
        .await
        .json();
    let components = &snapshot["groups"][0]["components"];
    let (api, email) = (&components[0], &components[1]);
    assert_eq!(api["bars"][89]["status"], "operational");
    assert_eq!(api["uptime_90d"], 1.0);
    assert_eq!(email["bars"][89]["status"], "operational");
    for component in [api, email] {
        let before: Vec<&Value> = component["bars"].as_array().unwrap()[..89]
            .iter()
            .filter(|bar| bar.get("status").is_some())
            .collect();
        assert!(before.is_empty(), "{}: {before:?}", component["name"]);
    }
    background.stop().await;
}

/// A new monitor's response times show per minute until there are two hourly points.
#[tokio::test]
async fn a_new_monitor_has_response_times_within_minutes() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let cron = harness
        .ok(
            &admin,
            "monitors.create",
            json!({ "node": root, "name": "cron", "interval": 60,
                    "settings": { "type": "push", "push_token": "cron-token-0001" } }),
        )
        .await["monitor"]["id"]
        .clone();
    let page = harness
        .ok(
            &admin,
            "pages.create",
            json!({ "node": root, "slug": "ops", "title": "Ops" }),
        )
        .await;
    harness
        .ok(
            &admin,
            "pages.updateLayout",
            json!({
                "page": page["page"]["id"],
                "expected_revision": page["page"]["revision"],
                "groups": [{ "name": "Jobs", "components": [
                    { "target": { "type": "monitor", "monitor": cron } },
                ] }],
            }),
        )
        .await;
    for ping in [12, 20] {
        let pushed = harness
            .get(&format!("/api/push/cron-token-0001?status=up&ping={ping}"))
            .await;
        assert_eq!(pushed.status, StatusCode::OK);
        harness.state.clock.advance(Duration::from_secs(60));
    }
    harness.state.storage.drain().await;
    let snapshot = get(&harness, "/s/ops/snapshot.json", Some(&admin))
        .await
        .json();
    let latency = &snapshot["groups"][0]["components"][0]["detail"]["latency"];
    let averages: Vec<&Value> = latency
        .as_array()
        .unwrap()
        .iter()
        .map(|point| &point["avg_ms"])
        .collect();
    assert_eq!(averages, [12.0, 20.0]);
    background.stop().await;
}

/// `incidents.json` pages through every incident the audience may see, newest first.
#[tokio::test]
async fn incident_history_pages_by_cursor() {
    let tree = Tree::new(Harness::manual().await).await;
    let harness = &tree.harness;
    let monitor = |id: &str| json!({ "type": "monitor", "monitor": id });
    for (title, target) in [
        ("First", &tree.api),
        ("Worker", &tree.worker),
        ("Second", &tree.api),
        ("Third", &tree.api),
    ] {
        harness
            .ok(
                &tree.admin,
                "incidents.create",
                json!({ "node": tree.acme, "title": title,
                        "impacts": [{ "target": monitor(target), "impact": "degraded" }] }),
            )
            .await;
        harness.state.clock.advance(Duration::from_secs(1));
    }
    let titles = |page: &Value| -> Vec<String> {
        page["incidents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|incident| incident["title"].as_str().unwrap().to_owned())
            .collect()
    };

    let first = get(harness, "/s/acme/incidents.json?limit=2", None)
        .await
        .json();
    assert_eq!(titles(&first), ["Third", "Second"]);
    let cursor = first["next_cursor"].as_str().unwrap();
    let rest = get(
        harness,
        &format!("/s/acme/incidents.json?limit=2&cursor={cursor}"),
        None,
    )
    .await
    .json();
    assert_eq!(titles(&rest), ["First"]);
    assert!(rest.get("next_cursor").is_none());

    let internal = get(harness, "/s/acme/incidents.json", Some(&tree.admin))
        .await
        .json();
    assert_eq!(titles(&internal), ["Third", "Second", "Worker", "First"]);
    for query in ["limit=0", "limit=101", "cursor=x"] {
        let refused = get(harness, &format!("/s/acme/incidents.json?{query}"), None).await;
        assert_eq!(refused.code(), "invalid_arguments", "{query}");
    }
    tree.access(json!({ "public": false })).await;
    let closed = get(harness, "/s/acme/incidents.json", None).await;
    assert_eq!(closed.code(), "unauthenticated");
}

/// The pages list names the restricted access modes in use, and `page.*` events on a node's
/// `pages:<id>` topic name their page.
#[tokio::test]
async fn the_list_names_access_modes_and_events_name_their_page() {
    let tree = Tree::new(Harness::new().await).await;
    let harness = &tree.harness;
    let modes = || async {
        harness
            .query(
                Some(&tree.admin),
                "pages.list",
                &format!("node={}", tree.acme),
            )
            .await
            .json()["pages"][0]["access_modes"]
            .clone()
    };
    assert_eq!(modes().await, json!([]));
    tree.access(json!({ "public": false, "password": { "value": "open sesame" }, "domains": ["acme.test"] }))
        .await;
    assert_eq!(modes().await, json!(["password", "email"]));
    harness
        .ok(
            &tree.admin,
            "pages.createShareToken",
            json!({ "page": tree.page_id(), "name": "Wall screen" }),
        )
        .await;
    assert_eq!(modes().await, json!(["password", "email", "token"]));

    let node = NodeId::new(tree.acme.parse().unwrap());
    let page = PageId::new(tree.page_id().parse().unwrap());
    let mut conn = harness.state.db.reader.acquire().await.unwrap();
    let events = events::after(&mut conn, &Topic::pages(node), 0, 100)
        .await
        .unwrap();
    assert!(events.iter().any(|event| event.kind == "page.created"));
    for event in events
        .iter()
        .filter(|event| event.kind.starts_with("page."))
    {
        assert_eq!(event.page, Some(page), "{}", event.kind);
    }
}

/// A heartbeat that becomes readable shows in the next snapshot at once, though nothing moved
/// the page's topic.
#[tokio::test]
async fn snapshots_show_a_newly_readable_beat_at_once() {
    let tree = Tree::new(Harness::new().await).await;
    let harness = &tree.harness;
    let today = || async {
        get(harness, "/s/acme/snapshot.json", None).await.json()["groups"][0]["components"][0]
            ["bars"][89]
            .clone()
    };
    assert!(today().await.get("status").is_none());
    let page = Topic::page(PageId::new(tree.page_id().parse().unwrap()));
    let head = || async {
        let mut conn = harness.state.db.reader.acquire().await.unwrap();
        events::head(&mut conn, &page).await.unwrap()
    };
    let before = head().await;

    let storage = &harness.state.storage;
    let beat = Heartbeat {
        monitor: MonitorId(tree.api.parse().unwrap()),
        probe: None,
        time_ms: now_ms(),
        status: Status::Up,
        latency_ms: Some(12),
        message: String::new(),
        retries: 0,
        http_status: None,
        cert_expiry_ms: None,
    };
    let admitted = storage.admit(None, &[beat]).await.unwrap();
    storage
        .watermark()
        .wait_for(|w| w.sequence >= admitted.sequence)
        .await
        .unwrap();
    assert_eq!(today().await["status"], "operational");
    assert_eq!(head().await, before);
}
