//! Page subscriptions end to end: double opt-in by email, the restricted page rule, component
//! selections, unsubscribing, webhook deliveries, the feeds and the maintenance calendar, and
//! the setting that switches all of it.

mod common;

use std::future::Future;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::fakes::{Discord, Mail, SmtpSink};
use common::{Answer, HOST, Harness, ORIGIN};
use serde_json::{Value, json};

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

async fn get(harness: &Harness, path: &str) -> Answer {
    harness.get(path).await
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

/// The secret in the first link of `mail` that starts with `prefix`.
fn token_in(mail: &Mail, prefix: &str) -> String {
    let body = mail.body();
    let link = body
        .split_whitespace()
        .find(|word| word.starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix} link in {body}"));
    let token = link.split_once("token=").unwrap().1;
    token.split('&').next().unwrap().to_owned()
}

/// A page `acme` showing the manual monitor `api` (as "API"), the static component "Email" and
/// the internal monitor `worker`, with subscriptions on, a mail server and a public URL.
struct Page {
    harness: Harness,
    admin: String,
    node: String,
    api: String,
    worker: String,
    /// The static component "Email".
    email: String,
    id: String,
    /// Page component IDs of "API" and "Email".
    components: [String; 2],
    sink: SmtpSink,
}

impl Page {
    async fn new(harness: Harness) -> Self {
        let admin = harness.session(&harness.admin).await;
        let root = harness.root().await;
        let node = harness.node(&admin, &root, "acme").await;
        let monitor = |name: &str, settings: Value| {
            let body = json!({ "node": node, "name": name, "interval": 60, "settings": settings });
            let (harness, admin) = (&harness, &admin);
            async move {
                harness.ok(admin, "monitors.create", body).await["monitor"]["id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            }
        };
        let api = monitor("api", json!({ "type": "manual" })).await;
        let worker = monitor(
            "worker",
            json!({ "type": "http", "url": "https://secret-db.invalid/health" }),
        )
        .await;
        let email = harness
            .ok(
                &admin,
                "components.create",
                json!({ "node": node, "name": "Email" }),
            )
            .await["component"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let created = harness
            .ok(
                &admin,
                "pages.create",
                json!({ "node": node, "slug": "acme", "title": "Acme status" }),
            )
            .await;
        let id = created["page"]["id"].as_str().unwrap().to_owned();
        let laid = harness
            .ok(
                &admin,
                "pages.updateLayout",
                json!({
                    "page": id,
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
        let shown = &laid["groups"][0]["components"];
        let components = [
            shown[0]["id"].as_str().unwrap().to_owned(),
            shown[1]["id"].as_str().unwrap().to_owned(),
        ];
        harness
            .ok(
                &admin,
                "pages.updateAccess",
                json!({ "page": id, "public": true }),
            )
            .await;
        let sink = SmtpSink::start().await;
        harness
            .ok(
                &admin,
                "instance.updateSmtp",
                json!({ "smtp": { "host": "127.0.0.1", "port": sink.port, "from": "V1Per <noreply@example.test>" } }),
            )
            .await;
        harness
            .ok(
                &admin,
                "instance.updateSettings",
                json!({ "name": "Test instance", "public_url": ORIGIN }),
            )
            .await;
        let page = Self {
            harness,
            admin,
            node,
            api,
            worker,
            email,
            id,
            components,
            sink,
        };
        page.subscriptions(true).await;
        page
    }

    async fn subscriptions(&self, on: bool) {
        self.harness
            .ok(
                &self.admin,
                "pages.update",
                json!({ "page": self.id, "slug": "acme", "title": "Acme status",
                        "locales": ["en", "de"], "subscriptions": on }),
            )
            .await;
    }

    /// Subscribes `email`, follows the confirmation link and confirms; returns the link secret.
    async fn subscribe(&self, email: &str, components: Value, locale: &str) -> String {
        let sent = self.sink.mails().len();
        let asked = post(
            &self.harness,
            "/s/acme/subscribe",
            json!({ "email": email, "components": components, "locale": locale }),
        )
        .await;
        assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.json());
        eventually("the confirmation mail", || async {
            self.sink.mails().len() > sent
        })
        .await;
        let mail = self.sink.mails()[sent].clone();
        assert_eq!(mail.to, [format!("<{email}>")]);
        let token = token_in(&mail, "http://v1per.test/s/acme/subscription?token=");
        let confirmed = post(
            &self.harness,
            "/s/acme/subscription/confirm",
            json!({ "token": token }),
        )
        .await;
        assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.json());
        assert_eq!(confirmed.json()["confirmed"], true);
        token
    }

    fn mails_to(&self, email: &str) -> Vec<Mail> {
        let to = format!("<{email}>");
        self.sink
            .mails()
            .into_iter()
            .filter(|mail| mail.to.contains(&to))
            .collect()
    }

    /// "Database upgrade" on the API, two days from now for an hour; returns its start.
    async fn maintenance(&self, description: &str) -> chrono::DateTime<chrono::Utc> {
        let start = chrono::Utc::now() + chrono::Duration::days(2);
        let local = |at: chrono::DateTime<chrono::Utc>| at.format("%Y-%m-%dT%H:%M").to_string();
        self.harness
            .ok(
                &self.admin,
                "maintenance.create",
                json!({ "node": self.node, "title": "Database upgrade", "description": description,
                        "monitors": [self.api],
                        "schedule": { "strategy": "single", "start_date": local(start),
                                      "end_date": local(start + chrono::Duration::hours(1)),
                                      "timezone": "UTC" } }),
            )
            .await;
        start
    }

    async fn incident(&self, title: &str, target: Value) -> String {
        self.harness
            .ok(
                &self.admin,
                "incidents.create",
                json!({ "node": self.node, "title": title, "message": "Looking into it",
                        "impacts": [{ "target": target, "impact": "degraded" }] }),
            )
            .await["incident"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

/// With the setting off every subscription route and feed answers 404 and the snapshot offers
/// nothing; with it on, the snapshot says what visitors may follow.
#[tokio::test]
async fn the_setting_switches_every_route() {
    let page = Page::new(Harness::new().await).await;
    let harness = &page.harness;
    let offer = &get(harness, "/s/acme/snapshot.json").await.json()["subscriptions"];
    assert_eq!(offer["email"], true);
    assert_eq!(offer["feeds"], true);
    assert_eq!(offer["components"], json!(page.components));

    page.subscriptions(false).await;
    assert!(
        get(harness, "/s/acme/snapshot.json")
            .await
            .json()
            .get("subscriptions")
            .is_none()
    );
    for path in [
        "/s/acme/feed.atom",
        "/s/acme/feed.rss",
        "/s/acme/maintenance.ics",
        "/s/acme/subscription?token=x",
    ] {
        assert_eq!(
            get(harness, path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    for path in [
        "/s/acme/subscribe",
        "/s/acme/subscription",
        "/s/acme/subscription/confirm",
        "/s/acme/subscription/components",
        "/s/acme/unsubscribe",
    ] {
        let answer = post(harness, path, json!({ "email": "a@b.test", "token": "x" })).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{path}");
    }
    let html = String::from_utf8(get(harness, "/s/acme").await.bytes).unwrap();
    assert!(!html.contains("feed.atom"));
}

/// Subscribing mails a link; opening it changes nothing until the page app posts the token.
/// A confirmed address shows up for the page's managers, who can remove it.
#[tokio::test]
async fn subscribing_needs_a_confirmation() {
    let page = Page::new(Harness::new().await).await;
    let harness = &page.harness;
    let asked = post(
        harness,
        "/s/acme/subscribe",
        json!({ "email": "Visitor@Example.test", "locale": "de" }),
    )
    .await;
    assert_eq!(asked.status, StatusCode::ACCEPTED);
    assert_eq!(asked.json(), json!({}));
    eventually("the confirmation mail", || async {
        page.sink.mails().len() == 1
    })
    .await;
    let mail = &page.sink.mails()[0];
    assert_eq!(mail.to, ["<visitor@example.test>"]);
    assert!(
        mail.header("Subject")
            .unwrap()
            .starts_with("Bestätigen Sie Ihr Abonnement")
    );
    let token = token_in(mail, "http://v1per.test/s/acme/subscription?token=");

    for _ in 0..2 {
        assert_eq!(
            get(harness, &format!("/s/acme/subscription?token={token}"))
                .await
                .status,
            StatusCode::OK
        );
    }
    let read = post(harness, "/s/acme/subscription", json!({ "token": token })).await;
    assert_eq!(read.json()["confirmed"], false);
    assert_eq!(read.json()["email"], "visitor@example.test");
    let available = read.json()["available"].clone();
    let names: Vec<&str> = available
        .as_array()
        .unwrap()
        .iter()
        .map(|component| component["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["API", "Email"]);
    let listed = harness
        .query(
            Some(&page.admin),
            "pages.subscribers",
            &format!("page={}", page.id),
        )
        .await
        .json();
    assert_eq!(listed["subscribers"], json!([]));

    let confirmed = post(
        harness,
        "/s/acme/subscription/confirm",
        json!({ "token": token }),
    )
    .await;
    assert_eq!(confirmed.json()["confirmed"], true);
    assert_eq!(
        post(
            harness,
            "/s/acme/subscription",
            json!({ "token": "not-a-token" })
        )
        .await
        .code(),
        "invalid_credentials"
    );
    let listed = harness
        .query(
            Some(&page.admin),
            "pages.subscribers",
            &format!("page={}", page.id),
        )
        .await
        .json();
    let subscriber = &listed["subscribers"][0];
    assert_eq!(subscriber["email"], "visitor@example.test");
    assert_eq!(subscriber["components"], json!([]));

    harness
        .ok(
            &page.admin,
            "pages.removeSubscriber",
            json!({ "subscriber": subscriber["id"] }),
        )
        .await;
    assert_eq!(
        post(harness, "/s/acme/subscription", json!({ "token": token }))
            .await
            .code(),
        "invalid_credentials"
    );
}

/// A restricted page takes subscriptions only from allowed addresses, and confirms them only
/// while the address is still allowed.
#[tokio::test]
async fn restricted_pages_take_allowed_addresses_only() {
    let page = Page::new(Harness::new().await).await;
    let harness = &page.harness;
    harness
        .ok(
            &page.admin,
            "pages.updateAccess",
            json!({ "page": page.id, "public": false, "domains": ["client.test"] }),
        )
        .await;
    let secret = harness
        .ok(
            &page.admin,
            "pages.createShareToken",
            json!({ "page": page.id, "name": "Client" }),
        )
        .await["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        post(
            harness,
            "/s/acme/subscribe",
            json!({ "email": "a@client.test" })
        )
        .await
        .code(),
        "unauthenticated"
    );
    for email in ["stranger@evil.test", "guest@client.test"] {
        let asked = post(
            harness,
            &format!("/s/acme/subscribe?token={secret}"),
            json!({ "email": email }),
        )
        .await;
        assert_eq!(asked.status, StatusCode::ACCEPTED);
    }
    eventually("the confirmation mail", || async {
        !page.sink.mails().is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mails = page.sink.mails();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].to, ["<guest@client.test>"]);
    assert_eq!(
        get(harness, "/s/acme/snapshot.json").await.code(),
        "unauthenticated"
    );
    assert_eq!(
        get(harness, &format!("/s/acme/snapshot.json?token={secret}"))
            .await
            .json()["subscriptions"]["feeds"],
        false
    );

    let token = token_in(&mails[0], "http://v1per.test/s/acme/subscription?token=");
    harness
        .ok(
            &page.admin,
            "pages.updateAccess",
            json!({ "page": page.id, "public": false, "domains": ["other.test"] }),
        )
        .await;
    assert_eq!(
        post(
            harness,
            "/s/acme/subscription/confirm",
            json!({ "token": token })
        )
        .await
        .code(),
        "invalid_credentials"
    );
}

/// Incidents notify the subscribers that follow one of their components, in their language,
/// with unsubscribe and manage links. Webhooks get the public incident as JSON, never anything
/// internal. Updates and resolutions notify too; an unpublished incident nobody.
#[tokio::test]
async fn incidents_notify_who_follows_them() {
    let harness = Harness::new().await;
    let background = harness.start();
    let page = Page::new(harness).await;
    let harness = &page.harness;
    let [api, email] = page.components.clone();
    page.subscribe("all@example.test", json!([]), "en").await;
    page.subscribe("mail@example.test", json!([email]), "de")
        .await;
    let discord = Discord::start().await;
    let webhook = harness
        .ok(
            &page.admin,
            "pages.addWebhook",
            json!({ "page": page.id, "url": discord.url, "components": [api] }),
        )
        .await;
    assert_eq!(webhook["host"], "127.0.0.1");
    assert!(!webhook.to_string().contains("s3cr3t"));
    assert_eq!(
        harness
            .run(
                Some(&page.admin),
                "pages.addWebhook",
                json!({ "page": page.id, "url": "ftp://example.test/x" })
            )
            .await
            .code(),
        "invalid_arguments"
    );

    let incident = page
        .incident(
            "API slow",
            json!({ "type": "monitor", "monitor": page.api }),
        )
        .await;
    eventually("the incident mail and post", || async {
        page.mails_to("all@example.test").len() == 2 && discord.posts().len() == 1
    })
    .await;
    let mail = page.mails_to("all@example.test")[1].clone();
    assert_eq!(mail.header("Subject").unwrap(), "Acme status: API slow");
    let body = mail.body();
    assert!(body.contains("Status: Investigating"), "{body}");
    assert!(body.contains("Affected: API"), "{body}");
    assert!(body.contains("Looking into it"), "{body}");
    let unsubscribe = mail.header("List-Unsubscribe").unwrap();
    assert!(
        unsubscribe.starts_with("<http://v1per.test/s/acme/unsubscribe?token="),
        "{unsubscribe}"
    );
    assert_eq!(
        mail.header("List-Unsubscribe-Post").unwrap(),
        "List-Unsubscribe=One-Click"
    );
    assert!(body.contains("&action=unsubscribe"), "{body}");

    let post_body = &discord.posts()[0];
    assert_eq!(post_body["event"], "incident_published");
    assert_eq!(post_body["page"]["slug"], "acme");
    assert_eq!(post_body["page"]["url"], "http://v1per.test/s/acme");
    assert_eq!(post_body["incident"]["title"], "API slow");
    assert_eq!(
        post_body["components"],
        json!([{ "id": api, "name": "API" }])
    );

    // Following only Email, the other subscriber heard nothing about the API.
    assert_eq!(page.mails_to("mail@example.test").len(), 1);
    page.incident(
        "Mail delays",
        json!({ "type": "component", "component": page.email }),
    )
    .await;
    eventually("the German mail", || async {
        page.mails_to("mail@example.test").len() == 2
    })
    .await;
    let german = page.mails_to("mail@example.test")[1].clone();
    assert_eq!(
        german.header("Subject").unwrap(),
        "Acme status: Mail delays"
    );
    assert!(german.body().contains("Wird untersucht"));

    // An update and the resolution notify; an internal-only incident does not.
    harness
        .ok(
            &page.admin,
            "incidents.addUpdate",
            json!({ "incident": incident, "state": "resolved", "message": "Fixed", "impacts": [] }),
        )
        .await;
    page.incident(
        "Worker queue stuck",
        json!({ "type": "monitor", "monitor": page.worker }),
    )
    .await;
    eventually("the resolution", || async { discord.posts().len() == 2 }).await;
    assert_eq!(discord.posts()[1]["event"], "incident_resolved");
    eventually("the resolution mail", || async {
        page.mails_to("all@example.test").len() == 4
    })
    .await;
    let resolved = page.mails_to("all@example.test")[3].clone();
    assert_eq!(
        resolved.header("Subject").unwrap(),
        "Acme status: Resolved: API slow"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let everything: String = page
        .sink
        .mails()
        .iter()
        .map(Mail::body)
        .chain(discord.posts().iter().map(Value::to_string))
        .collect();
    for leak in [
        "worker",
        "Worker queue",
        "secret-db.invalid",
        "\"monitor\"",
        "\"node\"",
    ] {
        assert!(!everything.contains(leak), "a notification contains {leak}");
    }

    // Unpublished from the page, an incident tells nobody about its updates.
    harness
        .ok(
            &page.admin,
            "incidents.unpublish",
            json!({ "incident": incident, "page": page.id }),
        )
        .await;
    let before = page.sink.mails().len();
    harness
        .ok(
            &page.admin,
            "incidents.addUpdate",
            json!({ "incident": incident, "state": "resolved", "message": "Again", "impacts": [] }),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(page.sink.mails().len(), before);
    let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_deliveries")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(queued, 0);
    background.stop().await;
}

/// The page app unsubscribes with a JSON body, a mail client with one click: a form post to
/// the `List-Unsubscribe` URL without an `Origin`. A foreign origin is refused.
#[tokio::test]
async fn subscribers_unsubscribe_from_the_page_or_with_one_click() {
    let page = Page::new(Harness::new().await).await;
    let harness = &page.harness;
    let first = page.subscribe("one@example.test", json!([]), "en").await;
    let second = page.subscribe("two@example.test", json!([]), "en").await;
    let chosen = post(
        harness,
        "/s/acme/subscription/components",
        json!({ "token": first, "components": [page.components[1]] }),
    )
    .await;
    assert_eq!(chosen.json()["components"], json!([page.components[1]]));

    let foreign = Request::post("/s/acme/unsubscribe")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "https://evil.test")
        .body(Body::from(json!({ "token": first }).to_string()))
        .unwrap();
    assert_eq!(harness.send(foreign).await.code(), "origin_rejected");
    let gone = post(harness, "/s/acme/unsubscribe", json!({ "token": first })).await;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);

    let one_click = Request::post(format!("/s/acme/unsubscribe?token={second}"))
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("List-Unsubscribe=One-Click"))
        .unwrap();
    assert_eq!(harness.send(one_click).await.status, StatusCode::NO_CONTENT);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM page_subscribers")
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

/// The feeds list published incidents as the public page shows them, the calendar its
/// maintenance. Both open on public pages, and on restricted ones with a share token.
#[tokio::test]
async fn feeds_and_the_calendar_follow_the_public_page() {
    let page = Page::new(Harness::new().await).await;
    let harness = &page.harness;
    page.incident(
        "API & website slow",
        json!({ "type": "monitor", "monitor": page.api }),
    )
    .await;
    page.incident(
        "Worker queue stuck",
        json!({ "type": "monitor", "monitor": page.worker }),
    )
    .await;
    let start = page.maintenance("Short, planned").await;

    let atom = get(harness, "/s/acme/feed.atom").await;
    assert_eq!(
        atom.header("content-type"),
        "application/atom+xml; charset=utf-8"
    );
    let atom = String::from_utf8(atom.bytes).unwrap();
    assert!(
        atom.contains("<title>API &amp; website slow</title>"),
        "{atom}"
    );
    assert!(
        atom.contains("<id>http://v1per.test/s/acme/incidents#"),
        "{atom}"
    );
    let rss = String::from_utf8(get(harness, "/s/acme/feed.rss").await.bytes).unwrap();
    assert!(
        rss.contains("<item>\n<title>API &amp; website slow</title>"),
        "{rss}"
    );
    for feed in [&atom, &rss] {
        assert!(feed.contains("Looking into it"));
        assert!(!feed.contains("Worker queue stuck"));
        assert!(!feed.contains("worker"));
    }
    let html = String::from_utf8(get(harness, "/s/acme").await.bytes).unwrap();
    assert!(html.contains("<link rel=\"alternate\" type=\"application/atom+xml\" title=\"Acme status\" href=\"/s/acme/feed.atom\">"));

    let ics = get(harness, "/s/acme/maintenance.ics").await;
    assert_eq!(ics.header("content-type"), "text/calendar; charset=utf-8");
    let ics = String::from_utf8(ics.bytes).unwrap();
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
    assert!(ics.contains("SUMMARY:Database upgrade\r\n"), "{ics}");
    assert!(
        ics.contains(&format!("DTSTART:{}\r\n", start.format("%Y%m%dT%H%M00Z"))),
        "{ics}"
    );
    assert!(
        ics.contains("DESCRIPTION:Short\\, planned\\n\\nAPI\r\n"),
        "{ics}"
    );

    harness
        .ok(
            &page.admin,
            "pages.updateAccess",
            json!({ "page": page.id, "public": false }),
        )
        .await;
    let secret = harness
        .ok(
            &page.admin,
            "pages.createShareToken",
            json!({ "page": page.id, "name": "Feed reader" }),
        )
        .await["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    for path in [
        "/s/acme/feed.atom",
        "/s/acme/feed.rss",
        "/s/acme/maintenance.ics",
    ] {
        assert_eq!(
            get(harness, path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
        let opened = get(harness, &format!("{path}?token={secret}")).await;
        assert_eq!(opened.status, StatusCode::OK, "{path}");
        assert_eq!(opened.header("cache-control"), "private, max-age=60");
    }
    let html =
        String::from_utf8(get(harness, &format!("/s/acme?token={secret}")).await.bytes).unwrap();
    assert!(!html.contains("feed.atom"));
}

/// A maintenance window is announced once when it first shows on the page, at the worker's
/// next look, which comes once a minute.
#[tokio::test]
async fn maintenance_is_announced_once() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let page = Page::new(harness).await;
    let harness = &page.harness;
    let discord = Discord::start().await;
    harness
        .ok(
            &page.admin,
            "pages.addWebhook",
            json!({ "page": page.id, "url": discord.url }),
        )
        .await;
    page.maintenance("").await;
    harness.state.clock.advance(Duration::from_secs(61));
    eventually("the announcement", || async { !discord.posts().is_empty() }).await;
    let announced = &discord.posts()[0];
    assert_eq!(announced["event"], "maintenance_scheduled");
    assert_eq!(announced["maintenance"]["title"], "Database upgrade");
    assert_eq!(announced["maintenance"]["active"], false);
    // The next look finds nothing new.
    harness.state.clock.advance(Duration::from_secs(61));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        discord
            .posts()
            .iter()
            .filter(|post| post["event"] == "maintenance_scheduled")
            .count(),
        1
    );
    background.stop().await;
}
