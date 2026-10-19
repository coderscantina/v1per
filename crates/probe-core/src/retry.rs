//! Kuma's retry rule (decision 0008). Probes apply it to their own results; the server's
//! evaluator applies it to the monitors it checks itself.

use v1per_protocol::operations::monitors::HeartbeatStatus;

/// What one check round found, before retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Round {
    Pass,
    /// A failure that goes through retries: pending while retries are left, then down.
    Fail,
    /// A failure that is down at once (json-query with `retry_only_on_status_code_failure`).
    FailNow,
}

/// The status after retries and the failures in a row it counts: a failure is pending while
/// `retries < max_retries`, then down; the count keeps rising while down and resets on a pass.
pub fn retry(round: Round, retries: u32, max_retries: u32) -> (HeartbeatStatus, u32) {
    match round {
        Round::Pass => (HeartbeatStatus::Up, 0),
        Round::FailNow => (HeartbeatStatus::Down, 0),
        Round::Fail if max_retries > 0 && retries < max_retries => {
            (HeartbeatStatus::Pending, retries + 1)
        }
        Round::Fail => (HeartbeatStatus::Down, retries.saturating_add(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A json-query mismatch that skips retries is down at once; the server's evaluator tests
    /// cover the rest of the rule.
    #[test]
    fn fail_now_skips_retries() {
        assert_eq!(retry(Round::FailNow, 0, 5), (HeartbeatStatus::Down, 0));
    }
}
