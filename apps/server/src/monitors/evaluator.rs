//! Monitor evaluation (decision 0008). Every result that can change a status passes through
//! one task: probe results (the built-in probe's and agent batches), the server's own rounds for
//! push, group and manual monitors, pushes, and a sweep for probes and monitors that went quiet.
//! It takes whatever is queued, evaluates it in one write transaction, admits the heartbeats
//! together with the monitor-level statuses durably, then commits, so the state never runs ahead
//! of recorded history and one commit serves many results.
//!
//! Rounds with several probes: each current probe result updates that probe's state and the
//! quorum over the monitor's live probes. A monitor-level beat, with its entry in the
//! monitor-level series, is recorded for every result of the lead probe, the live probe with
//! the lowest ID, and for any result that changes the monitor's status. So the series keeps one
//! entry per lead round however many probes check the monitor, as Kuma keeps one beat per
//! check, and a change never waits for the lead. A result that waited at its agent longer than
//! an interval is history only: its heartbeat is stored, but it changes no state.
//!
//! A probe is live for a monitor while it is online, not revoked, still assigned and its latest
//! result arrived within three intervals; the first three intervals after the server starts
//! count as recent. With no live probe the sweep sets the monitor pending once, without a
//! series entry. A remote probe without contact for 90 seconds goes offline, which the probe
//! journal records.
//!
//! Per batch it writes per-probe and monitor state, journals important beats and repeated down
//! alerts in `monitor_transitions`, records `monitor.*` events on the node's monitor topics and
//! `probe.offline` on `probes:<id>`. Subscribers are woken once the batch's heartbeats are
//! readable from storage, so a screen that reloads on an event sees the beat behind it.
//! A transition the journal records also queues its notifications and opens or resolves the
//! monitor's auto-incident, in the same transaction. Monitors under maintenance are not checked:
//! the built-in probe runs a round for them that records a maintenance beat, results still in
//! flight are dropped, and pushes are stored as maintenance.

use std::time::Duration;

use serde_json::json;
use sqlx::SqliteConnection;
use tokio::sync::{mpsc, oneshot, watch};
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{MonitorId, NodeId, ProbeId};
use v1per_protocol::operations::monitors::{JsonQuerySettings, KeywordSettings, MonitorSettings};
use v1per_storage::{self as storage, BatchId, Heartbeat, MonitorStatus, Refusal, Status};

use super::rules::{self, Member, Retried, Verdict};
use super::{from_text, status_code, status_from_code, status_from_wire, wire_status};
use crate::events;
use crate::incidents;
use crate::notifications;
use crate::pages::{self, Change};
use crate::probes::schedule::Next;
use crate::probes::wire::ProbeResult;
use crate::probes::{self, OFFLINE_AFTER_MS};
use crate::state::AppState;

/// Results waiting for the evaluator. Probe tasks wait when it is full.
const QUEUE: usize = 8_192;
/// Work items evaluated in one transaction.
const BATCH: usize = 512;
/// Kuma's allowance for clock differences on push deadlines.
const PUSH_GRACE_MS: i64 = 1_000;
/// How often quiet probes and monitors are looked for.
const SWEEP_MS: i64 = 5_000;
/// Intervals without a result after which a probe leaves a monitor's quorum.
const LIVE_INTERVALS: i64 = 3;
/// Longest wait for heartbeats to become readable before subscribers are woken anyway, so a
/// stalled transfer delays live screens but never silences them.
const READABLE_WAIT: Duration = Duration::from_secs(5);

/// A push as Kuma's push URL reports it.
pub struct Push {
    pub up: bool,
    pub message: String,
    pub latency_ms: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PushRefusal {
    /// No active monitor has the token.
    NotFound,
    /// The push could not be recorded; the sender may retry.
    Unavailable,
}

/// Why an agent batch was not admitted. Nothing of it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchRefusal {
    /// Retry later.
    Unavailable,
    /// The batch ID was admitted before with other results.
    Conflict,
}

/// An agent's batch ID and its clock when it sent the batch.
struct AgentBatch {
    id: String,
    sent_at: i64,
}

type BatchReply = oneshot::Sender<Result<bool, BatchRefusal>>;

enum Work {
    Results {
        probe: ProbeId,
        results: Vec<ProbeResult>,
        /// `None` for the built-in probe.
        batch: Option<AgentBatch>,
        reply: BatchReply,
    },
    Round {
        monitor: MonitorId,
        revision: i64,
        at: i64,
        reply: oneshot::Sender<Next>,
    },
    Push {
        token_digest: String,
        push: Push,
        at: i64,
        reply: oneshot::Sender<Result<(), PushRefusal>>,
    },
    Sweep,
}

/// Hands results to the evaluator task and waits for their outcome.
#[derive(Clone)]
pub struct Evaluator {
    work: mpsc::Sender<Work>,
}

/// The receiving end, taken once by [`run`].
pub struct Queue(mpsc::Receiver<Work>);

impl Evaluator {
    pub fn new() -> (Self, Queue) {
        let (work, queue) = mpsc::channel(QUEUE);
        (Self { work }, Queue(queue))
    }

    /// Evaluates one result of the built-in probe. A result for an older revision, a paused,
    /// deleted or unassigned monitor is dropped.
    pub async fn result(&self, probe: ProbeId, result: ProbeResult) {
        let (reply, outcome) = oneshot::channel();
        let work = Work::Results {
            probe,
            results: vec![result],
            batch: None,
            reply,
        };
        if self.work.send(work).await.is_ok() {
            let _ = outcome.await;
        }
    }

    /// Admits an agent's batch and evaluates its results, once per batch ID: true when the ID
    /// was admitted before and nothing new was stored.
    pub async fn batch(
        &self,
        probe: ProbeId,
        id: String,
        sent_at: i64,
        results: Vec<ProbeResult>,
    ) -> Result<bool, BatchRefusal> {
        let (reply, outcome) = oneshot::channel();
        let work = Work::Results {
            probe,
            results,
            batch: Some(AgentBatch { id, sent_at }),
            reply,
        };
        if self.work.send(work).await.is_err() {
            return Err(BatchRefusal::Unavailable);
        }
        outcome.await.unwrap_or(Err(BatchRefusal::Unavailable))
    }

    /// Runs one round of a monitor the server evaluates itself: a push deadline, a group or
    /// a manual monitor.
    pub async fn round(&self, monitor: MonitorId, revision: i64, at: i64) -> Next {
        let (reply, outcome) = oneshot::channel();
        let work = Work::Round {
            monitor,
            revision,
            at,
            reply,
        };
        if self.work.send(work).await.is_err() {
            return Next::Interval;
        }
        outcome.await.unwrap_or(Next::Interval)
    }

    /// Records a push for the active monitor whose push token has `token_digest`. Returns
    /// after the heartbeat is durable.
    pub async fn push(&self, token_digest: String, push: Push, at: i64) -> Result<(), PushRefusal> {
        let (reply, outcome) = oneshot::channel();
        let work = Work::Push {
            token_digest,
            push,
            at,
            reply,
        };
        if self.work.send(work).await.is_err() {
            return Err(PushRefusal::Unavailable);
        }
        outcome.await.unwrap_or(Err(PushRefusal::Unavailable))
    }
}

/// Evaluates queued work and sweeps every few seconds until `stop`, then finishes what is
/// already queued.
pub async fn run(state: AppState, mut queue: Queue, mut stop: watch::Receiver<bool>) {
    let mut sweep_at = state.clock.now_ms() + SWEEP_MS;
    loop {
        let first = tokio::select! {
            work = queue.0.recv() => match work {
                Some(work) => work,
                None => return,
            },
            _ = state.clock.sleep_until(sweep_at) => {
                sweep_at = state.clock.now_ms() + SWEEP_MS;
                Work::Sweep
            }
            _ = stop.changed() => break,
        };
        let batch = gather(first, &mut queue);
        evaluate(&state, batch).await;
    }
    queue.0.close();
    while let Some(first) = queue.0.recv().await {
        let batch = gather(first, &mut queue);
        evaluate(&state, batch).await;
    }
}

fn gather(first: Work, queue: &mut Queue) -> Vec<Work> {
    let mut batch = vec![first];
    while batch.len() < BATCH {
        match queue.0.try_recv() {
            Ok(work) => batch.push(work),
            Err(_) => break,
        }
    }
    batch
}

/// Heartbeats and monitor-level statuses admitted together.
#[derive(Default)]
struct Part {
    heartbeats: Vec<Heartbeat>,
    statuses: Vec<MonitorStatus>,
}

impl Part {
    fn is_empty(&self) -> bool {
        self.heartbeats.is_empty() && self.statuses.is_empty()
    }
}

/// One agent batch, admitted under its own ID.
struct Admission {
    id: BatchId,
    part: Part,
    reply: BatchReply,
    /// Storage refused the ID as reused with other results.
    conflict: bool,
}

/// What a batch produced, delivered only once it committed.
#[derive(Default)]
struct Outcome {
    /// Everything without an agent batch ID, admitted as one inbox row.
    plain: Part,
    batches: Vec<Admission>,
    topics: Vec<Topic>,
    replies: Vec<Reply>,
}

enum Reply {
    Next(oneshot::Sender<Next>, Next),
    Push(oneshot::Sender<Result<(), PushRefusal>>),
    Results(BatchReply),
}

/// The evaluator's clock reading for one transaction.
#[derive(Clone, Copy)]
struct Moment {
    now: i64,
    /// When the server started; liveness counts from here at the earliest.
    started: i64,
}

async fn evaluate(state: &AppState, batch: Vec<Work>) {
    let mut outcome = Outcome::default();
    match apply(state, batch, &mut outcome).await {
        Ok(admitted) => {
            state.outbox.notify_one();
            wake_when_readable(state, admitted, std::mem::take(&mut outcome.topics));
            for admission in outcome.batches {
                let outcome = if admission.conflict {
                    Err(BatchRefusal::Conflict)
                } else {
                    Ok(false)
                };
                let _ = admission.reply.send(outcome);
            }
            for reply in outcome.replies {
                match reply {
                    Reply::Next(reply, next) => {
                        let _ = reply.send(next);
                    }
                    Reply::Push(reply) => {
                        let _ = reply.send(Ok(()));
                    }
                    Reply::Results(reply) => {
                        let _ = reply.send(Ok(false));
                    }
                }
            }
        }
        // Dropping the replies tells every sender the batch was not recorded.
        Err(error) => tracing::error!(%error, "evaluating a batch of results failed"),
    }
}

/// Wakes the subscribers of `topics` once the watermark covers `admitted`, the highest inbox
/// sequence of the batch, or after [`READABLE_WAIT`].
fn wake_when_readable(state: &AppState, admitted: Option<i64>, topics: Vec<Topic>) {
    if topics.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Some(sequence) = admitted {
            let mut watermark = state.storage.watermark();
            let readable = watermark.wait_for(|watermark| watermark.sequence >= sequence);
            let _ = tokio::time::timeout(READABLE_WAIT, readable).await;
        }
        for topic in &topics {
            state.hub.publish(topic);
        }
    });
}

/// Applies a batch and returns the highest inbox sequence it admitted.
async fn apply(
    state: &AppState,
    batch: Vec<Work>,
    out: &mut Outcome,
) -> anyhow::Result<Option<i64>> {
    let at = Moment {
        now: state.clock.now_ms(),
        started: state.started_at,
    };
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    for work in batch {
        match work {
            Work::Results {
                probe,
                results,
                batch: Some(batch),
                reply,
            } => {
                if let Some(topic) =
                    probes::touch(&mut tx, probe, at.now, batch.sent_at, None).await?
                {
                    out.topics.push(topic);
                }
                let id = BatchId {
                    probe: storage::ProbeId(probe.get()),
                    id: batch.id,
                };
                // A resend of a batch in this transaction is answered by its retry.
                if out.batches.iter().any(|queued| queued.id == id) {
                    drop(reply);
                    continue;
                }
                match state.storage.admitted(&id).await {
                    Ok(Some(_)) => {
                        let _ = reply.send(Ok(true));
                        continue;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::error!(%error, "reading agent batch IDs failed");
                        let _ = reply.send(Err(BatchRefusal::Unavailable));
                        continue;
                    }
                }
                let mut part = Part::default();
                for result in results {
                    probe_result(
                        &mut tx,
                        at,
                        probe,
                        result,
                        batch.sent_at,
                        &mut part,
                        &mut out.topics,
                    )
                    .await?;
                }
                out.batches.push(Admission {
                    id,
                    part,
                    reply,
                    conflict: false,
                });
            }
            Work::Results {
                probe,
                results,
                batch: None,
                reply,
            } => {
                for result in results {
                    let sent_at = result.time;
                    probe_result(
                        &mut tx,
                        at,
                        probe,
                        result,
                        sent_at,
                        &mut out.plain,
                        &mut out.topics,
                    )
                    .await?;
                }
                out.replies.push(Reply::Results(reply));
            }
            Work::Round {
                monitor,
                revision,
                at: round_at,
                reply,
            } => {
                let next = round(&mut tx, monitor, revision, round_at, out).await?;
                out.replies.push(Reply::Next(reply, next));
            }
            Work::Push {
                token_digest,
                push,
                at: push_at,
                reply,
            } => {
                if push_beat(&mut tx, &token_digest, push, push_at, out).await? {
                    out.replies.push(Reply::Push(reply));
                } else {
                    let _ = reply.send(Err(PushRefusal::NotFound));
                }
            }
            Work::Sweep => sweep(&mut tx, at, &mut out.topics).await?,
        }
    }
    let admitted = admit(state, out).await?;
    tx.commit().await?;
    Ok(admitted)
}

/// Admits every part, concurrently so the storage group commit usually writes them in one
/// transaction. Returns the highest sequence admitted.
async fn admit(state: &AppState, out: &mut Outcome) -> anyhow::Result<Option<i64>> {
    let storage = &state.storage;
    let plain = async {
        if out.plain.is_empty() {
            return Ok(None);
        }
        storage
            .admit_evaluated(None, &out.plain.heartbeats, &out.plain.statuses)
            .await
            .map(|admitted| Some(admitted.sequence))
    };
    let batches = futures_util::future::join_all(out.batches.iter().map(|batch| {
        storage.admit_evaluated(
            Some(batch.id.clone()),
            &batch.part.heartbeats,
            &batch.part.statuses,
        )
    }));
    let (plain, batches) = tokio::join!(plain, batches);
    let mut highest = plain?;
    for (admitted, batch) in batches.into_iter().zip(&mut out.batches) {
        // An ID reused with other results is refused alone; its evaluation still commits with
        // the rest, as a resend would have been skipped.
        batch.conflict = admitted == Err(Refusal::BatchConflict);
        if !batch.conflict {
            highest = highest.max(Some(admitted?.sequence));
        }
    }
    Ok(highest)
}

/// The monitor fields evaluation needs.
#[derive(sqlx::FromRow)]
struct Target {
    id: i64,
    node_id: i64,
    #[sqlx(rename = "type")]
    kind: String,
    settings: String,
    active: bool,
    revision: i64,
    interval: i64,
    retry_interval: i64,
    max_retries: i64,
    resend_interval: i64,
    upside_down: bool,
    quorum: String,
    manual_status: Option<i64>,
}

/// `SELECT <every Target column> FROM monitors`, for `concat!` with a condition.
macro_rules! select_target {
    () => {
        "SELECT id, node_id, type, settings, active, revision, interval, retry_interval,
                max_retries, resend_interval, upside_down, quorum, manual_status
         FROM monitors"
    };
}

impl Target {
    fn monitor(&self) -> MonitorId {
        MonitorId::new(self.id)
    }

    fn max_retries(&self) -> u32 {
        self.max_retries as u32
    }

    /// How long a probe's result keeps it live: three of its longest gaps between checks.
    fn live_window_ms(&self) -> i64 {
        LIVE_INTERVALS * self.interval.max(self.retry_interval) * 1000
    }

    /// The URL of an http-type monitor with certificate expiry alerts on.
    fn certificate_alerts(&self) -> Option<String> {
        match serde_json::from_str::<MonitorSettings>(&self.settings).ok()? {
            MonitorSettings::Http(http)
            | MonitorSettings::Keyword(KeywordSettings { request: http, .. })
            | MonitorSettings::JsonQuery(JsonQuerySettings { request: http, .. })
                if http.expiry_notification && !http.ignore_tls =>
            {
                Some(http.url)
            }
            _ => None,
        }
    }

    fn runs_on_probes(&self) -> bool {
        serde_json::from_str::<MonitorSettings>(&self.settings)
            .is_ok_and(|settings| super::runs_on_probes(&settings))
    }
}

/// The current target of a scheduled round, unless it changed or stopped since.
async fn current(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
    revision: i64,
) -> sqlx::Result<Option<Target>> {
    let target: Option<Target> = sqlx::query_as(concat!(select_target!(), " WHERE id = ?"))
        .bind(monitor.get())
        .fetch_optional(conn)
        .await?;
    Ok(target.filter(|t| t.active && t.revision == revision))
}

/// The monitor-level state before this beat.
#[derive(sqlx::FromRow)]
struct Previous {
    status: i64,
    retries: i64,
    down_count: i64,
    beat_at: i64,
    changed_at: i64,
    no_live_probe: bool,
}

async fn previous(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
) -> sqlx::Result<Option<Previous>> {
    sqlx::query_as(
        "SELECT status, retries, down_count, beat_at, changed_at, no_live_probe
         FROM monitor_state WHERE monitor_id = ?",
    )
    .bind(monitor.get())
    .fetch_optional(conn)
    .await
}

/// A probe whose latest result counts in a monitor's quorum.
#[derive(sqlx::FromRow)]
pub(crate) struct LiveProbe {
    probe: i64,
    status: i64,
    message: String,
    latency_ms: Option<i64>,
    retries: i64,
}

impl LiveProbe {
    pub fn probe(&self) -> ProbeId {
        ProbeId::new(self.probe)
    }
}

/// The monitor's live probes, by ID: online, not revoked, assigned, and reported within
/// `window_ms` before `now`, where the time before `started` counts as `started`.
pub(crate) async fn live_probes(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
    window_ms: i64,
    now: i64,
    started: i64,
) -> sqlx::Result<Vec<LiveProbe>> {
    sqlx::query_as(
        "SELECT ps.probe_id AS probe, ps.status, ps.message, ps.latency_ms, ps.retries
         FROM probe_state ps
         JOIN monitor_probes mp ON mp.monitor_id = ps.monitor_id AND mp.probe_id = ps.probe_id
         JOIN probes p ON p.id = ps.probe_id
         WHERE ps.monitor_id = ? AND p.revoked_at IS NULL AND p.offline_since IS NULL
           AND max(ps.received_at, ?) >= ?
         ORDER BY ps.probe_id",
    )
    .bind(monitor.get())
    .bind(started)
    .bind(now - window_ms)
    .fetch_all(conn)
    .await
}

/// One probe result: its heartbeat, and for a current result the probe's state, the quorum and
/// the monitor-level beat when this probe leads or the status changes. `sent_at` is when the
/// probe sent it, on the probe's clock.
async fn probe_result(
    conn: &mut SqliteConnection,
    at: Moment,
    probe: ProbeId,
    result: ProbeResult,
    sent_at: i64,
    part: &mut Part,
    topics: &mut Vec<Topic>,
) -> anyhow::Result<()> {
    let monitor = result.monitor;
    let Some(target) = current(conn, monitor, result.revision).await? else {
        return Ok(());
    };
    let assigned: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM monitor_probes WHERE monitor_id = ? AND probe_id = ?")
            .bind(monitor.get())
            .bind(probe.get())
            .fetch_optional(&mut *conn)
            .await?;
    // A check that was under way when maintenance began counts for nothing.
    if assigned.is_none() || !target.runs_on_probes() || in_maintenance(conn, monitor).await? {
        return Ok(());
    }
    let status = status_from_wire(result.status);
    part.heartbeats.push(Heartbeat {
        monitor: storage::MonitorId(monitor.get()),
        probe: Some(storage::ProbeId(probe.get())),
        time_ms: result.time,
        status,
        latency_ms: result.latency_ms,
        message: result.message.clone(),
        retries: result.retries,
        http_status: result.http_status,
        cert_expiry_ms: result.cert_expiry,
    });
    if sent_at - result.time > target.interval * 1000 {
        return Ok(());
    }
    if let Some(chain) = &result.tls
        && let Some(url) = target.certificate_alerts()
    {
        for topic in
            notifications::check_certificates(conn, monitor, &url, chain, result.time, at.now)
                .await?
        {
            push_topic(topics, topic);
        }
    }

    let before: Option<i64> =
        sqlx::query_scalar("SELECT status FROM probe_state WHERE monitor_id = ? AND probe_id = ?")
            .bind(monitor.get())
            .bind(probe.get())
            .fetch_optional(&mut *conn)
            .await?;
    sqlx::query(
        "INSERT INTO probe_state
             (monitor_id, probe_id, status, message, latency_ms, retries, checked_at, received_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (monitor_id, probe_id) DO UPDATE SET status = excluded.status,
             message = excluded.message, latency_ms = excluded.latency_ms,
             retries = excluded.retries, checked_at = excluded.checked_at,
             received_at = excluded.received_at",
    )
    .bind(monitor.get())
    .bind(probe.get())
    .bind(status_code(status))
    .bind(&result.message)
    .bind(result.latency_ms)
    .bind(result.retries)
    .bind(result.time)
    .bind(at.now)
    .execute(&mut *conn)
    .await?;
    if before != Some(status_code(status)) {
        let payload = json!({ "monitor": monitor, "probe": probe, "status": result.status });
        let node = NodeId::new(target.node_id);
        for topic in
            super::record_evaluated(conn, node, "monitor.probe_changed", &payload, at.now).await?
        {
            push_topic(topics, topic);
        }
        for topic in pages::touch(conn, Change::Monitor(monitor), at.now).await? {
            push_topic(topics, topic);
        }
    }

    let live = live_probes(conn, monitor, target.live_window_ms(), at.now, at.started).await?;
    let statuses: Vec<Status> = live
        .iter()
        .filter_map(|live| status_from_code(live.status))
        .collect();
    let quorum = rules::quorum(from_text(&target.quorum)?, &statuses);
    let previous = previous(conn, monitor).await?;
    let leads = live.first().is_some_and(|lead| lead.probe() == probe);
    let changes = previous
        .as_ref()
        .is_none_or(|p| p.no_live_probe || status_from_code(p.status) != Some(quorum));
    if !leads && !changes {
        return Ok(());
    }
    // Message, latency and retries come from a probe that sees the monitor's status, this one
    // first.
    let code = status_code(quorum);
    let representative = live
        .iter()
        .filter(|live| live.status == code)
        .min_by_key(|live| live.probe() != probe);
    let (retried, latency_ms) = match representative {
        Some(live) => (
            Retried {
                status: quorum,
                retries: live.retries as u32,
                message: live.message.clone(),
            },
            live.latency_ms.map(|ms| ms as u32),
        ),
        None => (no_live_probe(), None),
    };
    beat(
        conn,
        &target,
        result.time,
        &retried,
        latency_ms,
        false,
        topics,
        at.now,
    )
    .await?;
    part.statuses.push(MonitorStatus {
        monitor: storage::MonitorId(monitor.get()),
        time_ms: result.time,
        status: quorum,
        latency_ms,
    });
    Ok(())
}

/// Kuma's beat for a monitor under maintenance (0008 item 9).
const UNDER_MAINTENANCE: &str = "Monitor under maintenance";

fn under_maintenance() -> Retried {
    Retried {
        status: Status::Maintenance,
        retries: 0,
        message: UNDER_MAINTENANCE.to_owned(),
    }
}

async fn in_maintenance(conn: &mut SqliteConnection, monitor: MonitorId) -> sqlx::Result<bool> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM maintenance_state WHERE monitor_id = ?")
            .bind(monitor.get())
            .fetch_optional(conn)
            .await?;
    Ok(found.is_some())
}

fn no_live_probe() -> Retried {
    Retried {
        status: Status::Pending,
        retries: 0,
        message: "No live probe".to_owned(),
    }
}

/// Takes remote probes without contact for [`OFFLINE_AFTER_MS`] offline and sets monitors
/// without a live probe pending.
async fn sweep(
    conn: &mut SqliteConnection,
    at: Moment,
    topics: &mut Vec<Topic>,
) -> anyhow::Result<()> {
    let quiet: Vec<(i64, i64, String, i64)> = sqlx::query_as(
        "SELECT id, node_id, name, last_seen_at FROM probes
         WHERE builtin = 0 AND revoked_at IS NULL AND offline_since IS NULL
           AND last_seen_at IS NOT NULL AND max(last_seen_at, ?) < ?",
    )
    .bind(at.started)
    .bind(at.now - OFFLINE_AFTER_MS)
    .fetch_all(&mut *conn)
    .await?;
    for (probe, node, name, last_seen) in quiet {
        sqlx::query("UPDATE probes SET offline_since = ? WHERE id = ?")
            .bind(at.now)
            .bind(probe)
            .execute(&mut *conn)
            .await?;
        sqlx::query("INSERT INTO probe_transitions (probe_id, time, online) VALUES (?, ?, 0)")
            .bind(probe)
            .bind(at.now)
            .execute(&mut *conn)
            .await?;
        let topic = Topic::probes(NodeId::new(node));
        let payload = json!({ "probe": ProbeId::new(probe) });
        events::record(conn, &topic, "probe.offline", &payload, at.now).await?;
        push_topic(topics, topic);
        let probe = notifications::ProbePayload {
            probe_name: name,
            last_seen,
        };
        for topic in
            notifications::queue_probe_offline(conn, NodeId::new(node), probe, at.now).await?
        {
            push_topic(topics, topic);
        }
    }

    let silent: Vec<Target> = sqlx::query_as(concat!(
        select_target!(),
        " WHERE active = 1 AND type NOT IN ('push', 'group', 'manual', 'unsupported')
            AND max(updated_at, ?1) < ?2 - 3000 * max(interval, retry_interval)
            AND NOT EXISTS (SELECT 1 FROM monitor_state s
                            WHERE s.monitor_id = monitors.id AND s.no_live_probe = 1)
            AND NOT EXISTS (SELECT 1 FROM maintenance_state ms WHERE ms.monitor_id = monitors.id)
            AND NOT EXISTS (
                SELECT 1 FROM probe_state ps
                JOIN monitor_probes mp
                  ON mp.monitor_id = ps.monitor_id AND mp.probe_id = ps.probe_id
                JOIN probes p ON p.id = ps.probe_id
                WHERE ps.monitor_id = monitors.id
                  AND p.revoked_at IS NULL AND p.offline_since IS NULL
                  AND max(ps.received_at, ?1) >= ?2 - 3000 * max(interval, retry_interval))"
    ))
    .bind(at.started)
    .bind(at.now)
    .fetch_all(&mut *conn)
    .await?;
    for target in silent {
        beat(
            conn,
            &target,
            at.now,
            &no_live_probe(),
            None,
            true,
            topics,
            at.now,
        )
        .await?;
    }
    Ok(())
}

async fn round(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
    revision: i64,
    at: i64,
    out: &mut Outcome,
) -> anyhow::Result<Next> {
    let Some(target) = current(conn, monitor, revision).await? else {
        return Ok(Next::Interval);
    };
    if in_maintenance(conn, monitor).await? {
        server_beat(conn, &target, at, &under_maintenance(), None, out).await?;
        return Ok(Next::Interval);
    }
    let previous = previous(conn, monitor).await?;
    let verdict = match target.kind.as_str() {
        "push" => {
            let interval_ms = target.interval * 1000;
            let healthy = if target.upside_down {
                Status::Down
            } else {
                Status::Up
            };
            if let Some(last) = &previous
                && status_from_code(last.status) == Some(healthy)
                && at - last.beat_at <= interval_ms + PUSH_GRACE_MS
            {
                // Waits for the deadline without recording anything, as Kuma does.
                return Ok(Next::At(
                    (last.beat_at + interval_ms + PUSH_GRACE_MS).max(at + 1),
                ));
            }
            Verdict::Fail("No heartbeat in the time window".to_owned())
        }
        "group" => rules::group(&members(conn, monitor).await?),
        "manual" => match target.manual_status.and_then(status_from_code) {
            Some(Status::Up) => Verdict::Pass("Up".to_owned()),
            Some(Status::Down) => Verdict::Set(Status::Down, "Down".to_owned()),
            Some(_) => Verdict::Set(Status::Pending, "Pending".to_owned()),
            None => Verdict::Set(
                Status::Pending,
                "Manual monitoring - No status set".to_owned(),
            ),
        },
        _ => return Ok(Next::Interval),
    };
    let verdict = if target.upside_down {
        verdict.upside_down()
    } else {
        verdict
    };
    let retries = previous.as_ref().map_or(0, |p| p.retries as u32);
    let retried = rules::retry(verdict, retries, target.max_retries());
    if target.kind == "manual" && target.manual_status.is_none() {
        // Pending without a status says nothing about the service, so it stays out of the
        // series: counted as down (0008 item 1), it would drag down the uptime of a monitor
        // whose status is set a moment after it was created.
        beat(
            conn,
            &target,
            at,
            &retried,
            None,
            false,
            &mut out.topics,
            at,
        )
        .await?;
    } else {
        server_beat(conn, &target, at, &retried, None, out).await?;
    }
    Ok(match retried.status {
        Status::Pending => Next::Retry,
        _ => Next::Interval,
    })
}

async fn members(conn: &mut SqliteConnection, group: MonitorId) -> sqlx::Result<Vec<Member>> {
    let rows: Vec<(String, bool, Option<i64>)> = sqlx::query_as(
        "SELECT m.name, m.active, s.status FROM monitors m
         LEFT JOIN monitor_state s ON s.monitor_id = m.id
         WHERE m.group_id = ? ORDER BY m.id",
    )
    .bind(group.get())
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, active, status)| Member {
            name,
            active,
            status: status.and_then(status_from_code),
        })
        .collect())
}

/// Records a push. False when no active monitor has the token.
async fn push_beat(
    conn: &mut SqliteConnection,
    token_digest: &str,
    push: Push,
    at: i64,
    out: &mut Outcome,
) -> anyhow::Result<bool> {
    let target: Option<Target> = sqlx::query_as(concat!(
        select_target!(),
        " WHERE push_token_hash = ? AND active = 1"
    ))
    .bind(token_digest)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(target) = target else {
        return Ok(false);
    };
    if in_maintenance(conn, target.monitor()).await? {
        server_beat(
            conn,
            &target,
            at,
            &under_maintenance(),
            push.latency_ms,
            out,
        )
        .await?;
        return Ok(true);
    }
    let previous = previous(conn, target.monitor()).await?;
    // Upside down applies to the pushed status before retries.
    let verdict = if push.up != target.upside_down {
        Verdict::Pass(push.message)
    } else {
        Verdict::Fail(push.message)
    };
    let retries = previous.as_ref().map_or(0, |p| p.retries as u32);
    let retried = rules::retry(verdict, retries, target.max_retries());
    server_beat(conn, &target, at, &retried, push.latency_ms, out).await?;
    Ok(true)
}

/// A beat of a monitor the server evaluates itself: its raw heartbeat is the monitor-level
/// series.
async fn server_beat(
    conn: &mut SqliteConnection,
    target: &Target,
    at: i64,
    retried: &Retried,
    latency_ms: Option<u32>,
    out: &mut Outcome,
) -> anyhow::Result<()> {
    out.plain.heartbeats.push(Heartbeat {
        monitor: storage::MonitorId(target.id),
        probe: None,
        time_ms: at,
        status: retried.status,
        latency_ms,
        message: retried.message.clone(),
        retries: retried.retries,
        http_status: None,
        cert_expiry_ms: None,
    });
    beat(
        conn,
        target,
        at,
        retried,
        latency_ms,
        false,
        &mut out.topics,
        at,
    )
    .await
}

/// Applies a monitor-level beat: flags, the resend counter, state, journal and topic event.
#[allow(clippy::too_many_arguments)]
async fn beat(
    conn: &mut SqliteConnection,
    target: &Target,
    at: i64,
    retried: &Retried,
    latency_ms: Option<u32>,
    no_live_probe: bool,
    topics: &mut Vec<Topic>,
    now: i64,
) -> anyhow::Result<()> {
    let monitor = target.monitor();
    let previous = previous(conn, monitor).await?;
    let before = previous.as_ref().and_then(|p| status_from_code(p.status));
    let flags = rules::flags(before, retried.status);
    let mut down_count = previous.as_ref().map_or(0, |p| p.down_count);
    let mut resend = false;
    if flags.important {
        down_count = 0;
    } else if retried.status == Status::Down && target.resend_interval > 0 {
        down_count += 1;
        if down_count >= target.resend_interval {
            resend = true;
            down_count = 0;
        }
    }
    let changed_at = match &previous {
        Some(p) if before == Some(retried.status) => p.changed_at,
        _ => at,
    };
    sqlx::query(
        "INSERT INTO monitor_state (monitor_id, status, message, latency_ms, retries, down_count,
                                    beat_at, changed_at, no_live_probe)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (monitor_id) DO UPDATE SET status = excluded.status,
             message = excluded.message, latency_ms = excluded.latency_ms,
             retries = excluded.retries, down_count = excluded.down_count,
             beat_at = excluded.beat_at, changed_at = excluded.changed_at,
             no_live_probe = excluded.no_live_probe",
    )
    .bind(monitor.get())
    .bind(status_code(retried.status))
    .bind(&retried.message)
    .bind(latency_ms)
    .bind(retried.retries)
    .bind(down_count)
    .bind(at)
    .bind(changed_at)
    .bind(no_live_probe)
    .execute(&mut *conn)
    .await?;
    if flags.important || resend {
        let transition: i64 = sqlx::query_scalar(
            "INSERT INTO monitor_transitions
                 (monitor_id, time, previous, status, message, important, notify, resend)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(monitor.get())
        .bind(at)
        .bind(before.map(status_code))
        .bind(status_code(retried.status))
        .bind(&retried.message)
        .bind(flags.important)
        .bind(flags.notify || resend)
        .bind(resend)
        .fetch_one(&mut *conn)
        .await?;
        if flags.important {
            for topic in
                incidents::on_transition(conn, monitor, retried.status, &retried.message, at)
                    .await?
            {
                push_topic(topics, topic);
            }
        }
        if flags.notify || resend {
            let queued = notifications::queue_beat(
                conn,
                notifications::Beat {
                    monitor,
                    transition,
                    status: retried.status,
                    time: at,
                    message: &retried.message,
                    latency_ms,
                    important: flags.important,
                    duration_s: previous.as_ref().map_or(0, |p| (at - p.beat_at) / 1000),
                    retries: retried.retries,
                },
                now,
            )
            .await?;
            for topic in queued {
                push_topic(topics, topic);
            }
        }
    }
    if before != Some(retried.status) {
        let payload = json!({ "monitor": monitor, "status": wire_status(retried.status) });
        let node = NodeId::new(target.node_id);
        for topic in
            super::record_evaluated(conn, node, "monitor.status_changed", &payload, at).await?
        {
            push_topic(topics, topic);
        }
        for topic in pages::touch(conn, Change::Monitor(monitor), at).await? {
            push_topic(topics, topic);
        }
    }
    Ok(())
}

fn push_topic(topics: &mut Vec<Topic>, topic: Topic) {
    if !topics.contains(&topic) {
        topics.push(topic);
    }
}
