//! SQLite durable inbox: admission commits, agent batch IDs, stable batch assignment,
//! retirement.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sqlx::Row;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteSynchronous,
};

use crate::record::{BatchId, INBOX_FORMAT, Payload, decode};
use crate::{Admitted, Error, Refusal, failpoint, now_ms};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// One admission, already encoded by the caller.
pub(crate) struct Encoded {
    /// An agent batch ID with the fingerprint of its heartbeats.
    pub batch: Option<(BatchId, blake3::Hash)>,
    /// Heartbeats plus monitor-level entries.
    pub entries: u64,
    pub payload: Vec<u8>,
}

/// A stable transfer batch: its ID and membership never change once committed.
pub(crate) struct Batch {
    pub id: i64,
    pub first_seq: i64,
    pub last_seq: i64,
    pub bytes: u64,
    /// Inbox rows in sequence order, each decoded.
    pub rows: Vec<(i64, Payload)>,
}

impl Batch {
    fn entries(&self) -> u64 {
        self.rows.iter().map(|(_, p)| p.entries()).sum()
    }
}

/// What waits in the inbox, kept in memory so admission checks its budget without a query.
#[derive(Default)]
pub(crate) struct Backlog {
    pub rows: AtomicU64,
    pub entries: AtomicU64,
    pub bytes: AtomicU64,
}

#[derive(Clone)]
pub(crate) struct Inbox {
    writer: SqlitePool,
    reader: SqlitePool,
    pub backlog: Arc<Backlog>,
}

impl Inbox {
    pub async fn open(path: &Path) -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        // macOS fsync does not flush the drive cache; FULL needs F_FULLFSYNC there.
        #[cfg(target_os = "macos")]
        let options = options.pragma("fullfsync", "ON");

        // Incremental vacuum lets a drained inbox give its peak size back.
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            // A COMMIT that fails (for example on a full disk) can leave the connection inside a
            // transaction, and every later write would fail until a restart. Replace it instead.
            .before_acquire(|conn, _| {
                Box::pin(async move { Ok(!sqlx::Connection::is_in_transaction(conn)) })
            })
            .connect_with(options.clone().auto_vacuum(SqliteAutoVacuum::Incremental))
            .await?;
        MIGRATOR.run(&writer).await?;
        let reader = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options.read_only(true))
            .await?;

        let (rows, entries, bytes): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*), coalesce(sum(entries), 0), coalesce(sum(bytes), 0) FROM inbox",
        )
        .fetch_one(&writer)
        .await?;
        let backlog = Backlog::default();
        backlog.rows.store(rows as u64, Ordering::Relaxed);
        backlog.entries.store(entries as u64, Ordering::Relaxed);
        backlog.bytes.store(bytes as u64, Ordering::Relaxed);
        Ok(Self {
            writer,
            reader,
            backlog: Arc::new(backlog),
        })
    }

    /// Commits a group of admissions in one durable transaction. Returns one outcome per
    /// admission; a refused one stores nothing and does not affect the others.
    pub async fn commit(
        &self,
        requests: &[&Encoded],
    ) -> sqlx::Result<Vec<Result<Admitted, Refusal>>> {
        let now = now_ms();
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let mut outcomes = Vec::with_capacity(requests.len());
        let (mut rows, mut entries, mut bytes) = (0u64, 0u64, 0u64);
        for request in requests {
            if let Some((batch, fingerprint)) = &request.batch {
                let seen: Option<(Vec<u8>, i64)> = sqlx::query_as(
                    "SELECT fingerprint, seq FROM agent_batches WHERE probe_id = ? AND batch_key = ?",
                )
                .bind(batch.probe.0)
                .bind(&batch.id)
                .fetch_optional(&mut *tx)
                .await?;
                if let Some((first, sequence)) = seen {
                    outcomes.push(if first == fingerprint.as_bytes() {
                        Ok(Admitted {
                            sequence,
                            repeated: true,
                        })
                    } else {
                        Err(Refusal::BatchConflict)
                    });
                    continue;
                }
            }
            let sequence: i64 = sqlx::query_scalar(
                "INSERT INTO inbox (received_at, format, entries, bytes, payload)
                 VALUES (?, ?, ?, ?, ?) RETURNING seq",
            )
            .bind(now)
            .bind(INBOX_FORMAT)
            .bind(request.entries as i64)
            .bind(request.payload.len() as i64)
            .bind(request.payload.as_slice())
            .fetch_one(&mut *tx)
            .await?;
            if let Some((batch, fingerprint)) = &request.batch {
                sqlx::query(
                    "INSERT INTO agent_batches (probe_id, batch_key, fingerprint, seq, received_at)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(batch.probe.0)
                .bind(&batch.id)
                .bind(&fingerprint.as_bytes()[..])
                .bind(sequence)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
            rows += 1;
            entries += request.entries;
            bytes += request.payload.len() as u64;
            outcomes.push(Ok(Admitted {
                sequence,
                repeated: false,
            }));
        }
        failpoint::hit("admission.before_commit");
        tx.commit().await?;
        self.backlog.rows.fetch_add(rows, Ordering::Relaxed);
        self.backlog.entries.fetch_add(entries, Ordering::Relaxed);
        self.backlog.bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(outcomes)
    }

    /// Returns the oldest assigned batch, or assigns a new one from pending rows.
    pub async fn next_batch(
        &self,
        max_rows: usize,
        max_bytes: u64,
    ) -> Result<Option<Batch>, Error> {
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        let assigned: Option<(i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT id, first_seq, last_seq, bytes FROM batches ORDER BY id LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let (id, first_seq, last_seq, bytes) = match assigned {
            Some((id, first, last, bytes)) => {
                tx.commit().await?;
                (id, first, last, bytes as u64)
            }
            None => {
                let pending: Vec<(i64, i64)> = sqlx::query_as(
                    "SELECT seq, bytes FROM inbox WHERE batch_id IS NULL ORDER BY seq LIMIT ?",
                )
                .bind(max_rows as i64)
                .fetch_all(&mut *tx)
                .await?;
                let Some(&(first_seq, _)) = pending.first() else {
                    tx.commit().await?;
                    return Ok(None);
                };
                failpoint::hit("transfer.before_assign");
                let (mut last_seq, mut bytes, mut count) = (0i64, 0u64, 0i64);
                for &(seq, size) in &pending {
                    if count > 0 && bytes + size as u64 > max_bytes {
                        break;
                    }
                    last_seq = seq;
                    bytes += size as u64;
                    count += 1;
                }
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO batches (first_seq, last_seq, records, bytes, assigned_at)
                     VALUES (?, ?, ?, ?, ?) RETURNING id",
                )
                .bind(first_seq)
                .bind(last_seq)
                .bind(count)
                .bind(bytes as i64)
                .bind(now_ms())
                .fetch_one(&mut *tx)
                .await?;
                sqlx::query(
                    "UPDATE inbox SET batch_id = ? WHERE batch_id IS NULL AND seq BETWEEN ? AND ?",
                )
                .bind(id)
                .bind(first_seq)
                .bind(last_seq)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                failpoint::hit("transfer.after_assign");
                (id, first_seq, last_seq, bytes)
            }
        };

        let stored =
            sqlx::query("SELECT seq, format, payload FROM inbox WHERE batch_id = ? ORDER BY seq")
                .bind(id)
                .fetch_all(&self.reader)
                .await?;
        let mut rows = Vec::with_capacity(stored.len());
        for row in stored {
            let seq: i64 = row.get(0);
            let format: i64 = row.get(1);
            if format != INBOX_FORMAT {
                return Err(Error::Incompatible(format!(
                    "inbox row {seq} has format {format}"
                )));
            }
            let payload = decode(row.get::<&[u8], _>(2))
                .map_err(|e| Error::Incompatible(format!("inbox row {seq}: {e}")))?;
            rows.push((seq, payload));
        }
        Ok(Some(Batch {
            id,
            first_seq,
            last_seq,
            bytes,
            rows,
        }))
    }

    /// Deletes a batch whose ledger entry is committed in DuckDB.
    pub async fn retire(&self, batch: &Batch) -> sqlx::Result<()> {
        let mut tx = self.writer.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM inbox WHERE batch_id = ?")
            .bind(batch.id)
            .execute(&mut *tx)
            .await?;
        failpoint::hit("transfer.during_retire");
        sqlx::query("DELETE FROM batches WHERE id = ?")
            .bind(batch.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let retired = batch.rows.len() as u64;
        if self.backlog.rows.load(Ordering::SeqCst) == retired {
            // Drained: return free pages to the filesystem before the backlog reads empty, so
            // a drain that returns has given the space back. Under load freed pages are reused.
            sqlx::query("PRAGMA incremental_vacuum")
                .execute(&self.writer)
                .await?;
        }
        self.backlog
            .entries
            .fetch_sub(batch.entries(), Ordering::Relaxed);
        self.backlog.bytes.fetch_sub(batch.bytes, Ordering::Relaxed);
        self.backlog.rows.fetch_sub(retired, Ordering::SeqCst);
        Ok(())
    }

    /// The sequence `batch` was admitted at, if its ID is still kept.
    pub async fn batch_sequence(&self, batch: &BatchId) -> sqlx::Result<Option<i64>> {
        sqlx::query_scalar("SELECT seq FROM agent_batches WHERE probe_id = ? AND batch_key = ?")
            .bind(batch.probe.0)
            .bind(&batch.id)
            .fetch_optional(&self.reader)
            .await
    }

    /// Receive time of the oldest row not yet retired, in Unix milliseconds.
    pub async fn oldest_received_ms(&self) -> sqlx::Result<Option<i64>> {
        sqlx::query_scalar("SELECT received_at FROM inbox ORDER BY seq LIMIT 1")
            .fetch_optional(&self.reader)
            .await
    }

    /// Forgets agent batch IDs first received before `before_ms`.
    pub async fn prune_agent_batches(&self, before_ms: i64) -> sqlx::Result<u64> {
        Ok(
            sqlx::query("DELETE FROM agent_batches WHERE received_at < ?")
                .bind(before_ms)
                .execute(&self.writer)
                .await?
                .rows_affected(),
        )
    }

    /// Agent batch IDs first received before `before_ms`.
    pub async fn count_agent_batches(&self, before_ms: i64) -> sqlx::Result<u64> {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM agent_batches WHERE received_at < ?")
            .bind(before_ms)
            .fetch_one(&self.reader)
            .await
            .map(|count| count as u64)
    }

    /// Writes a consistent copy of the inbox to `path`. A read, so admission goes on.
    pub async fn vacuum_into(&self, path: &Path) -> Result<(), Error> {
        vacuum_into(&self.reader, path).await
    }

    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }
}

/// The inbox schema this release creates: its latest migration.
pub(crate) fn release_schema() -> i64 {
    MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0)
}

/// A read-only pool of one connection on an inbox file that is not open for admission.
pub(crate) async fn read_only(path: &Path) -> sqlx::Result<SqlitePool> {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
}

pub(crate) async fn vacuum_into(pool: &SqlitePool, path: &Path) -> Result<(), Error> {
    let target = path
        .to_str()
        .ok_or_else(|| Error::Incompatible(format!("{} is not UTF-8", path.display())))?;
    sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(pool)
        .await?;
    Ok(())
}

/// The latest applied migration of an inbox file; `None` before the first migration ran,
/// as in a file a killed process created.
pub(crate) async fn stored_schema(pool: &SqlitePool) -> sqlx::Result<Option<i64>> {
    let migrated: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;
    if !migrated {
        return Ok(None);
    }
    sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await
}

/// Inbox rows of batches after `applied_batch`, or not in a batch yet: what transfer still
/// has to apply.
pub(crate) async fn pending_records(pool: &SqlitePool, applied_batch: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM inbox WHERE batch_id IS NULL OR batch_id > ?")
        .bind(applied_batch)
        .fetch_one(pool)
        .await
}
