//! ICMP echo over unprivileged datagram sockets (Linux `ping_group_range`, macOS), falling back
//! to raw sockets when the process has `CAP_NET_RAW`.

use std::io::ErrorKind;
use std::net::IpAddr;
use std::time::Duration;

use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};
use tokio::time::Instant;

use crate::definition::PingCheck;
use crate::result::{CheckResult, Failure};

const PACKET_INTERVAL: Duration = Duration::from_secs(1);

const NOT_PERMITTED: &str = "ICMP sockets are not permitted for this process. On Linux, allow \
    unprivileged ping with `sysctl net.ipv4.ping_group_range=\"0 2147483647\"` (Docker sets this \
    by default) or grant CAP_NET_RAW";

/// Sends `ping_count` echoes one second apart and stops at `deadline`, like `ping -c -w -W`.
/// Up when any reply arrives; latency is the first reply's round trip, as node-ping reports.
pub(crate) async fn check(ping: &PingCheck, deadline: Instant) -> CheckResult {
    let host = ping.hostname.trim_start_matches('[').trim_end_matches(']');
    let Some(ip) = resolve(host).await else {
        return CheckResult::down(
            Failure::Check,
            format!("ping: cannot resolve {host}: Unknown host"),
        );
    };
    let kind = if ip.is_ipv4() { ICMP::V4 } else { ICMP::V6 };
    let client = match Client::new(&Config::builder().kind(kind).build()) {
        Ok(client) => client,
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            return CheckResult::down(Failure::NotPermitted, NOT_PERMITTED);
        }
        Err(error) => return CheckResult::down(Failure::Check, format!("ping: {error}")),
    };
    let mut pinger = client.pinger(ip, PingIdentifier(rand::random())).await;
    let payload = vec![0; usize::from(ping.packet_size)];
    let per_request = Duration::try_from_secs_f64(ping.ping_per_request_timeout)
        .unwrap_or(Duration::from_secs(2));

    let mut first_reply = None;
    let mut sent = 0;
    for sequence in 0..ping.ping_count {
        let now = Instant::now();
        let remaining = deadline.saturating_duration_since(now);
        if remaining.is_zero() {
            break;
        }
        pinger.timeout(per_request.min(remaining));
        sent += 1;
        match pinger.ping(PingSequence(sequence), &payload).await {
            Ok((_, round_trip)) => {
                first_reply.get_or_insert(round_trip);
            }
            Err(SurgeError::Timeout { .. }) => {}
            Err(error) => return CheckResult::down(Failure::Check, format!("ping: {error}")),
        }
        if sequence + 1 < ping.ping_count {
            tokio::time::sleep_until((now + PACKET_INTERVAL).min(deadline)).await;
        }
    }

    match first_reply {
        Some(round_trip) => {
            let latency = u64::try_from(round_trip.as_millis()).unwrap_or(u64::MAX);
            CheckResult::up("").with_latency(latency)
        }
        None => CheckResult::down(
            Failure::Check,
            format!("{sent} packets transmitted, 0 received, 100% packet loss"),
        ),
    }
}

/// IPv4 first, like Kuma, which retries with IPv6 only when the IPv4 ping cannot resolve.
async fn resolve(host: &str) -> Option<IpAddr> {
    if let Ok(ip) = host.parse() {
        return Some(ip);
    }
    let addresses: Vec<IpAddr> = tokio::net::lookup_host((host, 0))
        .await
        .ok()?
        .map(|address| address.ip())
        .collect();
    addresses
        .iter()
        .find(|ip| ip.is_ipv4())
        .or(addresses.first())
        .copied()
}
