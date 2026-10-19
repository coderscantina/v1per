//! ICMP needs unprivileged datagram sockets: available on macOS, and on Linux when
//! `net.ipv4.ping_group_range` covers the test user's group (Docker's default) or with CAP_NET_RAW.

mod support;

use serde_json::json;
use support::definition;
use v1per_checks::{CheckContext, Status, run};

#[tokio::test]
async fn ping_to_loopback_passes() {
    let check = definition(
        json!({ "type": "ping", "timeout": 5, "hostname": "127.0.0.1", "ping_count": 2 }),
    );
    let result = run(&check, &CheckContext::new()).await;

    assert_eq!(result.status, Status::Up, "{}", result.message);
    assert!(result.latency_ms.is_some());
}
