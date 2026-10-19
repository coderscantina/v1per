//! `v1per import kuma` (milestone 10, decision 0007): an Uptime Kuma 2.5.0 SQLite database into
//! an empty node.
//!
//! The source is opened read-only and must be an unmodified Kuma 2.5.0 (`v1per-kuma-import`).
//! A run holds the data directory's lock, so it never runs beside `serve`, and goes through
//! steps that each end in a checkpoint ([`state`]): the configuration in one transaction
//! ([`control`]), then `stat_*` rollups and raw heartbeats in chunks through heartbeat storage
//! ([`history`]), then the reconciliation manifest ([`reconcile`]) together with each
//! monitor's state at cutover, seeded from its newest Kuma heartbeat. Running the same command
//! after an interruption resumes from the checkpoints without duplicates; `serve` refuses to
//! start until the import is complete. A node takes one import, ever.

pub(crate) mod api;
mod control;
mod history;
mod reconcile;
mod state;

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};
use v1per_kuma_import::{ImportPlan, Source};
use v1per_protocol::ids::NodeId;
use v1per_protocol::operations::imports::{ImportManifest, ImportSource, ImportState, ImportStep};
use v1per_storage::{Budgets, Storage};

use crate::config::SecretsArgs;
use crate::db::{CONTROL_DB_FILE, Db, now_ms};
use crate::identity::secrets::SecretKey;
use crate::init::{self, Exit, Failure};
use crate::lock::DataLock;
use crate::nodes::scope::path_cte;
use control::{Target, Written};
use state::Advance;

const DAY_MS: i64 = 86_400_000;

#[derive(clap::Subcommand)]
pub enum Command {
    /// Imports an Uptime Kuma 2.5.0 SQLite database into an empty node, or resumes an
    /// interrupted import into it. Stop the server first.
    Kuma(KumaArgs),
}

#[derive(clap::Args)]
pub struct KumaArgs {
    #[arg(long, env = "V1PER_DATA_DIR", default_value = "data")]
    pub data_dir: PathBuf,
    /// Kuma's `kuma.db`, read and never written. Stop Kuma first, or copy the file together
    /// with its `-wal` and `-shm` files.
    #[arg(long, env = "V1PER_KUMA_DB")]
    pub source: PathBuf,
    /// The empty node to import into: its ID, or its path from the root, such as
    /// `Root/Clients/Acme`.
    #[arg(long)]
    pub into: String,
    /// Prints what the import would bring, map, inventory and refuse, and writes nothing.
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub options: Options,
    #[command(flatten)]
    pub secrets: SecretsArgs,
}

/// Settings a run starts with. A resume must use the same ones.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    /// Domain of the logins of Kuma usernames that are not email addresses:
    /// `<username>@<domain>`.
    #[arg(long, default_value = "kuma.invalid")]
    pub login_domain: String,
    /// Days of raw heartbeats to import, counted back from the start of the import. Older
    /// history comes from Kuma's rollups.
    #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u32).range(1..=3650))]
    pub raw_days: u32,
    /// Source rows per chunk: the unit of memory use and of checkpoints.
    #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u32).range(1..=100_000))]
    pub chunk_rows: u32,
}

pub enum Outcome {
    /// The dry-run report.
    DryRun(String),
    Imported {
        manifest: Box<ImportManifest>,
        /// Where the manifest was written.
        path: PathBuf,
    },
}

/// Runs `v1per import kuma`.
pub async fn run(args: KumaArgs, secrets_key: Option<&str>) -> Result<Outcome, Failure> {
    let dir = args.data_dir.clone();
    init::check_initialized(&dir)?;
    if args.dry_run {
        return dry_run(&args).await.map(Outcome::DryRun);
    }
    let _lock = DataLock::try_acquire(&dir)
        .map_err(|e| Failure::storage(&dir, e))?
        .ok_or_else(|| {
            Failure::new(
                Exit::Refused,
                "server_running",
                format!(
                    "{}: another v1per process, `serve` or an import, uses this data directory; stop it and run the import again",
                    dir.display()
                ),
            )
        })?;
    let key = SecretKey::load(&dir, secrets_key, args.secrets.secrets_key_file.as_deref())
        .map_err(|e| Failure::new(Exit::Refused, "secrets_key", e.to_string()))?;
    let fingerprint = hash_source(&args.source)?;
    let source = open_source(&args.source)?;
    let db = Db::open(&dir)
        .await
        .map_err(|e| Failure::storage(&dir, e))?;
    let result = import(&db, &key, &source, &args, fingerprint).await;
    db.close().await;
    result
}

/// Refuses with `import_in_progress` while an import into this data directory is unfinished:
/// serving would run monitors whose history is still being written.
pub async fn refuse_while_running(db: &Db) -> Result<(), Failure> {
    let mut conn = db
        .reader
        .acquire()
        .await
        .map_err(|e| Failure::new(Exit::Internal, "internal", e.to_string()))?;
    match state::unfinished(&mut conn).await {
        Ok(None) => Ok(()),
        Ok(Some((node, source))) => Err(Failure::new(
            Exit::Refused,
            "import_in_progress",
            format!(
                "the import of {source} into node {node} has not finished; run `v1per import kuma` with the same source and node to resume it"
            ),
        )),
        Err(e) => Err(Failure::new(Exit::Internal, "internal", e.to_string())),
    }
}

async fn import(
    db: &Db,
    key: &SecretKey,
    source: &Source,
    args: &KumaArgs,
    fingerprint: Fingerprint,
) -> Result<Outcome, Failure> {
    let dir = &args.data_dir;
    let mut conn = db
        .writer
        .acquire()
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    let target = resolve(&mut conn, &args.into).await?;
    let plan = plan(source)?;
    let source_name = args.source.display().to_string();
    let existing = state::run_for(&mut conn, target.node)
        .await
        .map_err(internal)?;
    let (run, clock_ms, options) = match existing {
        Some(run) if run.state() == ImportState::Complete => {
            return Err(Failure::new(
                Exit::Refused,
                "already_imported",
                format!(
                    "node {} already holds the import of {}; a node takes one import. Its manifest is {}",
                    target.node,
                    run.source,
                    manifest_path(dir, target.node).display()
                ),
            ));
        }
        Some(run) => {
            if run.fingerprint != fingerprint.sha256 {
                return Err(Failure::new(
                    Exit::Refused,
                    "source_changed",
                    format!(
                        "{source_name} changed since this import started from {}; resume with the same, unchanged file",
                        run.source
                    ),
                ));
            }
            let stored: Options = serde_json::from_str(&run.options).map_err(internal)?;
            if stored != args.options {
                return Err(Failure::new(
                    Exit::Refused,
                    "options_changed",
                    format!(
                        "resume with the options the import started with: --login-domain {} --raw-days {} --chunk-rows {}",
                        stored.login_domain, stored.raw_days, stored.chunk_rows
                    ),
                ));
            }
            (run.id, run.clock_ms, stored)
        }
        None => {
            let problems = control::preflight(&mut conn, &target, &plan, &args.options)
                .await
                .map_err(internal)?;
            if !problems.is_empty() {
                return Err(Failure::new(
                    Exit::Refused,
                    "import_conflict",
                    problems.join("; "),
                ));
            }
            let clock_ms = now_ms();
            let options = serde_json::to_string(&args.options).map_err(internal)?;
            let run = state::start(
                &mut conn,
                target.node,
                &source_name,
                &fingerprint.sha256,
                &options,
                clock_ms,
            )
            .await
            .map_err(internal)?;
            (run, clock_ms, args.options.clone())
        }
    };
    drop(conn);
    tracing::info!(node = %target.node, run, "importing {source_name}");

    let storage = Storage::open(dir, Budgets::default())
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    let run = Run {
        db,
        key,
        storage: &storage,
        source,
        plan: &plan,
        id: run,
        target: &target,
        options: &options,
        clock_ms,
    };
    let result = run.steps(args, fingerprint).await;
    if let Err(error) = storage.close().await {
        tracing::error!(%error, "closing heartbeat storage failed");
    }
    let (manifest, path) = result.map_err(|e| {
        Failure::new(
            Exit::Internal,
            "import_failed",
            format!("{e:#}; run the same command again to resume"),
        )
    })?;
    Ok(Outcome::Imported {
        manifest: Box::new(manifest),
        path,
    })
}

/// One run's steps and what they share.
struct Run<'a> {
    db: &'a Db,
    key: &'a SecretKey,
    storage: &'a Storage,
    source: &'a Source,
    plan: &'a ImportPlan,
    id: i64,
    target: &'a Target,
    options: &'a Options,
    clock_ms: i64,
}

impl Run<'_> {
    async fn steps(
        &self,
        args: &KumaArgs,
        fingerprint: Fingerprint,
    ) -> anyhow::Result<(ImportManifest, PathBuf)> {
        let written = self.control().await?;
        let monitors = {
            let mut conn = self.db.writer.acquire().await?;
            state::objects(&mut conn, self.id, "monitor").await?
        };
        let chunk_rows = self.options.chunk_rows as usize;
        tracing::info!("importing stat_* rollups");
        history::stats(
            &self.db.writer,
            self.storage,
            self.source,
            self.id,
            &monitors,
            chunk_rows,
        )
        .await?;
        tracing::info!("importing raw heartbeats");
        let raw_since = self.clock_ms - i64::from(self.options.raw_days) * DAY_MS;
        history::heartbeats(
            &self.db.writer,
            self.storage,
            self.source,
            self.id,
            &monitors,
            raw_since,
            chunk_rows,
        )
        .await?;
        // What an earlier, interrupted process admitted is moved first, so counts are final.
        self.storage.drain().await;

        tracing::info!("reconciling");
        let after = hash_source(&args.source).map_err(|f| anyhow::anyhow!(f.message))?;
        let source = ImportSource {
            path: args.source.display().to_string(),
            sha256: fingerprint.sha256.clone(),
            bytes: fingerprint.bytes,
            unchanged: after.sha256 == fingerprint.sha256,
        };
        let mut tx = self.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        history::seed_state(&mut tx, self.source, &monitors).await?;
        let finished_at = now_ms();
        let manifest = reconcile::manifest(
            &mut tx,
            self.storage,
            self.source,
            reconcile::Inputs {
                run: self.id,
                node: self.target.node,
                plan: self.plan,
                written,
                source,
                started_at: self.clock_ms,
                raw_since,
                monitors: &monitors,
            },
            finished_at,
        )
        .await?;
        let path = manifest_path(&args.data_dir, self.target.node);
        let json = serde_json::to_string_pretty(&manifest)?;
        let partial = path.with_extension("json.partial");
        std::fs::write(&partial, &json)?;
        std::fs::rename(&partial, &path)?;
        state::complete(&mut tx, self.id, &json, finished_at).await?;
        state::advance(
            &mut tx,
            self.id,
            ImportStep::Reconcile,
            Advance {
                cursor: None,
                chunks: 1,
                rows: 0,
                skipped: 0,
                done: true,
            },
        )
        .await?;
        tx.commit().await?;
        Ok((manifest, path))
    }

    /// Writes the configuration once; a resume reads what it reported from its checkpoint.
    async fn control(&self) -> anyhow::Result<Written> {
        let mut tx = self.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let checkpoint = state::checkpoint_of(&mut tx, self.id, ImportStep::Control).await?;
        if checkpoint.done {
            return Ok(serde_json::from_str(
                checkpoint.cursor.as_deref().unwrap_or("{}"),
            )?);
        }
        tracing::info!("importing configuration");
        let written = control::write(
            &mut tx,
            self.key,
            self.id,
            self.target,
            self.plan,
            self.options,
            self.clock_ms,
        )
        .await?;
        tx.commit().await?;
        Ok(written)
    }
}

/// Where the manifest of the import into `node` is written.
pub fn manifest_path(dir: &Path, node: NodeId) -> PathBuf {
    dir.join(format!("import-node-{node}.json"))
}

/// What `v1per import kuma` prints when it is done.
pub fn summary(manifest: &ImportManifest, path: &Path) -> String {
    let mut text = String::new();
    let verdict = if manifest.reconciled {
        "reconciled"
    } else {
        "NOT reconciled"
    };
    let _ = writeln!(text, "Kuma import into node {}: {verdict}", manifest.node);
    let tables = manifest.tables.iter().filter(|t| t.reconciled).count();
    let _ = writeln!(
        text,
        "  tables     {tables} of {} reconciled; nodes from groups {} of {}",
        manifest.tables.len(),
        manifest.nodes.created,
        manifest.nodes.expected
    );
    for table in manifest.tables.iter().filter(|t| !t.reconciled) {
        let _ = writeln!(
            text,
            "    {}: source {}, mapped {}, inventoried {}, in v1per {}",
            table.table,
            table.source,
            table.mapped,
            table.inventoried,
            table
                .destination
                .map_or_else(|| "-".to_owned(), |n| n.to_string())
        );
    }
    for history in &manifest.history {
        let _ = writeln!(
            text,
            "  {:<14} source {}, imported {}, skipped {}, outside window {}{}",
            history.table,
            history.source,
            history.imported,
            history.skipped,
            history.outside_window,
            if history.reconciled { "" } else { "  DIFFERS" }
        );
    }
    let _ = writeln!(
        text,
        "  uptime     {} of {} monitors match Kuma over 24h, 30d and 1y",
        manifest.uptime.matched, manifest.uptime.monitors
    );
    for login in &manifest.logins {
        let _ = writeln!(
            text,
            "  login      {} -> {}{}{}",
            login.kuma_username,
            login.email,
            if login.two_factor { ", 2FA kept" } else { "" },
            match login.password {
                v1per_protocol::operations::imports::ImportedPassword::Kept => "",
                v1per_protocol::operations::imports::ImportedPassword::Reset =>
                    ", needs a new password",
            }
        );
    }
    let _ = writeln!(
        text,
        "  source     {}",
        if manifest.source.unchanged {
            "unchanged"
        } else {
            "CHANGED during the import"
        }
    );
    let _ = writeln!(text, "  manifest   {}", path.display());
    text
}

async fn dry_run(args: &KumaArgs) -> Result<String, Failure> {
    let source = open_source(&args.source)?;
    let plan = plan(&source)?;
    let since = now_ms() - i64::from(args.options.raw_days) * DAY_MS;
    let report = source.report(&plan, since).map_err(internal)?;
    let mut text = report.to_string();

    let _ = writeln!(text, "\nLogins");
    for user in &plan.users {
        let _ = writeln!(
            text,
            "  {} -> {}",
            user.username,
            control::login(&user.username, &args.options.login_domain)
        );
    }

    // Read only: the server may be running during a rehearsal.
    let options = SqliteConnectOptions::new()
        .filename(args.data_dir.join(CONTROL_DB_FILE))
        .read_only(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .map_err(|e| Failure::storage(&args.data_dir, e))?;
    let target = resolve(&mut conn, &args.into).await?;
    let _ = writeln!(text, "\nTarget node {}", target.node);
    let migrated: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'import_runs')",
    )
    .fetch_one(&mut conn)
    .await
    .map_err(internal)?;
    let existing = if migrated {
        state::run_for(&mut conn, target.node)
            .await
            .map_err(internal)?
    } else {
        None
    };
    match existing {
        Some(run) if run.state() == ImportState::Complete => {
            let _ = writeln!(text, "  refused: the node already holds an import");
        }
        Some(_) => {
            let _ = writeln!(
                text,
                "  an interrupted import into this node exists; importing resumes it"
            );
        }
        None => {
            let problems = control::preflight(&mut conn, &target, &plan, &args.options)
                .await
                .map_err(internal)?;
            if problems.is_empty() {
                let _ = writeln!(text, "  ready to import");
            }
            for problem in problems {
                let _ = writeln!(text, "  refused: {problem}");
            }
        }
    }
    let _ = conn.close().await;
    Ok(text)
}

/// The node `spec` names: its ID, or its path of names from the root.
async fn resolve(conn: &mut SqliteConnection, spec: &str) -> Result<Target, Failure> {
    let not_found = || {
        Failure::new(
            Exit::Usage,
            "node_not_found",
            format!(
                "no node `{spec}`; name it by ID or by its path from the root, such as `Root/Clients`"
            ),
        )
    };
    let id = match spec.trim().parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            let mut parent: Option<i64> = None;
            for name in spec.split('/').map(str::trim).filter(|n| !n.is_empty()) {
                let ids: Vec<i64> =
                    sqlx::query_scalar("SELECT id FROM nodes WHERE parent_id IS ? AND name = ?")
                        .bind(parent)
                        .bind(name)
                        .fetch_all(&mut *conn)
                        .await
                        .map_err(internal)?;
                match ids.as_slice() {
                    [id] => parent = Some(*id),
                    [] => return Err(not_found()),
                    _ => {
                        return Err(Failure::new(
                            Exit::Usage,
                            "node_ambiguous",
                            format!("several nodes match `{spec}`; name the node by its ID"),
                        ));
                    }
                }
            }
            parent.ok_or_else(not_found)?
        }
    };
    let depth: Option<i64> = sqlx::query_scalar(concat!(path_cte!(), "SELECT max(step) FROM path"))
        .bind(id)
        .fetch_one(conn)
        .await
        .map_err(internal)?;
    let depth = depth.ok_or_else(not_found)?;
    Ok(Target {
        node: NodeId::new(id),
        depth: depth as u32,
    })
}

fn open_source(path: &Path) -> Result<Source, Failure> {
    Source::open(path).map_err(|e| Failure::new(Exit::Refused, "source_refused", e.to_string()))
}

fn plan(source: &Source) -> Result<ImportPlan, Failure> {
    source
        .plan()
        .map_err(|e| Failure::new(Exit::Refused, "source_refused", e.to_string()))
}

fn internal(error: impl std::fmt::Display) -> Failure {
    Failure::new(Exit::Internal, "internal", error.to_string())
}

struct Fingerprint {
    sha256: String,
    bytes: u64,
}

/// SHA-256 over the database file and, when it has content, its WAL: what the import reads.
fn hash_source(path: &Path) -> Result<Fingerprint, Failure> {
    let unreadable = |e: std::io::Error| {
        Failure::new(
            Exit::Usage,
            "source_unreadable",
            format!("{}: {e}", path.display()),
        )
    };
    let mut hasher = Sha256::new();
    let mut bytes = 0;
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let wal = PathBuf::from(wal);
    let has_wal = std::fs::metadata(&wal).is_ok_and(|meta| meta.len() > 0);
    for file in std::iter::once(path).chain(has_wal.then_some(wal.as_path())) {
        let mut file = std::fs::File::open(file).map_err(unreadable)?;
        let mut buffer = vec![0; 1 << 20];
        loop {
            let read = file.read(&mut buffer).map_err(unreadable)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            bytes += read as u64;
        }
    }
    let digest = hasher.finalize();
    Ok(Fingerprint {
        sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
        bytes,
    })
}
