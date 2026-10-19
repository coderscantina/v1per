//! The scheduler every probe shares, the built-in one and remote agents (decision 0003).
//!
//! A monitor's rounds fall on a fixed grid: the next one is due an interval (a retry interval
//! while pending) after the previous one was due, not after it finished, so a slow check does
//! not shift later ones. A monitor unchanged since the probe started joins the grid at a phase
//! derived from its ID, so restarts spread the load the same way every time and every probe
//! checks a monitor in the same slot; one created or changed since checks at once. A semaphore
//! bounds the checks in flight. Each probe applies Kuma's retry rule to its own results
//! (decision 0008), so it knows its next slot without asking the server.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::Semaphore;
use v1per_checks::{CheckContext, Failure};
use v1per_protocol::ids::MonitorId;

use crate::clock::Clock;
use crate::retry::{Round, retry};
use crate::wire::{Assignment, ChainCertificate, ProbeResult, TlsChain};

/// Checks in flight at once. A check mostly waits on the network; this bounds sockets and
/// memory when many targets hang until their timeout.
const CONCURRENCY: usize = 256;

/// When a monitor's schedule runs next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// One interval after this round was due.
    Interval,
    /// One retry interval after this round was due: the probe or monitor is pending.
    Retry,
    /// At this time, Unix milliseconds: a push monitor's deadline.
    At(i64),
}

/// Where a schedule's first round falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Now,
    /// The monitor's slot on its interval grid.
    Phase,
    /// One interval from now, as a push monitor waits for its first push.
    AfterInterval,
}

impl Start {
    /// At once for a monitor changed since the probe started, else in its slot.
    pub fn for_change(updated_at: i64, probe_started: i64) -> Self {
        if updated_at < probe_started {
            Start::Phase
        } else {
            Start::Now
        }
    }
}

/// One monitor's due times.
pub struct Grid {
    interval_ms: i64,
    retry_interval_ms: i64,
    due: i64,
}

impl Grid {
    pub fn new(
        monitor: MonitorId,
        interval_ms: i64,
        retry_interval_ms: i64,
        start: Start,
        now: i64,
    ) -> Self {
        let due = match start {
            Start::Now => now,
            Start::Phase => now + phase_wait(monitor, interval_ms, now),
            Start::AfterInterval => now + interval_ms,
        };
        Self {
            interval_ms,
            retry_interval_ms,
            due,
        }
    }

    pub fn due(&self) -> i64 {
        self.due
    }

    pub fn advance(&mut self, next: Next, now: i64) {
        let step = match next {
            Next::At(at) => {
                self.due = at;
                return;
            }
            Next::Retry => self.retry_interval_ms,
            Next::Interval => self.interval_ms,
        };
        self.due += step;
        // More than a step behind: the missed rounds are gone, start again from now. More than
        // a step ahead: the clock went back (or stands still), so wait one step from now.
        if now - self.due > step {
            self.due = now;
        } else if self.due - now > step {
            self.due = now + step;
        }
    }
}

/// How late checks start, for health and the scheduler measurement.
#[derive(Debug, Default)]
pub struct Lag {
    started: AtomicU64,
    late: AtomicU64,
}

impl Lag {
    /// Checks started, and those that started more than one interval after they were due.
    pub fn read(&self) -> (u64, u64) {
        (
            self.started.load(Ordering::Relaxed),
            self.late.load(Ordering::Relaxed),
        )
    }

    fn record(&self, late_ms: i64, interval_ms: i64) {
        self.started.fetch_add(1, Ordering::Relaxed);
        if late_ms > interval_ms {
            self.late.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Runs one probe's checks.
pub struct Checker {
    checks: CheckContext,
    permits: Semaphore,
    lag: Arc<Lag>,
}

impl Checker {
    pub fn new(lag: Arc<Lag>) -> Self {
        Self {
            checks: CheckContext::new(),
            permits: Semaphore::new(CONCURRENCY),
            lag,
        }
    }

    /// Runs the round due at `due` and applies the probe's retries; `retries` holds the
    /// failures in a row before and after.
    pub async fn check(
        &self,
        assignment: &Assignment,
        retries: &mut u32,
        due: i64,
        clock: &Clock,
    ) -> ProbeResult {
        let permit = self.permits.acquire().await.expect("never closed");
        let started = clock.now_ms();
        self.lag.record(started - due, assignment.interval_ms);
        let result = v1per_checks::run(&assignment.definition, &self.checks).await;
        drop(permit);
        let round = match (result.status, &result.failure) {
            (v1per_checks::Status::Up, _) => Round::Pass,
            (_, Some(Failure::JsonQueryMismatch)) if assignment.down_on_query_mismatch => {
                Round::FailNow
            }
            _ => Round::Fail,
        };
        let (status, retried) = retry(round, *retries, assignment.max_retries);
        *retries = retried;
        ProbeResult {
            monitor: assignment.monitor,
            revision: assignment.revision,
            time: started,
            status,
            retries: retried,
            latency_ms: result
                .latency_ms
                .map(|ms| u32::try_from(ms).unwrap_or(u32::MAX)),
            message: result.message,
            http_status: result.http_status,
            cert_expiry: result
                .tls
                .as_ref()
                .and_then(|tls| tls.chain.first())
                .map(|leaf| leaf.valid_to.timestamp_millis()),
            tls: result.tls.map(|tls| TlsChain {
                valid: tls.valid,
                certificates: tls
                    .chain
                    .into_iter()
                    .map(|cert| ChainCertificate {
                        subject: cert.subject,
                        cert_type: cert.cert_type,
                        valid_to: cert.valid_to.timestamp_millis(),
                        fingerprint: cert.fingerprint256,
                    })
                    .collect(),
            }),
        }
    }
}

/// How long until `monitor`'s slot on its interval grid: a stable phase from its ID.
fn phase_wait(monitor: MonitorId, interval_ms: i64, now: i64) -> i64 {
    let phase = (spread(monitor.get() as u64) % interval_ms.max(1) as u64) as i64;
    (phase - now).rem_euclid(interval_ms.max(1))
}

/// SplitMix64's finalizer: neighbouring IDs land far apart.
fn spread(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_are_stable_and_spread_over_the_interval() {
        let interval = 60_000;
        let now = 1_700_000_000_123;
        let waits: Vec<i64> = (1..=2000)
            .map(|id| phase_wait(MonitorId::new(id), interval, now))
            .collect();
        assert!(waits.iter().all(|wait| (0..interval).contains(wait)));
        assert_eq!(waits[0], phase_wait(MonitorId::new(1), interval, now));
        // Ten equal slices of the interval each hold roughly a tenth of the monitors.
        for slice in 0..10 {
            let range = slice * 6_000..(slice + 1) * 6_000;
            let count = waits.iter().filter(|wait| range.contains(*wait)).count();
            assert!((150..250).contains(&count), "slice {slice}: {count}");
        }
        // The slot is the same wall-clock phase whenever it is computed.
        let later = now + 12_345;
        assert_eq!(
            (now + waits[0]) % interval,
            (later + phase_wait(MonitorId::new(1), interval, later)) % interval
        );
    }
}
