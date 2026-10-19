use tokio::net::TcpStream;
use tokio::time::Instant;

use crate::definition::PortCheck;
use crate::result::{CheckResult, Failure};

/// Kuma's tcping: connect once, report the connect time.
pub(crate) async fn check(port: &PortCheck) -> CheckResult {
    let started = Instant::now();
    match TcpStream::connect((port.hostname.as_str(), port.port)).await {
        Ok(_) => {
            let latency = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            CheckResult::up(format!("{latency} ms")).with_latency(latency)
        }
        Err(_) => CheckResult::down(Failure::Check, "Connection failed"),
    }
}
