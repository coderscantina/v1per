//! Heartbeat storage: a durable SQLite result inbox, stable batch transfer into DuckDB,
//! a watermark, bounded typed reads and retention.
//!
//! [`Storage::admit`] returns only after a durable SQLite commit. A background transfer
//! applies stable batches to DuckDB exactly once through an applied-batch ledger, updating
//! the minute, hour and day rollups in the same transaction, and then retires them from the
//! inbox. [`Storage::watermark`] follows the highest contiguous applied position. Reads run
//! on bounded worker threads inside one DuckDB transaction each. Ledger, batch membership
//! and retirement stay inside.

mod failpoint;
mod heartbeats;
mod inbox;
mod queries;
mod reader;
mod record;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::JoinHandle;

pub use failpoint::BOUNDARIES as FAILPOINTS;
#[cfg(feature = "failpoints")]
pub use failpoint::fail_disk;
pub use heartbeats::{HEARTBEAT_SCHEMA, KumaStat};
pub use queries::{
    Counts, DayBar, HeartbeatCursor, HeartbeatPage, ImportedRows, Latency, LatencyPoint,
    Resolution, Series, UptimeWindow,
};
pub use reader::{Read, ReadError, Snapshot};
pub use record::{BatchId, Heartbeat, INBOX_FORMAT, MonitorId, MonitorStatus, ProbeId, Status};

use inbox::{Encoded, Inbox};
use reader::Workers;

/// Files inside the data directory.
pub const INBOX_FILE: &str = "inbox.sqlite";
pub const HEARTBEATS_FILE: &str = "heartbeats.duckdb";
pub const HEARTBEATS_WAL_FILE: &str = "heartbeats.duckdb.wal";
pub const SPILL_DIR: &str = "spill";

/// Installation-wide resource budgets. Milestone 02 measures and records them.
#[derive(Debug, Clone)]
pub struct Budgets {
    /// Encoded bytes allowed in the durable inbox backlog. Admission is refused above it.
    pub inbox_bytes: u64,
    /// Encoded bytes waiting for the next group commit.
    pub admission_queue_bytes: u64,
    /// Free bytes the data volume keeps for controlled shutdown and diagnostics.
    pub disk_reserve_bytes: u64,
    /// Largest stable batch, in inbox rows (admissions) and encoded bytes.
    pub batch_rows: usize,
    pub batch_bytes: u64,
    /// Wait after a commit wakeup before assigning, so batches amortize DuckDB commits.
    pub transfer_delay: Duration,
    /// DuckDB worker threads shared by the writer and every reader.
    pub engine_threads: u32,
    /// DuckDB buffer manager limit; spill goes to `<data>/spill` beyond it.
    pub engine_memory_bytes: u64,
    /// Concurrent reads and how many may wait for a worker.
    pub readers: usize,
    pub read_queue: usize,
    /// Cancellation limit for one read, queue wait included.
    pub query_timeout: Duration,
}

impl Default for Budgets {
    /// Starting values for the 2 vCPU, 4 GiB reference host, copied from the template
    /// until the milestone 02 benchmark replaces them.
    fn default() -> Self {
        const MIB: u64 = 1024 * 1024;
        Self {
            inbox_bytes: 1024 * MIB,
            admission_queue_bytes: 64 * MIB,
            disk_reserve_bytes: 4 * 1024 * MIB,
            batch_rows: 20_000,
            batch_bytes: 32 * MIB,
            // Status changes are decided before transfer, so only charts and uptime wait for
            // it; a longer delay means fewer ledger rows and rollup merges.
            transfer_delay: Duration::from_secs(1),
            engine_threads: 2,
            engine_memory_bytes: 1024 * MIB,
            readers: 2,
            read_queue: 8,
            query_timeout: Duration::from_secs(30),
        }
    }
}

/// How long DuckDB keeps data. The server runs [`Storage::expire`] periodically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    pub raw: Duration,
    /// `None` keeps minute rollups forever. Open decision in 0002; hour and day rollups are
    /// always kept.
    pub minute_rollups: Option<Duration>,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            raw: Duration::from_secs(90 * 86_400),
            minute_rollups: None,
        }
    }
}

/// What one [`Storage::expire`] run deleted, or [`Storage::expirable`] would delete.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Expired {
    pub heartbeats: u64,
    pub minute_rollups: u64,
}

/// Highest contiguous admitted position applied to DuckDB: the last applied transfer batch
/// and the last inbox sequence it contains. Batches apply in ID order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Watermark {
    pub batch: i64,
    pub sequence: i64,
}

/// A durably committed admission. Its heartbeats are visible to reads once the watermark
/// sequence reaches `sequence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted {
    pub sequence: i64,
    /// The agent batch ID was admitted before with the same content; nothing new was stored.
    pub repeated: bool,
}

/// Whole-admission refusal. Nothing from the admission is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Inbox or admission queue budget is full; the agent retries later.
    #[error("admission budget full")]
    Busy,
    /// Durable commit failed, disk headroom is below the reserve, or storage is shutting down.
    #[error("storage unavailable")]
    Unavailable,
    /// The agent batch ID was admitted before with different content.
    #[error("batch ID reused with different content")]
    BatchConflict,
}

/// Where a pair of engine files stands (see [`inspect`]). A backup records it, and a restore
/// checks that transfer applies exactly the pending records on top of the ledger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Positions {
    /// The last batch in the ledger and the inbox sequence it ends with.
    pub applied_batch: i64,
    pub applied_sequence: i64,
    /// Inbox rows of every batch the ledger holds.
    pub applied_records: i64,
    /// Inbox rows not applied yet. Transfer applies them after a restore.
    pub pending_records: i64,
}

/// Schema versions as stored; `None` for a file that does not exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schemas {
    pub inbox: Option<i64>,
    pub heartbeats: Option<i64>,
}

/// Space on the volume of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disk {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// Inbox depth, watermark and transfer health, for diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct Health {
    pub inbox_rows: u64,
    /// Heartbeats and monitor-level entries waiting.
    pub inbox_entries: u64,
    pub inbox_bytes: u64,
    pub watermark: Watermark,
    /// Failed transfer attempts since start.
    pub transfer_failures: u64,
    /// Since when every transfer attempt failed, Unix milliseconds; `None` while it works.
    pub transfer_failing_since_ms: Option<i64>,
    /// Transfer batches applied since start, the DuckDB time they took, and the part of it
    /// spent merging rollups. Differences between two readings give the cost per batch.
    pub applied_batches: u64,
    pub apply_time: Duration,
    pub rollup_time: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("inbox: {0}")]
    Sqlite(#[from] sqlx::Error),
    #[error("inbox migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("heartbeat engine: {0}")]
    Engine(#[from] duckdb::Error),
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("refusing incompatible replay: {0}")]
    Incompatible(String),
}

struct Request {
    encoded: Encoded,
    reply: oneshot::Sender<Result<Admitted, Refusal>>,
}

/// Transfer attempts, for health: a failing transfer is not the same as a slow one.
#[derive(Default)]
struct TransferHealth {
    failures: AtomicU64,
    /// Zero while transfer works.
    failing_since_ms: AtomicI64,
    applied: AtomicU64,
    apply_nanos: AtomicU64,
    rollup_nanos: AtomicU64,
}

type Writer = Arc<Mutex<duckdb::Connection>>;

pub struct Storage {
    data_dir: PathBuf,
    budgets: Budgets,
    inbox: Inbox,
    queued: Arc<AtomicU64>,
    admissions: Mutex<Option<mpsc::UnboundedSender<Request>>>,
    writer: Writer,
    readers: Workers,
    watermark: watch::Receiver<Watermark>,
    transfer: Arc<TransferHealth>,
    stop: watch::Sender<bool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Storage {
    /// Opens or creates `<data_dir>/inbox.sqlite` and `<data_dir>/heartbeats.duckdb`,
    /// publishes the committed watermark from the ledger, and resumes any assigned batch.
    pub async fn open(data_dir: &Path, budgets: Budgets) -> Result<Self, Error> {
        let spill = data_dir.join(SPILL_DIR);
        std::fs::create_dir_all(&spill)?;
        let inbox = Inbox::open(&data_dir.join(INBOX_FILE)).await?;
        let conn = heartbeats::open(&data_dir.join(HEARTBEATS_FILE), &spill, &budgets)?;
        let readers = Workers::start(
            "heartbeat-read",
            &conn,
            budgets.readers,
            budgets.read_queue,
            budgets.query_timeout,
        )?;
        // Restart reconciliation: the ledger is the source of truth for visibility.
        let (watermark_tx, watermark) = watch::channel(heartbeats::watermark(&conn)?);
        let writer = Arc::new(Mutex::new(conn));

        let (admissions, requests) = mpsc::unbounded_channel();
        let queued = Arc::new(AtomicU64::new(0));
        let wake = Arc::new(Notify::new());
        let transfer = Arc::new(TransferHealth::default());
        let (stop, stopped) = watch::channel(false);
        let tasks = vec![
            tokio::spawn(admit_loop(
                inbox.clone(),
                requests,
                queued.clone(),
                wake.clone(),
                data_dir.to_path_buf(),
                budgets.disk_reserve_bytes,
            )),
            tokio::spawn(transfer_loop(
                Transfer {
                    inbox: inbox.clone(),
                    writer: writer.clone(),
                    health: transfer.clone(),
                    watermark: watermark_tx,
                    budgets: budgets.clone(),
                },
                wake,
                stopped,
            )),
        ];
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            budgets,
            inbox,
            queued,
            admissions: Mutex::new(Some(admissions)),
            writer,
            readers,
            watermark,
            transfer,
            stop,
            tasks: Mutex::new(tasks),
        })
    }

    /// Admits heartbeats in one durable commit, possibly grouped with other admissions.
    /// Returns only after SQLite reports the commit. With an agent `batch` ID, a repeat with
    /// the same heartbeats returns the original sequence and one with others is refused.
    /// A heartbeat counts in its probe's rollup series; one with probe `None` (push, group,
    /// manual) counts in the monitor-level series.
    pub async fn admit(
        &self,
        batch: Option<BatchId>,
        heartbeats: &[Heartbeat],
    ) -> Result<Admitted, Refusal> {
        self.admit_evaluated(batch, heartbeats, &[]).await
    }

    /// Records the evaluator's monitor-level statuses through the same durable inbox and
    /// ledger as heartbeats. They update the monitor-level rollups only, with no raw row.
    pub async fn record_statuses(&self, statuses: &[MonitorStatus]) -> Result<Admitted, Refusal> {
        self.admit_evaluated(None, &[], statuses).await
    }

    /// [`Storage::admit`] and [`Storage::record_statuses`] in one inbox row, so heartbeats and
    /// the monitor-level statuses evaluated from them commit together or not at all. With an
    /// agent `batch` ID the repeat check compares the heartbeats only: the statuses are the
    /// server's own derivation and need not come out the same for a resent batch.
    pub async fn admit_evaluated(
        &self,
        batch: Option<BatchId>,
        heartbeats: &[Heartbeat],
        statuses: &[MonitorStatus],
    ) -> Result<Admitted, Refusal> {
        let payload = record::encode(heartbeats, statuses);
        let batch = batch.map(|batch| {
            let fingerprint = if statuses.is_empty() {
                blake3::hash(&payload)
            } else {
                blake3::hash(&record::encode(heartbeats, &[]))
            };
            (batch, fingerprint)
        });
        self.submit(batch, payload, heartbeats.len() + statuses.len())
            .await
    }

    /// The inbox sequence an agent batch ID was admitted at, while its ID is kept (see
    /// [`Storage::prune_agent_batches`]).
    pub async fn admitted(&self, batch: &BatchId) -> Result<Option<i64>, Error> {
        Ok(self.inbox.batch_sequence(batch).await?)
    }

    async fn submit(
        &self,
        batch: Option<(BatchId, blake3::Hash)>,
        payload: Vec<u8>,
        entries: usize,
    ) -> Result<Admitted, Refusal> {
        let bytes = payload.len() as u64;
        let queued = self.queued.fetch_add(bytes, Ordering::SeqCst) + bytes;
        let backlog = self.inbox.backlog.bytes.load(Ordering::SeqCst);
        if queued > self.budgets.admission_queue_bytes
            || backlog + queued > self.budgets.inbox_bytes
        {
            self.queued.fetch_sub(bytes, Ordering::SeqCst);
            return Err(Refusal::Busy);
        }
        let (reply, outcome) = oneshot::channel();
        let request = Request {
            encoded: Encoded {
                batch,
                entries: entries as u64,
                payload,
            },
            reply,
        };
        let sender = self.admissions.lock().expect("admissions").clone();
        if !sender.is_some_and(|sender| sender.send(request).is_ok()) {
            self.queued.fetch_sub(bytes, Ordering::SeqCst);
            return Err(Refusal::Unavailable);
        }
        outcome.await.unwrap_or(Err(Refusal::Unavailable))
    }

    pub fn budgets(&self) -> &Budgets {
        &self.budgets
    }

    /// Follows the committed watermark.
    pub fn watermark(&self) -> watch::Receiver<Watermark> {
        self.watermark.clone()
    }

    pub fn health(&self) -> Health {
        let since = self.transfer.failing_since_ms.load(Ordering::Relaxed);
        Health {
            inbox_rows: self.inbox.backlog.rows.load(Ordering::Relaxed),
            inbox_entries: self.inbox.backlog.entries.load(Ordering::Relaxed),
            inbox_bytes: self.inbox.backlog.bytes.load(Ordering::Relaxed),
            watermark: *self.watermark.borrow(),
            transfer_failures: self.transfer.failures.load(Ordering::Relaxed),
            transfer_failing_since_ms: (since != 0).then_some(since),
            applied_batches: self.transfer.applied.load(Ordering::Relaxed),
            apply_time: Duration::from_nanos(self.transfer.apply_nanos.load(Ordering::Relaxed)),
            rollup_time: Duration::from_nanos(self.transfer.rollup_nanos.load(Ordering::Relaxed)),
        }
    }

    /// Age of the oldest admission still waiting in the inbox: how far transfer lags behind.
    /// Zero when the inbox is empty.
    pub async fn inbox_age(&self) -> Result<Duration, Error> {
        let oldest = self.inbox.oldest_received_ms().await?;
        Ok(Duration::from_millis(
            oldest.map_or(0, |at| now_ms().saturating_sub(at).max(0) as u64),
        ))
    }

    /// Waits until every admission so far is applied and retired.
    pub async fn drain(&self) {
        let mut watermark = self.watermark();
        while self.inbox.backlog.rows.load(Ordering::SeqCst) > 0 {
            let _ = tokio::time::timeout(Duration::from_millis(50), watermark.changed()).await;
        }
    }

    /// Runs `f` on a bounded read worker inside one DuckDB transaction. For diagnostics and
    /// benchmarks; product reads use the typed methods below.
    pub async fn read<T, F>(&self, f: F) -> Result<Read<T>, ReadError>
    where
        T: Send + 'static,
        F: FnOnce(&Snapshot) -> duckdb::Result<T> + Send + 'static,
    {
        self.readers.read(f).await
    }

    /// Up to `limit` raw heartbeats of `monitor`, newest first, from `probe` only when given,
    /// after `after` (the `next` of the page before).
    pub async fn heartbeats(
        &self,
        monitor: MonitorId,
        probe: Option<ProbeId>,
        limit: u32,
        after: Option<HeartbeatCursor>,
    ) -> Result<Read<HeartbeatPage>, ReadError> {
        self.read(move |s| queries::heartbeats(s.connection(), monitor, probe, limit, after))
            .await
    }

    /// The newest heartbeat of each monitor at or after `since_ms`.
    pub async fn latest(
        &self,
        monitors: &[MonitorId],
        since_ms: i64,
    ) -> Result<Read<Vec<Heartbeat>>, ReadError> {
        let monitors = monitors.to_vec();
        self.read(move |s| queries::latest(s.connection(), &monitors, since_ms))
            .await
    }

    /// Uptime counts per monitor of `series` over `window`, ending with the bucket that
    /// holds `now_ms`.
    pub async fn uptime(
        &self,
        monitors: &[MonitorId],
        series: Series,
        window: UptimeWindow,
        now_ms: i64,
    ) -> Result<Read<BTreeMap<MonitorId, Counts>>, ReadError> {
        let monitors = monitors.to_vec();
        self.read(move |s| queries::uptime(s.connection(), &monitors, series, window, now_ms))
            .await
    }

    /// Daily bars of `series` for the `days` local days ending with `through` in `timezone`,
    /// from hour rollups. Zones with a non-hour offset round the day boundary to a UTC hour.
    /// Imported days without hourly data fall back to Kuma's UTC day.
    pub async fn day_bars(
        &self,
        monitors: &[MonitorId],
        series: Series,
        timezone: Tz,
        through: NaiveDate,
        days: u32,
    ) -> Result<Read<BTreeMap<MonitorId, Vec<DayBar>>>, ReadError> {
        let monitors = monitors.to_vec();
        self.read(move |s| {
            queries::day_bars(s.connection(), &monitors, series, timezone, through, days)
        })
        .await
    }

    /// Latency of `series` per `resolution` bucket in `[from_ms, to_ms)`.
    pub async fn latency(
        &self,
        monitor: MonitorId,
        series: Series,
        resolution: Resolution,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Read<Vec<LatencyPoint>>, ReadError> {
        self.read(move |s| {
            queries::latency(s.connection(), monitor, series, resolution, from_ms, to_ms)
        })
        .await
    }

    /// Writes imported Kuma `stat_*` rows of one monitor and resolution as they are. Rows in
    /// the span the chunk covers are replaced, so retrying a chunk counts it once.
    pub async fn import_kuma_stats(
        &self,
        monitor: MonitorId,
        resolution: Resolution,
        rows: Vec<KumaStat>,
    ) -> Result<(), Error> {
        self.with_writer(move |conn| {
            Ok(heartbeats::import_kuma_stats(
                conn, monitor, resolution, &rows,
            )?)
        })
        .await
    }

    /// What the Kuma import wrote for `monitors`, for its reconciliation.
    pub async fn imported_rows(
        &self,
        monitors: &[MonitorId],
    ) -> Result<Read<ImportedRows>, ReadError> {
        let monitors = monitors.to_vec();
        self.read(move |s| queries::imported_rows(s.connection(), &monitors))
            .await
    }

    /// Deletes raw heartbeats, and minute rollups when their retention is set, older than
    /// `retention` before `now_ms`. Works a day at a time, releasing the writer in between
    /// so transfer keeps going; admission never waits for it.
    pub async fn expire(&self, retention: Retention, now_ms: i64) -> Result<Expired, Error> {
        let mut expired = Expired::default();
        let raw_cutoff = now_ms - retention.raw.as_millis() as i64;
        expired.heartbeats = self.expire_table("heartbeat", "time", raw_cutoff).await?;
        if let Some(keep) = retention.minute_rollups {
            let cutoff = now_ms - keep.as_millis() as i64;
            expired.minute_rollups = self.expire_table("rollup_minute", "bucket", cutoff).await?;
        }
        Ok(expired)
    }

    /// What [`Storage::expire`] would delete now, deleting nothing.
    pub async fn expirable(&self, retention: Retention, now_ms: i64) -> Result<Expired, Error> {
        let raw_cutoff = now_ms - retention.raw.as_millis() as i64;
        let minute_cutoff = retention
            .minute_rollups
            .map(|keep| now_ms - keep.as_millis() as i64);
        self.with_writer(move |conn| {
            Ok(Expired {
                heartbeats: heartbeats::count_before(conn, "heartbeat", "time", raw_cutoff)?,
                minute_rollups: match minute_cutoff {
                    Some(cutoff) => {
                        heartbeats::count_before(conn, "rollup_minute", "bucket", cutoff)?
                    }
                    None => 0,
                },
            })
        })
        .await
    }

    async fn expire_table(
        &self,
        table: &'static str,
        column: &'static str,
        cutoff_ms: i64,
    ) -> Result<u64, Error> {
        let mut total = 0;
        loop {
            failpoint::disk("retention.step")?;
            let deleted = self
                .with_writer(move |conn| {
                    Ok(heartbeats::expire_step(conn, table, column, cutoff_ms)?)
                })
                .await?;
            if deleted == 0 {
                return Ok(total);
            }
            total += deleted;
        }
    }

    /// Forgets agent batch IDs first received more than `keep` before `now_ms`. An agent
    /// retrying a batch after that is admitted again.
    pub async fn prune_agent_batches(&self, keep: Duration, now_ms: i64) -> Result<u64, Error> {
        Ok(self
            .inbox
            .prune_agent_batches(now_ms - keep.as_millis() as i64)
            .await?)
    }

    /// Agent batch IDs [`Storage::prune_agent_batches`] would forget now.
    pub async fn prunable_agent_batches(&self, keep: Duration, now_ms: i64) -> Result<u64, Error> {
        Ok(self
            .inbox
            .count_agent_batches(now_ms - keep.as_millis() as i64)
            .await?)
    }

    /// Copies the heartbeat file and a snapshot of the inbox into `dir` while the DuckDB
    /// writer waits, so the two match: transfer, retention and imports pause, admission and
    /// reads go on and the inbox absorbs new results. Returns how long the writer waited.
    pub async fn snapshot(&self, dir: &Path) -> Result<Duration, Error> {
        let writer = self.writer.clone();
        let inbox = self.inbox.clone();
        let source = self.data_dir.clone();
        let dir = dir.to_path_buf();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let conn = writer.lock().expect("heartbeat writer");
            let paused = Instant::now();
            // A checkpoint folds the WAL into the file. A reader inside a transaction can keep
            // it from finishing; then the WAL is copied too. Neither file changes while the
            // writer waits, so the copies match either way.
            let _ = conn.execute_batch("CHECKPOINT");
            failpoint::disk("backup.copy")?;
            copy_heartbeats(&source, &dir)?;
            runtime.block_on(inbox.vacuum_into(&dir.join(INBOX_FILE)))?;
            drop(conn);
            Ok(paused.elapsed())
        })
        .await
        .expect("snapshot task")
    }

    /// Stops in order: admission first (queued admissions still commit), then transfer after
    /// the batch in hand, then the read workers, then a DuckDB checkpoint. Admission answers
    /// unavailable from the first step on. The next open reconciles from the ledger.
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.admissions.lock().expect("admissions").take();
        let _ = self.stop.send(true);
        let tasks = std::mem::take(&mut *self.tasks.lock().expect("tasks"));
        for task in tasks {
            let _ = task.await;
        }
        self.readers.close();
        self.inbox.close().await;
        self.with_writer(|conn| Ok(conn.execute_batch("CHECKPOINT")?))
            .await
    }

    /// [`Storage::shutdown`] for an owner.
    pub async fn close(self) -> Result<(), Error> {
        self.shutdown().await
    }

    /// Runs `f` on the single DuckDB writer, off the async executor.
    async fn with_writer<T, F>(&self, f: F) -> Result<T, Error>
    where
        T: Send + 'static,
        F: FnOnce(&mut duckdb::Connection) -> Result<T, Error> + Send + 'static,
    {
        let writer = self.writer.clone();
        tokio::task::spawn_blocking(move || f(&mut writer.lock().expect("heartbeat writer")))
            .await
            .expect("writer task")
    }
}

/// Group commit: takes every admission queued at the moment the writer is free and commits
/// them together. No added delay; queueing alone forms the group.
async fn admit_loop(
    inbox: Inbox,
    mut requests: mpsc::UnboundedReceiver<Request>,
    queued: Arc<AtomicU64>,
    wake: Arc<Notify>,
    data_dir: PathBuf,
    reserve: u64,
) {
    const GROUP_BYTES: u64 = 16 * 1024 * 1024;
    while let Some(first) = requests.recv().await {
        let mut bytes = first.encoded.payload.len() as u64;
        let mut group = vec![first];
        while bytes < GROUP_BYTES {
            match requests.try_recv() {
                Ok(next) => {
                    bytes += next.encoded.payload.len() as u64;
                    group.push(next);
                }
                Err(_) => break,
            }
        }
        let result = if free_bytes(&data_dir).is_ok_and(|free| free >= reserve) {
            let parts: Vec<&Encoded> = group.iter().map(|r| &r.encoded).collect();
            inbox.commit(&parts).await.map_err(|_| Refusal::Unavailable)
        } else {
            Err(Refusal::Unavailable)
        };
        queued.fetch_sub(bytes, Ordering::SeqCst);
        match result {
            Ok(outcomes) => {
                failpoint::hit("admission.after_commit");
                wake.notify_one();
                for (request, outcome) in group.into_iter().zip(outcomes) {
                    let _ = request.reply.send(outcome);
                }
            }
            Err(refusal) => {
                for request in group {
                    let _ = request.reply.send(Err(refusal));
                }
            }
        }
    }
}

/// What the single DuckDB writer needs to move batches.
struct Transfer {
    inbox: Inbox,
    writer: Writer,
    health: Arc<TransferHealth>,
    watermark: watch::Sender<Watermark>,
    budgets: Budgets,
}

/// The single DuckDB writer: assign or resume a stable batch, apply it once, publish the
/// watermark, retire the inbox rows.
async fn transfer_loop(transfer: Transfer, wake: Arc<Notify>, mut stop: watch::Receiver<bool>) {
    while !*stop.borrow() {
        match transfer_one(&transfer).await {
            Ok(moved) => {
                transfer.health.failing_since_ms.store(0, Ordering::Relaxed);
                if moved {
                    continue;
                }
                tokio::select! {
                    _ = async {
                        wake.notified().await;
                        tokio::time::sleep(transfer.budgets.transfer_delay).await;
                    } => {}
                    _ = stop.changed() => {}
                }
            }
            Err(error) => {
                transfer.health.failures.fetch_add(1, Ordering::Relaxed);
                let _ = transfer.health.failing_since_ms.compare_exchange(
                    0,
                    now_ms(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                eprintln!("v1per-storage: transfer failed, retrying the same batch: {error}");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = stop.changed() => {}
                }
            }
        }
    }
}

async fn transfer_one(transfer: &Transfer) -> Result<bool, Error> {
    let Some(batch) = transfer
        .inbox
        .next_batch(transfer.budgets.batch_rows, transfer.budgets.batch_bytes)
        .await?
    else {
        return Ok(false);
    };
    let writer = transfer.writer.clone();
    let (batch, applied, took) = tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let applied = heartbeats::apply(&mut writer.lock().expect("heartbeat writer"), &batch);
        (batch, applied, started.elapsed())
    })
    .await
    .expect("apply task");
    let (applied, rollups) = applied?;
    let health = &transfer.health;
    health.applied.fetch_add(1, Ordering::Relaxed);
    health
        .apply_nanos
        .fetch_add(took.as_nanos() as u64, Ordering::Relaxed);
    health
        .rollup_nanos
        .fetch_add(rollups.as_nanos() as u64, Ordering::Relaxed);
    failpoint::hit("transfer.before_notify");
    transfer.watermark.send_if_modified(|current| {
        let newer = applied > *current;
        if newer {
            *current = applied;
        }
        newer
    });
    failpoint::hit("transfer.before_retire");
    transfer.inbox.retire(&batch).await?;
    Ok(true)
}

/// [`Storage::snapshot`] of a data directory no process holds, at whatever schema it has.
/// A file that does not exist yet is left out.
pub async fn snapshot_stopped(data_dir: &Path, dir: &Path) -> Result<(), Error> {
    if data_dir.join(HEARTBEATS_FILE).exists() {
        copy_heartbeats(data_dir, dir)?;
    }
    let inbox = data_dir.join(INBOX_FILE);
    if inbox.exists() {
        let pool = inbox::read_only(&inbox).await?;
        let copied = inbox::vacuum_into(&pool, &dir.join(INBOX_FILE)).await;
        pool.close().await;
        copied?;
    }
    Ok(())
}

/// The positions of the engine files in `dir`, which no process may hold. A copied DuckDB
/// WAL is folded into its file first, so the heartbeat file stands alone.
pub async fn inspect(dir: &Path) -> Result<Positions, Error> {
    let file = dir.join(HEARTBEATS_FILE);
    let (applied, applied_records) = if file.exists() {
        let wal = dir.join(HEARTBEATS_WAL_FILE);
        tokio::task::spawn_blocking(move || {
            let conn = heartbeats::open_file(&file)?;
            conn.execute_batch("CHECKPOINT")?;
            let ledger = heartbeats::ledger(&conn)?;
            drop(conn);
            match std::fs::remove_file(wal) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
            Ok::<_, Error>(ledger)
        })
        .await
        .expect("inspect task")?
    } else {
        (Watermark::default(), 0)
    };
    let inbox = dir.join(INBOX_FILE);
    let pending_records = if inbox.exists() {
        let pool = inbox::read_only(&inbox).await?;
        let pending = inbox::pending_records(&pool, applied.batch).await;
        pool.close().await;
        pending?
    } else {
        0
    };
    Ok(Positions {
        applied_batch: applied.batch,
        applied_sequence: applied.sequence,
        applied_records,
        pending_records,
    })
}

/// The inbox schema this release runs on; the heartbeat schema is [`HEARTBEAT_SCHEMA`].
pub fn inbox_schema() -> i64 {
    inbox::release_schema()
}

/// The schema versions stored in `dir`, which no process may hold.
pub async fn stored_schemas(dir: &Path) -> Result<Schemas, Error> {
    let inbox = dir.join(INBOX_FILE);
    let inbox = if inbox.exists() {
        let pool = inbox::read_only(&inbox).await?;
        let version = inbox::stored_schema(&pool).await;
        pool.close().await;
        version?
    } else {
        None
    };
    let file = dir.join(HEARTBEATS_FILE);
    let heartbeats = if file.exists() {
        tokio::task::spawn_blocking(move || {
            let config = duckdb::Config::default()
                .access_mode(duckdb::AccessMode::ReadOnly)?
                .threads(1)?
                .enable_autoload_extension(false)?;
            heartbeats::stored_schema(&duckdb::Connection::open_with_flags(&file, config)?)
        })
        .await
        .expect("schema task")?
    } else {
        None
    };
    Ok(Schemas { inbox, heartbeats })
}

/// The heartbeat file and, when one exists, its WAL.
fn copy_heartbeats(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from.join(HEARTBEATS_FILE), to.join(HEARTBEATS_FILE))?;
    match std::fs::copy(from.join(HEARTBEATS_WAL_FILE), to.join(HEARTBEATS_WAL_FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result.map(drop),
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_millis() as i64
}

fn free_bytes(path: &Path) -> std::io::Result<u64> {
    disk(path).map(|disk| disk.free_bytes)
}

/// Free space for unprivileged writers, and the volume's size.
pub fn disk(path: &Path) -> std::io::Result<Disk> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    // SAFETY: statvfs fills the zeroed struct for a valid NUL-terminated path.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let block = stat.f_frsize as u64;
    Ok(Disk {
        free_bytes: stat.f_bavail as u64 * block,
        total_bytes: stat.f_blocks as u64 * block,
    })
}
