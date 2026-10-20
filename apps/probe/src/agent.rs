//! The agent loop (decisions 0003 and 0011). It only connects out to the server: it long-polls
//! its assignment list, runs the shared scheduler and checks, seals results every second, and
//! delivers them oldest first through the [`Buffer`], which keeps them on disk while the server
//! is unreachable. Credentials stay in memory only, so an agent started while the server is
//! unreachable checks nothing until it gets its list.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::{Notify, mpsc, watch};
use tokio::task::JoinHandle;
use url::Url;
use v1per_probe_core::clock::Clock;
use v1per_probe_core::now_ms;
use v1per_probe_core::schedule::{Checker, Grid, Lag, Start};
use v1per_probe_core::wire::{
    Assignment, AssignmentList, AssignmentsRequest, BATCH_RESULTS_MAX, BatchAck, Health, PROTOCOL,
    PROTOCOL_HEADER, ProbeResult, Refusal, ResultBatch,
};
use v1per_protocol::ids::MonitorId;

use crate::buffer::Buffer;

/// How long results gather before they are sealed into one batch.
const SEAL_EVERY: Duration = Duration::from_secs(1);
/// Client limit of an assignment poll: the server's long poll plus margin.
const POLL_TIMEOUT: Duration = Duration::from_secs(40);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(30);

/// Runs until `shutdown`, or returns the server's refusal for good (revoked token, unsupported
/// protocol). Undelivered results are on disk either way.
pub async fn run(
    server: Url,
    token: String,
    buffer: Buffer,
    shutdown: impl Future<Output = ()>,
) -> Result<(), String> {
    let buffer = Arc::new(buffer);
    let server = Server::new(server, token);
    tracing::info!(server = %server.base, buffered = buffer.buffered(), "probe agent starting");

    let lag = Arc::new(Lag::default());
    let (assigned, assignments) = watch::channel(Vec::new());
    let (results, collected) = mpsc::channel(4 * BATCH_RESULTS_MAX);
    let sealed = Arc::new(Notify::new());
    let (stop, stopped) = watch::channel(false);
    let scheduler = tokio::spawn(schedule(assignments, results, lag.clone(), stopped));
    let sealer = tokio::spawn(seal(collected, buffer.clone(), sealed.clone()));
    let outcome = tokio::select! {
        refused = poll(&server, &buffer, &lag, assigned) => Err(refused),
        refused = deliver(&server, &buffer, &sealed) => Err(refused),
        _ = shutdown => Ok(()),
    };
    // Checks stop first; the sealer then seals what they produced and ends.
    let _ = stop.send(true);
    let _ = scheduler.await;
    let _ = sealer.await;
    if let Err(error) = buffer.spill() {
        tracing::error!(%error, "writing the result buffer failed; undelivered results are lost");
    }
    tracing::info!(buffered = buffer.buffered(), "probe agent stopped");
    outcome
}

/// Why a request failed.
enum Failed {
    /// The server is unreachable or busy: try again later.
    Retry(String),
    /// The server refuses this agent for good.
    Refused(String),
    /// The server refuses this request; sending it again would not help.
    Rejected(String),
}

struct Server {
    client: reqwest::Client,
    base: Url,
    token: String,
}

impl Server {
    fn new(mut base: Url, token: String) -> Self {
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .user_agent(concat!("v1per-probe/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("a client with a preconfigured rustls backend builds");
        Self {
            client,
            base,
            token,
        }
    }

    async fn post<B: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        timeout: Duration,
    ) -> Result<R, Failed> {
        let url = self.base.join(path).expect("a relative API path joins");
        let response = self
            .client
            .post(url)
            .timeout(timeout)
            .bearer_auth(&self.token)
            .header(PROTOCOL_HEADER, PROTOCOL)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(body).expect("requests serialize"))
            .send()
            .await
            .map_err(|error| Failed::Retry(error.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| Failed::Retry(error.to_string()))?;
        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map_err(|error| Failed::Retry(format!("unreadable answer: {error}")));
        }
        let message = serde_json::from_slice::<Refusal>(&bytes)
            .map(|refusal| refusal.message)
            .unwrap_or_else(|_| format!("HTTP {status}"));
        Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::UPGRADE_REQUIRED => Failed::Refused(message),
            StatusCode::TOO_MANY_REQUESTS | StatusCode::REQUEST_TIMEOUT => Failed::Retry(message),
            status if status.is_server_error() => Failed::Retry(message),
            _ => Failed::Rejected(message),
        })
    }
}

/// Waits that double from [`RETRY_FIRST`] up to [`RETRY_MAX`].
struct Backoff(Duration);

impl Backoff {
    fn new() -> Self {
        Self(RETRY_FIRST)
    }

    async fn wait(&mut self) {
        tokio::time::sleep(self.0).await;
        self.0 = (self.0 * 2).min(RETRY_MAX);
    }

    fn reset(&mut self) {
        self.0 = RETRY_FIRST;
    }
}

/// Tells the buffer whether the server answers; going offline spills what waits in memory.
fn reachable(buffer: &Buffer, reachable: bool) {
    if let Err(error) = buffer.set_offline(!reachable) {
        tracing::error!(%error, "writing the result buffer failed");
    }
}

/// Polls the assignment list with the agent's health; returns the server's final refusal.
async fn poll(
    server: &Server,
    buffer: &Buffer,
    lag: &Lag,
    assigned: watch::Sender<Vec<Assignment>>,
) -> String {
    let mut version = None;
    let mut backoff = Backoff::new();
    loop {
        let (checks_started, checks_late) = lag.read();
        let request = AssignmentsRequest {
            version: version.clone(),
            sent_at: now_ms(),
            health: Health {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                platform: Some(v1per_probe_core::wire::platform()),
                checks_started,
                checks_late,
                buffered_results: buffer.buffered(),
                dropped_results: buffer.dropped(),
            },
        };
        match server
            .post::<_, AssignmentList>("api/probe/assignments", &request, POLL_TIMEOUT)
            .await
        {
            Ok(list) => {
                backoff.reset();
                reachable(buffer, true);
                if let Some(assignments) = list.assignments {
                    tracing::info!(monitors = assignments.len(), version = %list.version, "new assignments");
                    assigned.send_replace(assignments);
                }
                version = Some(list.version);
            }
            Err(Failed::Refused(message)) => return message,
            Err(Failed::Retry(message)) => {
                tracing::warn!(%message, "polling assignments failed; retrying");
                reachable(buffer, false);
                backoff.wait().await;
            }
            Err(Failed::Rejected(message)) => {
                tracing::warn!(%message, "the server rejected the poll; retrying");
                backoff.wait().await;
            }
        }
    }
}

/// Delivers sealed batches oldest first, each until the server acknowledges it; returns the
/// server's final refusal.
async fn deliver(server: &Server, buffer: &Buffer, sealed: &Notify) -> String {
    let mut backoff = Backoff::new();
    loop {
        let batch = match buffer.oldest() {
            Ok(Some(batch)) => batch,
            Ok(None) => {
                sealed.notified().await;
                continue;
            }
            Err(error) => {
                tracing::error!(%error, "reading the result buffer failed; retrying");
                backoff.wait().await;
                continue;
            }
        };
        let body = ResultBatch {
            batch: batch.id.clone(),
            sent_at: now_ms(),
            results: batch.results.clone(),
        };
        let delivered = match server
            .post::<_, BatchAck>("api/probe/results", &body, REQUEST_TIMEOUT)
            .await
        {
            Ok(_) => true,
            Err(Failed::Refused(message)) => return message,
            Err(Failed::Rejected(message)) => {
                tracing::error!(batch = %batch.id, %message, "the server rejected a batch; dropping it");
                true
            }
            Err(Failed::Retry(message)) => {
                tracing::warn!(%message, buffered = buffer.buffered(), "delivering results failed; retrying");
                false
            }
        };
        reachable(buffer, delivered);
        if !delivered {
            backoff.wait().await;
        } else if let Err(error) = buffer.delivered(&batch) {
            // The next attempt resends it and the server answers it as a repeat.
            tracing::error!(%error, "marking a delivered batch failed");
            backoff.wait().await;
        } else {
            backoff.reset();
        }
    }
}

/// Seals results once a second, or sooner when a batch is full, until every check task is
/// gone.
async fn seal(mut results: mpsc::Receiver<ProbeResult>, buffer: Arc<Buffer>, sealed: Arc<Notify>) {
    while let Some(first) = results.recv().await {
        let mut pending = vec![first];
        let deadline = tokio::time::Instant::now() + SEAL_EVERY;
        while pending.len() < BATCH_RESULTS_MAX {
            match tokio::time::timeout_at(deadline, results.recv()).await {
                Ok(Some(result)) => pending.push(result),
                Ok(None) | Err(_) => break,
            }
        }
        if let Err(error) = buffer.push(pending) {
            // The batch stays in memory and spills with the next one.
            tracing::error!(%error, "writing the result buffer failed");
        }
        sealed.notify_one();
    }
}

/// Keeps one check task per assignment until `stop`.
async fn schedule(
    mut assignments: watch::Receiver<Vec<Assignment>>,
    results: mpsc::Sender<ProbeResult>,
    lag: Arc<Lag>,
    mut stop: watch::Receiver<bool>,
) {
    let checker = Arc::new(Checker::new(lag));
    let started = now_ms();
    let mut tasks: HashMap<MonitorId, (i64, JoinHandle<()>)> = HashMap::new();
    loop {
        let mut wanted: HashMap<MonitorId, Assignment> = assignments
            .borrow_and_update()
            .iter()
            .map(|assignment| (assignment.monitor, assignment.clone()))
            .collect();
        tasks.retain(|monitor, (revision, task)| {
            let keep = wanted
                .get(monitor)
                .is_some_and(|wanted| wanted.revision == *revision);
            if !keep {
                task.abort();
            }
            keep
        });
        wanted.retain(|monitor, _| !tasks.contains_key(monitor));
        for (monitor, assignment) in wanted {
            let revision = assignment.revision;
            let task = tokio::spawn(check(assignment, checker.clone(), results.clone(), started));
            tasks.insert(monitor, (revision, task));
        }
        tokio::select! {
            changed = assignments.changed() => if changed.is_err() { break },
            _ = stop.changed() => break,
        }
    }
    for (_, task) in tasks.into_values() {
        task.abort();
        let _ = task.await;
    }
}

/// One monitor's checks, forever. Retries start from zero with every new revision.
async fn check(
    assignment: Assignment,
    checker: Arc<Checker>,
    results: mpsc::Sender<ProbeResult>,
    started: i64,
) {
    let clock = Clock::system();
    let mut retries = 0;
    let mut grid = Grid::new(
        assignment.monitor,
        assignment.interval_ms,
        assignment.retry_interval_ms,
        Start::for_change(assignment.updated_at, started),
        clock.now_ms(),
    );
    loop {
        clock.sleep_until(grid.due()).await;
        let result = checker
            .check(&assignment, &mut retries, grid.due(), &clock)
            .await;
        let next = result.next();
        if results.send(result).await.is_err() {
            return;
        }
        grid.advance(next, clock.now_ms());
    }
}
