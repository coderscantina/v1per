//! Scheduler measurement (milestone 05): the reference workload of 2,000 monitors, 400 at a
//! 20 second and 1,600 at a 60 second interval, against a local target on the system clock.
//! The release gate allows under 0.1 % of checks to start more than one interval late.
//!
//! Takes minutes, so it runs on request:
//! `cargo test --release -p v1per-server --test scheduler_load -- --ignored --nocapture`.
//! `V1PER_LOAD_SECONDS` sets how long it runs (default 180). Raise the open file limit first
//! (`ulimit -n 4096`): checks in flight and the target's connections each hold a socket.

mod common;

use std::time::{Duration, Instant};

use common::{Harness, Target};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "takes minutes; run for scheduler evidence"]
async fn two_thousand_monitors_start_on_time() {
    let seconds: u64 = std::env::var("V1PER_LOAD_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(180);
    let harness = Harness::new().await;
    let admin = harness.session(&harness.admin).await;
    let root = harness.root().await;
    let target = Target::start().await;
    for index in 0..2_000 {
        let interval = if index < 400 { 20 } else { 60 };
        harness
            .ok(
                &admin,
                "monitors.create",
                json!({
                    "node": root,
                    "name": format!("load {index}"),
                    "interval": interval,
                    "settings": { "type": "http", "url": format!("{}/{index}", target.url) },
                }),
            )
            .await;
    }

    // Started after the monitors exist, as after a restart: every monitor joins its phase.
    let background = harness.start();
    let began = Instant::now();
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    let (started, late) = background.check_lag();
    let health = harness.state.storage.health();
    background.stop().await;

    let expected = seconds * (400 / 20 + 1_600 / 60);
    let fraction = late as f64 / started.max(1) as f64;
    println!(
        "ran {:?}: {started} checks started (about {expected} expected), {late} more than one \
         interval late ({:.4} %); target hits {}; inbox rows {}, transfer failures {}",
        began.elapsed(),
        fraction * 100.0,
        target.hits(),
        health.inbox_rows,
        health.transfer_failures,
    );
    assert!(started * 10 >= expected * 9, "too few checks ran");
    assert!(fraction < 0.001, "{late} of {started} checks started late");
}
