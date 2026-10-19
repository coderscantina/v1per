//! Maintenance end to end: covered monitors and members of covered groups skip their checks,
//! record maintenance beats and send no alerts; windows follow their timezone across a DST
//! change on the evaluator's clock.

mod common;

use std::future::Future;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use common::fakes::Discord;
use common::{Harness, Target};
use serde_json::json;
use v1per_server::clock::Clock;

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

async fn status(harness: &Harness, monitor: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT status FROM monitor_state WHERE monitor_id = ?")
        .bind(monitor.parse::<i64>().unwrap())
        .fetch_optional(&harness.state.db.reader)
        .await
        .unwrap()
}

fn advance(harness: &Harness, seconds: u64) {
    harness.state.clock.advance(Duration::from_secs(seconds));
}

/// A manual maintenance on a group covers its member: no checks reach the target, both record
/// maintenance, nothing is alerted. Once it ends, the still failing member alerts, as Kuma's
/// maintenance-to-down rule says.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn maintenance_skips_checks_and_silences_alerts() {
    let harness = Harness::manual().await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let (hook, target) = (Discord::start().await, Target::start().await);
    let chat = harness
        .ok(
            &admin,
            "channels.create",
            json!({ "node": root, "name": "chat", "is_default": true,
                    "settings": { "type": "discord", "webhook_url": { "value": hook.url } } }),
        )
        .await["channel"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let group = harness
        .ok(
            &admin,
            "monitors.create",
            json!({ "node": root, "name": "shop", "interval": 20, "settings": { "type": "group" } }),
        )
        .await["monitor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let site = harness
        .ok(
            &admin,
            "monitors.create",
            json!({ "node": root, "name": "site", "interval": 20, "group": group,
                    "settings": { "type": "http", "url": target.url } }),
        )
        .await["monitor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    eventually("the site to be up", || async {
        status(&harness, &site).await == Some(1)
    })
    .await;

    let window = harness
        .ok(
            &admin,
            "maintenance.create",
            json!({ "node": root, "title": "Migration", "monitors": [group],
                    "schedule": { "strategy": "manual" } }),
        )
        .await;
    assert_eq!(window["maintenance"]["status"], "under_maintenance");
    let maintenance = window["maintenance"]["id"].as_str().unwrap().to_owned();
    eventually("both in maintenance", || async {
        status(&harness, &site).await == Some(3) && status(&harness, &group).await == Some(3)
    })
    .await;
    let message: String =
        sqlx::query_scalar("SELECT message FROM monitor_state WHERE monitor_id = ?")
            .bind(site.parse::<i64>().unwrap())
            .fetch_one(&harness.state.db.reader)
            .await
            .unwrap();
    assert_eq!(message, "Monitor under maintenance");

    target.answer(500);
    let hits = target.hits();
    for _ in 0..3 {
        advance(&harness, 20);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(target.hits(), hits, "no check runs during maintenance");
    assert_eq!(status(&harness, &site).await, Some(3));
    assert!(hook.posts().is_empty());

    harness
        .ok(
            &admin,
            "maintenance.pause",
            json!({ "maintenance": maintenance }),
        )
        .await;
    eventually("the site to be checked and down", || async {
        advance(&harness, 1);
        status(&harness, &site).await == Some(0)
    })
    .await;
    eventually("one down alert", || async { !hook.posts().is_empty() }).await;
    let log = harness
        .query(
            Some(&admin),
            "channels.deliveries",
            &format!("channel={chat}"),
        )
        .await
        .json();
    let events: Vec<&str> = log["deliveries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["summary"].as_str().unwrap())
        .collect();
    assert!(events.contains(&"[site] [🔴 Down] Request failed with status code 500"));
    assert!(events.iter().all(|summary| summary.contains("Down")));
    background.stop().await;
}

async fn in_maintenance(harness: &Harness, monitor: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT 1 FROM maintenance_state WHERE monitor_id = ?")
        .bind(monitor.parse::<i64>().unwrap())
        .fetch_optional(&harness.state.db.reader)
        .await
        .unwrap()
        .is_some()
}

/// A daily 09:00 Vienna window moves from 07:00 to 08:00 UTC when summer time ends on
/// 2026-10-25, in the preview and on the evaluator's clock.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daily_windows_follow_their_timezone_across_dst() {
    let at = |day: u32, hour: u32, minute: u32| {
        Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap()
            .timestamp_millis()
    };
    let harness = Harness::with_clock(Clock::manual(at(24, 6, 0))).await;
    let background = harness.start();
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;

    let preview = harness
        .query(
            Some(&admin),
            "maintenance.preview",
            "strategy=cron&cron=0%209%20*%20*%20*&duration=3600&timezone=Europe%2FVienna",
        )
        .await
        .json();
    assert_eq!(
        preview["windows"].as_array().unwrap()[..2],
        [
            json!({ "start": at(24, 7, 0), "end": at(24, 8, 0) }),
            json!({ "start": at(25, 8, 0), "end": at(25, 9, 0) }),
        ]
    );
    let invalid = harness
        .query(
            Some(&admin),
            "maintenance.preview",
            "strategy=recurring-weekday&start_time=25:00",
        )
        .await
        .json();
    assert_eq!(
        invalid["errors"],
        json!({
            "start_time": [{ "code": "invalid_format" }],
            "end_time": [{ "code": "required" }],
            "weekdays": [{ "code": "required" }],
        })
    );

    let monitor = harness
        .ok(
            &admin,
            "monitors.create",
            json!({ "node": root, "name": "batch", "interval": 60, "settings": { "type": "manual" } }),
        )
        .await["monitor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    harness
        .ok(
            &admin,
            "maintenance.create",
            json!({ "node": root, "title": "Nightly", "monitors": [monitor],
                    "schedule": { "strategy": "cron", "cron": "0 9 * * *", "duration": 3600,
                                  "timezone": "Europe/Vienna" } }),
        )
        .await;
    assert!(!in_maintenance(&harness, &monitor).await);

    let step = |to: i64| {
        let now = harness.state.clock.now_ms();
        advance(&harness, ((to - now) / 1000) as u64);
    };
    for (to, covered) in [
        (at(24, 7, 0), true),
        (at(24, 8, 0), false),
        (at(25, 7, 0), false),
        (at(25, 8, 0), true),
        (at(25, 9, 0), false),
    ] {
        step(to);
        eventually(&format!("maintenance {covered} at {to}"), || async {
            in_maintenance(&harness, &monitor).await == covered
        })
        .await;
        if covered {
            eventually("a maintenance beat", || async {
                status(&harness, &monitor).await == Some(3)
            })
            .await;
        }
    }
    let changes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM topic_events WHERE topic = ? AND kind = 'maintenance.status_changed'",
    )
    .bind(format!("maintenance:{root}"))
    .fetch_one(&harness.state.db.reader)
    .await
    .unwrap();
    assert_eq!(changes, 4);
    background.stop().await;
}
