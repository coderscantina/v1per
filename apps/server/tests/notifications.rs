//! Notifications end to end: monitor transitions through the outbox to a local SMTP sink and
//! a Discord webhook stand-in, with Kuma 2.5.0's messages, retries across an outage and a
//! restart, resend intervals, test sends, and channel access and sealing.

mod common;

use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Utc};
use common::fakes::{Discord, SmtpSink};
use common::{Harness, Target};
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

fn advance(harness: &Harness, seconds: u64) {
    harness.state.clock.advance(Duration::from_secs(seconds));
}

/// The time of the monitor's latest beat, once it is `at` or later.
async fn beat_at(harness: &Harness, monitor: &str, at: i64) {
    eventually(&format!("a beat at {at}"), || async {
        let beat: Option<i64> =
            sqlx::query_scalar("SELECT beat_at FROM monitor_state WHERE monitor_id = ?")
                .bind(monitor.parse::<i64>().unwrap())
                .fetch_optional(&harness.state.db.reader)
                .await
                .unwrap();
        beat.is_some_and(|beat| beat >= at)
    })
    .await;
}

fn discord(name: &str, url: &str) -> Value {
    json!({ "name": name, "settings": { "type": "discord", "webhook_url": { "value": url } } })
}

async fn channel(harness: &Harness, cookie: &str, node: &str, channel: Value) -> String {
    let mut args = channel;
    args["node"] = json!(node);
    let created = harness.ok(cookie, "channels.create", args).await;
    created["channel"]["id"].as_str().unwrap().to_owned()
}

/// A monitor of the local target that is down at the first failure.
async fn monitor(harness: &Harness, cookie: &str, node: &str, url: &str, extra: Value) -> String {
    let mut args = json!({
        "node": node, "name": "site", "interval": 20,
        "settings": { "type": "http", "url": url },
    });
    for (key, value) in extra.as_object().unwrap() {
        args[key] = value.clone();
    }
    let created = harness.ok(cookie, "monitors.create", args).await;
    created["monitor"]["id"].as_str().unwrap().to_owned()
}

async fn deliveries(harness: &Harness, cookie: &str, channel: &str) -> Vec<Value> {
    let page = harness
        .query(
            Some(cookie),
            "channels.deliveries",
            &format!("channel={channel}"),
        )
        .await
        .json();
    page["deliveries"].as_array().unwrap().clone()
}

/// Unix milliseconds of the latest important down or up transition.
async fn transition_time(harness: &Harness, monitor: &str, status: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT time FROM monitor_transitions WHERE monitor_id = ? AND status = ?
         ORDER BY id DESC LIMIT 1",
    )
    .bind(monitor.parse::<i64>().unwrap())
    .bind(status)
    .fetch_one(&harness.state.db.reader)
    .await
    .unwrap()
}

fn vienna(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap()
        .with_timezone(&chrono_tz::Europe::Vienna)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// The milestone's main path: a monitor's default channels receive Kuma's down and up messages
/// by email and Discord, each exactly once, in the instance timezone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn down_and_up_reach_email_and_discord_like_kuma() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    harness
        .ok(
            &admin,
            "instance.updateSettings",
            json!({ "timezone": "Europe/Vienna" }),
        )
        .await;
    let (sink, hook, target) = (
        SmtpSink::start().await,
        Discord::start().await,
        Target::start().await,
    );
    let mail = channel(
        &harness,
        &admin,
        &root,
        json!({
            "name": "Ops mail", "is_default": true,
            "settings": {
                "type": "smtp", "host": "127.0.0.1", "port": sink.port,
                "from": "v1per <alerts@example.test>", "to": "ops@example.test",
            },
        }),
    )
    .await;
    let mut chat = discord("Ops chat", &hook.url);
    chat["is_default"] = json!(true);
    let chat = channel(&harness, &admin, &root, chat).await;

    target.answer(500);
    let id = monitor(&harness, &admin, &root, &target.url, json!({})).await;
    let detail = harness
        .query(Some(&admin), "monitors.get", &format!("monitor={id}"))
        .await
        .json();
    assert_eq!(detail["monitor"]["channels"], json!([mail, chat]));

    eventually("the down messages", || async {
        sink.mails().len() == 1 && hook.posts().len() == 1
    })
    .await;
    let down_at = transition_time(&harness, &id, 0).await;
    let subject = "[site] [🔴 Down] Request failed with status code 500";
    let sent = &sink.mails()[0];
    assert_eq!(sent.header("Subject").unwrap(), subject);
    assert_eq!(sent.to, ["<ops@example.test>"]);
    assert_eq!(
        sent.body(),
        format!("{subject}\nTime (Europe/Vienna): {}", vienna(down_at))
    );
    let time = DateTime::<Utc>::from_timestamp_millis(down_at).unwrap();
    assert_eq!(
        hook.posts()[0],
        json!({
            "username": "Uptime Kuma",
            "embeds": [{
                "title": "❌ Your service site went down. ❌",
                "color": 16711680,
                "timestamp": time.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "fields": [
                    { "name": "Service Name", "value": "site" },
                    { "name": "Service URL", "value": target.url },
                    { "name": "Went Offline", "value": format!("<t:{}:F>", down_at / 1000) },
                    { "name": "Time (Europe/Vienna)", "value": vienna(down_at) },
                    { "name": "Error", "value": "Request failed with status code 500" },
                ],
            }],
        })
    );

    target.answer(200);
    advance(&harness, 20);
    eventually("the up messages", || async {
        sink.mails().len() == 2 && hook.posts().len() == 2
    })
    .await;
    assert_eq!(
        sink.mails()[1].header("Subject").unwrap(),
        "[site] [✅ Up] 200 - OK"
    );
    let up = &hook.posts()[1]["embeds"][0];
    assert_eq!(up["title"], "✅ Your service site is up! ✅");
    assert_eq!(up["color"], 65280);

    let log = deliveries(&harness, &admin, &chat).await;
    let states: Vec<(&str, &str, u64)> = log
        .iter()
        .map(|d| {
            (
                d["event"].as_str().unwrap(),
                d["state"].as_str().unwrap(),
                d["attempts"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("monitor_up", "delivered", 1),
            ("monitor_down", "delivered", 1)
        ]
    );
    assert_eq!(log[1]["summary"], subject);
    // Nothing more goes out while the monitor stays up.
    advance(&harness, 20);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!((sink.mails().len(), hook.posts().len()), (2, 2));
    background.stop().await;
}

/// A provider outage delays a message without losing it, a restart in the middle of the
/// retries resumes them, and the message arrives exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_outage_delays_delivery_across_a_restart() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let (hook, target) = (Discord::start().await, Target::start().await);
    let chat = channel(&harness, &admin, &root, discord("Ops chat", &hook.url)).await;
    hook.answer(503);
    target.answer(500);
    monitor(
        &harness,
        &admin,
        &root,
        &target.url,
        json!({ "channels": [chat] }),
    )
    .await;

    eventually("a failed first attempt", || async {
        let log = deliveries(&harness, &admin, &chat).await;
        log.len() == 1 && log[0]["attempts"] == 1
    })
    .await;
    let log = deliveries(&harness, &admin, &chat).await;
    assert_eq!(log[0]["state"], "pending");
    assert_eq!(log[0]["error"], "provider answered HTTP 503: ");
    let retry_at = log[0]["next_attempt_at"].as_i64().unwrap();
    assert_eq!(retry_at - harness.state.clock.now_ms(), 10_000);

    let harness = harness.restart(background).await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    advance(&harness, 10);
    eventually("a second failed attempt", || async {
        deliveries(&harness, &admin, &chat).await[0]["attempts"] == 2
    })
    .await;
    hook.answer(204);
    advance(&harness, 20);
    eventually("the delivery", || async {
        deliveries(&harness, &admin, &chat).await[0]["state"] == "delivered"
    })
    .await;
    let log = deliveries(&harness, &admin, &chat).await;
    assert_eq!(log[0]["attempts"], 3);
    assert!(log[0].get("error").is_none());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(hook.posts().len(), 1);
    background.stop().await;
}

/// Kuma's resend interval: the down message repeats after every second non-important down
/// beat, and only then.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn down_messages_repeat_every_resend_interval() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let (hook, target) = (Discord::start().await, Target::start().await);
    let chat = channel(&harness, &admin, &root, discord("Ops chat", &hook.url)).await;
    target.answer(500);
    let start = harness.state.clock.now_ms();
    let id = monitor(
        &harness,
        &admin,
        &root,
        &target.url,
        json!({ "channels": [chat], "resend_interval": 2 }),
    )
    .await;
    beat_at(&harness, &id, start).await;
    for round in 1..=5 {
        advance(&harness, 20);
        beat_at(&harness, &id, start + round * 20_000).await;
    }
    // Down at 0, repeated at 40 and 80 seconds.
    eventually("three down messages", || async { hook.posts().len() == 3 }).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let posts = hook.posts();
    assert_eq!(posts.len(), 3);
    assert!(
        posts
            .iter()
            .all(|post| post["embeds"][0]["title"] == "❌ Your service site went down. ❌")
    );
    background.stop().await;
}

/// Channels are Admin business, sealed at rest, never answered with their secrets, usable in
/// their subtree only, and testable before saving, through the instance SMTP server too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn channels_are_scoped_sealed_and_testable() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let team = harness.node(&admin, &root, "team").await;
    let other = harness.node(&admin, &root, "other").await;
    harness.grant(&harness.member, &team, "member").await;
    let member = harness.session(&harness.member).await;
    let hook = Discord::start().await;
    let token = "s3cr3t-webhook-token";

    let refused = harness
        .run(
            Some(&member),
            "channels.create",
            json!({ "node": team, "name": "x", "settings": { "type": "discord", "webhook_url": { "value": hook.url } } }),
        )
        .await;
    assert_eq!(refused.code(), "forbidden");
    let invalid = harness
        .run(
            Some(&admin),
            "channels.create",
            json!({ "node": team, "name": "x", "settings": { "type": "discord", "message_template": "{% if %}" } }),
        )
        .await;
    assert_eq!(
        invalid.json()["errors"],
        json!({
            "settings.message_template": [{ "code": "invalid_format" }],
            "settings.webhook_url": [{ "code": "required" }],
        })
    );

    let chat = channel(&harness, &admin, &root, discord("Ops chat", &hook.url)).await;
    let elsewhere = channel(&harness, &admin, &other, discord("Other", &hook.url)).await;
    let listed = harness
        .query(Some(&admin), "channels.list", &format!("node={root}"))
        .await;
    assert!(!String::from_utf8_lossy(&listed.bytes).contains(token));
    assert_eq!(
        listed.json()["channels"][0]["settings"]["webhook_url"],
        json!({ "set": true })
    );
    let stored: Vec<u8> = sqlx::query_scalar("SELECT config FROM channels WHERE id = ?")
        .bind(chat.parse::<i64>().unwrap())
        .fetch_one(&harness.state.db.reader)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains(token));

    let available = harness
        .query(Some(&member), "channels.available", &format!("node={team}"))
        .await
        .json();
    assert_eq!(
        available["channels"],
        json!([{ "id": chat, "name": "Ops chat", "provider": "discord", "is_default": false, "active": true, "own": false }])
    );
    let foreign = harness
        .run(
            Some(&member),
            "monitors.create",
            json!({ "node": team, "name": "m", "interval": 60, "settings": { "type": "manual" }, "channels": [elsewhere] }),
        )
        .await;
    assert_eq!(
        foreign.json()["errors"]["channels"][0]["code"],
        "invalid_value"
    );

    // A test of the unsaved form keeps the stored webhook and reports its outcome.
    let tested = harness
        .ok(
            &admin,
            "channels.test",
            json!({ "node": root, "channel": chat, "name": "Renamed", "settings": { "type": "discord" , "webhook_url": {} } }),
        )
        .await;
    let delivery = tested["delivery"].as_str().unwrap().to_owned();
    eventually("the test message", || async {
        let answer = harness
            .query(
                Some(&admin),
                "channels.delivery",
                &format!("delivery={delivery}"),
            )
            .await
            .json();
        answer["delivery"]["state"] == "delivered"
    })
    .await;
    assert_eq!(
        hook.posts(),
        [json!({ "username": "Uptime Kuma", "content": "Renamed Testing" })]
    );

    // An email channel without a server of its own sends through the instance's.
    let sink = SmtpSink::start().await;
    harness
        .ok(
            &admin,
            "instance.updateSmtp",
            json!({ "smtp": { "host": "127.0.0.1", "port": sink.port, "from": "v1per <noreply@example.test>", "username": "v1per", "password": { "value": "smtp-pass" } } }),
        )
        .await;
    let settings = harness.query(Some(&admin), "instance.settings", "").await;
    assert!(!String::from_utf8_lossy(&settings.bytes).contains("smtp-pass"));
    let tested = harness
        .ok(
            &admin,
            "channels.test",
            json!({ "node": root, "name": "Mail", "settings": { "type": "smtp", "to": "ops@example.test" } }),
        )
        .await;
    let mail_test = tested["delivery"].as_str().unwrap().to_owned();
    eventually("the test mail", || async {
        let answer = harness
            .query(
                Some(&admin),
                "channels.delivery",
                &format!("delivery={mail_test}"),
            )
            .await
            .json();
        assert_ne!(answer["delivery"]["state"], "failed", "{answer}");
        sink.mails().len() == 1
    })
    .await;
    assert_eq!(sink.mails()[0].from, "<noreply@example.test>");
    assert_eq!(sink.mails()[0].auth.as_deref(), Some("v1per:smtp-pass"));
    assert_eq!(sink.mails()[0].header("Subject").unwrap(), "Mail Testing");
    assert!(tested["delivery"].is_string());

    // Nothing the outbox keeps holds a secret.
    let texts: Vec<String> = sqlx::query_scalar(
        "SELECT payload || summary || coalesce(error, '') || coalesce(detail, '')
         FROM notification_outbox",
    )
    .fetch_all(&harness.state.db.reader)
    .await
    .unwrap();
    assert!(
        texts
            .iter()
            .all(|text| !text.contains(token) && !text.contains("smtp-pass"))
    );
    background.stop().await;
}

/// A test can send a down alert filled from a monitor on the node or below. It stays a test
/// row, and a monitor elsewhere is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_can_send_a_monitor_alert() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let team = harness.node(&admin, &root, "team").await;
    let other = harness.node(&admin, &root, "other").await;
    let hook = Discord::start().await;
    let inside = monitor(&harness, &admin, &team, "http://127.0.0.1:9/", json!({})).await;
    let outside = monitor(&harness, &admin, &other, "http://127.0.0.1:9/", json!({})).await;
    let form = |monitor: &str| {
        json!({
            "node": team, "name": "Ops chat", "sample": "monitor_down", "monitor": monitor,
            "settings": { "type": "discord", "webhook_url": { "value": hook.url } },
        })
    };

    let refused = harness
        .run(Some(&admin), "channels.test", form(&outside))
        .await;
    assert_eq!(
        refused.json()["errors"]["monitor"][0]["code"],
        "invalid_value"
    );

    let queued = harness.ok(&admin, "channels.test", form(&inside)).await;
    let delivery = queued["delivery"].as_str().unwrap().to_owned();
    eventually("the sample alert", || async { hook.posts().len() == 1 }).await;
    assert_eq!(
        hook.posts()[0]["embeds"][0]["title"],
        "❌ Your service site went down. ❌"
    );
    let (event, monitor_id, summary): (String, Option<i64>, String) =
        sqlx::query_as("SELECT event, monitor_id, summary FROM notification_outbox WHERE id = ?")
            .bind(delivery.parse::<i64>().unwrap())
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap();
    assert_eq!(event, "test");
    assert_eq!(monitor_id, None);
    assert!(summary.starts_with("[site] [🔴 Down]"), "{summary}");
    background.stop().await;
}

/// `channels.get` and a monitor's deliveries across the channels its reader may see. Anyone
/// who reads the monitor sees its alerts; the provider's answers need `channels.manage`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_monitor_lists_its_deliveries_per_readable_channel() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let team = harness.node(&admin, &root, "team").await;
    harness.grant(&harness.member, &team, "admin").await;
    let member = harness.session(&harness.member).await;
    let hook = Discord::start().await;
    let ops = channel(&harness, &admin, &root, discord("Ops", &hook.url)).await;
    let own = channel(&harness, &admin, &team, discord("Team", &hook.url)).await;

    let got = harness
        .query(Some(&member), "channels.get", &format!("channel={own}"))
        .await
        .json();
    assert_eq!(got["channel"]["name"], "Team");
    let refused = harness
        .query(Some(&member), "channels.get", &format!("channel={ops}"))
        .await;
    assert_eq!(refused.code(), "not_found");

    let created = harness
        .ok(
            &admin,
            "monitors.create",
            json!({ "node": team, "name": "api", "interval": 60,
                    "settings": { "type": "manual" }, "channels": [ops, own] }),
        )
        .await;
    let monitor = created["monitor"]["id"].as_str().unwrap().to_owned();
    harness
        .ok(
            &admin,
            "monitors.setManualStatus",
            json!({ "monitor": monitor, "status": "down" }),
        )
        .await;
    let listed = |cookie: String| {
        let (harness, monitor) = (&harness, &monitor);
        async move {
            let page = harness
                .query(
                    Some(&cookie),
                    "channels.monitorDeliveries",
                    &format!("monitor={monitor}"),
                )
                .await
                .json();
            let mut channels: Vec<String> = page["deliveries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|delivery| delivery["channel"].as_str().unwrap().to_owned())
                .collect();
            channels.sort();
            channels
        }
    };
    eventually("both down deliveries", || async {
        listed(admin.clone()).await.len() == 2
    })
    .await;
    let mut both = vec![ops.clone(), own.clone()];
    both.sort();
    assert_eq!(listed(admin.clone()).await, both);
    assert_eq!(listed(member.clone()).await, std::slice::from_ref(&own));

    // A Viewer reads the monitor's recent alerts without the provider's answers.
    let viewer_id = harness.person("viewer").await;
    harness.grant(&viewer_id, &team, "viewer").await;
    let viewer = harness.session(&viewer_id).await;
    assert_eq!(listed(viewer.clone()).await, std::slice::from_ref(&own));
    let page = |cookie: String| {
        let (harness, monitor) = (&harness, &monitor);
        async move {
            harness
                .query(
                    Some(&cookie),
                    "channels.monitorDeliveries",
                    &format!("monitor={monitor}"),
                )
                .await
                .json()["deliveries"][0]
                .clone()
        }
    };
    let (managed, read) = (page(member.clone()).await, page(viewer.clone()).await);
    assert_eq!(read["id"], managed["id"]);
    assert_eq!(read["state"], "delivered");
    assert!(managed.get("detail").is_some(), "{managed}");
    assert!(
        read.get("detail").is_none() && read.get("error").is_none(),
        "{read}"
    );
    let refused = harness
        .query(Some(&viewer), "channels.get", &format!("channel={own}"))
        .await;
    assert_eq!(refused.code(), "forbidden");

    // Without `node.read` on the monitor's node nothing is listed.
    let outsider_id = harness.person("outsider").await;
    let outsider = harness.session(&outsider_id).await;
    let hidden = harness
        .query(
            Some(&outsider),
            "channels.monitorDeliveries",
            &format!("monitor={monitor}"),
        )
        .await;
    assert_eq!(hidden.code(), "not_found");
    background.stop().await;
}

/// `instance.testSmtp` mails the given or the saved server's settings, never more than five
/// times a minute.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_instance_smtp_server_can_be_tested() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let sink = SmtpSink::start().await;
    let form = json!({ "host": "127.0.0.1", "port": sink.port, "from": "v1per <noreply@example.test>",
                       "username": "v1per", "password": { "value": "typed-pass" } });

    let unsaved = harness
        .run(
            Some(&admin),
            "instance.testSmtp",
            json!({ "to": "ops@example.test" }),
        )
        .await;
    assert_eq!(unsaved.json()["errors"]["smtp"][0]["code"], "required");
    let invalid = harness
        .run(
            Some(&admin),
            "instance.testSmtp",
            json!({ "to": "not mail", "smtp": form }),
        )
        .await;
    assert_eq!(invalid.json()["errors"]["to"][0]["code"], "invalid_format");

    let tested = harness
        .ok(
            &admin,
            "instance.testSmtp",
            json!({ "to": "ops@example.test", "smtp": form }),
        )
        .await;
    assert!(tested["delivery"].is_string());
    eventually("the typed server's test mail", || async {
        sink.mails().len() == 1
    })
    .await;
    assert_eq!(sink.mails()[0].to, ["<ops@example.test>"]);
    assert_eq!(sink.mails()[0].auth.as_deref(), Some("v1per:typed-pass"));

    let mut saved = form.clone();
    saved["password"] = json!({ "value": "saved-pass" });
    harness
        .ok(&admin, "instance.updateSmtp", json!({ "smtp": saved }))
        .await;
    let mut kept = form.clone();
    kept["password"] = json!({});
    for smtp in [None, Some(kept)] {
        let mut args = json!({ "to": "ops@example.test" });
        if let Some(smtp) = smtp {
            args["smtp"] = smtp;
        }
        harness.ok(&admin, "instance.testSmtp", args).await;
    }
    eventually("the saved server's test mails", || async {
        sink.mails().len() == 3
    })
    .await;
    for mail in &sink.mails()[1..] {
        assert_eq!(mail.auth.as_deref(), Some("v1per:saved-pass"));
    }

    // Three were queued and two refused for their fields; the limit counts every call.
    let limited = harness
        .run(
            Some(&admin),
            "instance.testSmtp",
            json!({ "to": "ops@example.test" }),
        )
        .await;
    assert_eq!(limited.code(), "rate_limited");
    let member = harness.session(&harness.member).await;
    let refused = harness
        .run(
            Some(&member),
            "instance.testSmtp",
            json!({ "to": "ops@example.test" }),
        )
        .await;
    assert_eq!(refused.code(), "forbidden");
    background.stop().await;
}
