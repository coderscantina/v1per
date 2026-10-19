//! Services shared by every request, composed once at startup.

use std::ops::Deref;
use std::sync::{Arc, Mutex};

use v1per_protocol::limits::{
    LOGIN_ATTEMPTS_PER_MINUTE, LOGIN_CHALLENGE_ATTEMPTS, PAGE_ACCESS_ATTEMPTS_PER_MINUTE,
};
use v1per_storage::Storage;

use crate::clock::Clock;
use crate::config::Config;
use crate::db::Db;
use crate::events::Hub;
use crate::identity::secrets::SecretKey;
use crate::identity::{ChallengeAttempts, RateLimiter};
use crate::monitors::{Evaluator, Queue};
use crate::pages::SnapshotCache;
use crate::probes::schedule::Lag;

/// Pushes one push token may send per minute.
const PUSHES_PER_TOKEN_PER_MINUTE: u32 = 120;
/// Unknown push tokens one address may try per minute.
const PUSH_MISSES_PER_MINUTE: u32 = 30;
/// Unknown probe tokens one address may try per minute.
const PROBE_MISSES_PER_MINUTE: u32 = 30;
/// Sign-in links one address may be sent per minute.
const PAGE_MAILS_PER_MINUTE: u32 = 3;
/// SMTP test mails one instance admin may queue per minute.
const SMTP_TESTS_PER_MINUTE: u32 = 5;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub db: Db,
    pub config: Config,
    pub clock: Clock,
    pub login_limiter: RateLimiter,
    pub challenge_attempts: ChallengeAttempts,
    /// Seals monitor credentials, notification tokens and other secrets at rest (decision 0006).
    pub secrets: SecretKey,
    /// Wakes open subscriptions after a command commits.
    pub hub: Hub,
    /// Heartbeats and rollups: the durable result inbox and DuckDB.
    pub storage: Arc<Storage>,
    /// Decides monitor status from check results, rounds and pushes.
    pub evaluator: Evaluator,
    /// The evaluator's work queue until background work takes it.
    pub(crate) evaluator_queue: Mutex<Option<Queue>>,
    pub push_tokens: RateLimiter,
    pub push_misses: RateLimiter,
    pub probe_misses: RateLimiter,
    /// How late the built-in probe's checks start.
    pub builtin_lag: Arc<Lag>,
    /// When this state was composed: probes count as live for their first intervals after it.
    pub started_at: i64,
    /// Wakes the notification outbox after the evaluator committed messages.
    pub outbox: tokio::sync::Notify,
    /// Status page passwords, sign-in link requests and codes, per client address.
    pub page_attempts: RateLimiter,
    /// Sign-in links per recipient address.
    pub page_mails: RateLimiter,
    pub page_snapshots: SnapshotCache,
    /// `instance.testSmtp` per person.
    pub smtp_tests: RateLimiter,
}

impl AppState {
    pub fn new(
        db: Db,
        config: Config,
        clock: Clock,
        secrets: SecretKey,
        storage: Arc<Storage>,
    ) -> Self {
        let (evaluator, queue) = Evaluator::new();
        let started_at = clock.now_ms();
        Self(Arc::new(Inner {
            db,
            config,
            clock,
            login_limiter: RateLimiter::per_minute(LOGIN_ATTEMPTS_PER_MINUTE),
            challenge_attempts: ChallengeAttempts::new(LOGIN_CHALLENGE_ATTEMPTS),
            secrets,
            hub: Hub::default(),
            storage,
            evaluator,
            evaluator_queue: Mutex::new(Some(queue)),
            push_tokens: RateLimiter::per_minute(PUSHES_PER_TOKEN_PER_MINUTE),
            push_misses: RateLimiter::per_minute(PUSH_MISSES_PER_MINUTE),
            probe_misses: RateLimiter::per_minute(PROBE_MISSES_PER_MINUTE),
            builtin_lag: Arc::new(Lag::default()),
            started_at,
            outbox: tokio::sync::Notify::new(),
            page_attempts: RateLimiter::per_minute(PAGE_ACCESS_ATTEMPTS_PER_MINUTE),
            page_mails: RateLimiter::per_minute(PAGE_MAILS_PER_MINUTE),
            page_snapshots: SnapshotCache::default(),
            smtp_tests: RateLimiter::per_minute(SMTP_TESTS_PER_MINUTE),
        }))
    }
}

impl Deref for AppState {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        &self.0
    }
}
