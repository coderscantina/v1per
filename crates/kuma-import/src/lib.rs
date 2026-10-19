//! The source side of the Uptime Kuma 2.5.0 import (decision 0007).
//!
//! [`Source::open`] opens a Kuma SQLite file read-only and refuses anything but an unmodified
//! 2.5.0 schema. [`Source::plan`] maps every configuration row into an [`ImportPlan`] or the
//! inventory, [`Source::report`] summarises it for `--dry-run`, and [`Source::heartbeats`]
//! and [`Source::stats`] stream history in resumable chunks. Nothing here writes; the server
//! applies the plan, seals the secrets it carries and writes history to storage.
//!
//! Secrets stay in [`Secret`] or [`SourceRecord`], whose `Debug` output is redacted, and
//! the [`DryRunReport`] only counts them.

mod channels;
mod history;
mod monitors;
mod pages;
mod plan;
mod planner;
mod record;
mod report;
mod source;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

pub use history::{
    HeartbeatChunk, Heartbeats, HistoryError, KumaHeartbeat, KumaStat, LatestHeartbeat, StatChunk,
    StatCursor, StatTable, Stats, Status,
};
pub use plan::*;
pub use planner::PlanError;
pub use record::SourceRecord;
pub use report::{
    Disposition, DryRunReport, HeartbeatCounts, InventoryGroup, MonitorTypeCount, ProviderCount,
    TableCount,
};
pub use source::{OpenError, ReadMode};
pub use v1per_checks::Secret;

/// An open, verified Kuma 2.5.0 database.
pub struct Source {
    conn: Connection,
    path: PathBuf,
    read_mode: ReadMode,
}

impl Source {
    /// Opens `path` read-only and checks the knex migrations and columns of Kuma 2.5.0.
    ///
    /// Without a WAL next to it the file is opened with `immutable=1`: no locks and no side
    /// files. When a WAL with content exists (Kuma was not stopped cleanly, or still runs),
    /// it is opened `mode=ro` so committed WAL pages are seen; SQLite may then create the
    /// `-shm` index, but neither the database nor the WAL is written. Stop Kuma first.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, OpenError> {
        let path = path.as_ref();
        let (conn, read_mode) = source::open(path)?;
        Ok(Self {
            conn,
            path: path.to_owned(),
            read_mode,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_mode(&self) -> ReadMode {
        self.read_mode
    }

    /// Maps the configuration tables.
    pub fn plan(&self) -> Result<ImportPlan, PlanError> {
        planner::plan(&self.conn)
    }

    /// Summarises `plan`, counting heartbeats against a raw retention window that starts
    /// at `since_ms` (UTC Unix milliseconds).
    pub fn report(&self, plan: &ImportPlan, since_ms: i64) -> rusqlite::Result<DryRunReport> {
        report::build(
            &self.conn,
            self.path.clone(),
            self.read_mode,
            plan,
            since_ms,
        )
    }

    /// Heartbeats at or after `since_ms`, in ID order, `chunk_size` at a time. Pass the
    /// last chunk's `last_id` as `after` to resume.
    pub fn heartbeats(
        &self,
        since_ms: i64,
        after: Option<i64>,
        chunk_size: usize,
    ) -> Heartbeats<'_> {
        Heartbeats {
            conn: &self.conn,
            since: history::time_text(since_ms),
            after: after.unwrap_or(i64::MIN),
            chunk_size: chunk_size.max(1),
            done: false,
        }
    }

    /// The newest heartbeat of every monitor that has one: its state at cutover.
    pub fn latest_heartbeats(&self) -> Result<BTreeMap<MonitorId, LatestHeartbeat>, HistoryError> {
        history::latest(&self.conn)
    }

    /// Rows per Kuma monitor of one `stat_*` table, for reconciliation.
    pub fn stat_rows(&self, table: StatTable) -> rusqlite::Result<BTreeMap<MonitorId, u64>> {
        history::stat_rows(&self.conn, table)
    }

    /// Heartbeats per Kuma monitor at or after `since_ms` and before it.
    pub fn heartbeat_rows(
        &self,
        since_ms: i64,
    ) -> rusqlite::Result<BTreeMap<MonitorId, (u64, u64)>> {
        history::heartbeat_rows(&self.conn, since_ms)
    }

    /// Up and down beats per Kuma monitor summed over the buckets of `table` in
    /// `[from_ms, to_ms)`: how Kuma computes an uptime window (decision 0008).
    pub fn uptime_counts(
        &self,
        table: StatTable,
        from_ms: i64,
        to_ms: i64,
    ) -> rusqlite::Result<BTreeMap<MonitorId, (u64, u64)>> {
        history::uptime_counts(&self.conn, table, from_ms, to_ms)
    }

    /// Start of the newest rollup bucket Kuma wrote, UTC Unix milliseconds: when it last
    /// recorded a beat.
    pub fn newest_bucket_ms(&self) -> rusqlite::Result<Option<i64>> {
        history::newest_bucket_ms(&self.conn)
    }

    /// Every row of one `stat_*` table, one monitor per chunk of at most `chunk_size` rows.
    /// Pass the last chunk's `cursor` as `after` to resume.
    pub fn stats(
        &self,
        table: StatTable,
        after: Option<StatCursor>,
        chunk_size: usize,
    ) -> Stats<'_> {
        Stats {
            conn: &self.conn,
            table,
            fetched_until: after.map_or((i64::MIN, i64::MIN), |cursor| {
                (cursor.monitor.0, cursor.timestamp)
            }),
            chunk_size: chunk_size.max(1),
            buffer: VecDeque::new(),
            exhausted: false,
        }
    }
}
