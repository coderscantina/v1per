//! The operator commands. `restore` and `upgrade` work on a directory no server holds.
//! `backup`, `retention` and `diagnostics` are jobs: while a server holds the data directory
//! they are queued for it and the command waits for the outcome; without one the command
//! takes the lock, opens the engines as `serve` does and runs the queue itself.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use serde_json::Value;
use v1per_protocol::operations::instance::{InstanceOperations, RetentionCounts};
use v1per_storage::{Budgets, Storage};

use super::backup::Summary;
use super::diagnostics::{self, size};
use super::jobs::{self, Job};
use super::restore::{self, Restored};
use super::upgrade::{self, Upgraded};
use super::{internal, storage_failure, versions};
use crate::clock::Clock;
use crate::config::{Config, SecretsArgs};
use crate::db::{Db, now_ms};
use crate::identity::secrets::SecretKey;
use crate::imports;
use crate::init::{Exit, Failure, check_initialized};
use crate::lock::DataLock;
use crate::state::AppState;

const POLL: Duration = Duration::from_millis(200);

#[derive(Args)]
pub struct BackupArgs {
    #[arg(long, env = "V1PER_DATA_DIR")]
    pub data_dir: PathBuf,
    /// The archive to write: a new file, best on other storage than the data directory.
    #[arg(long)]
    pub out: PathBuf,
    /// Also put the secrets key into the archive. Anyone who has the file can then read every
    /// stored secret.
    #[arg(long)]
    pub include_key: bool,
    #[command(flatten)]
    pub secrets: SecretsArgs,
}

#[derive(Args)]
pub struct RestoreArgs {
    /// An archive written by `v1per backup`.
    #[arg(long)]
    pub from: PathBuf,
    /// A new, empty data directory to restore into.
    #[arg(long, env = "V1PER_DATA_DIR")]
    pub data_dir: PathBuf,
    /// The key of the instance the backup came from, unless the backup holds it.
    #[command(flatten)]
    pub secrets: SecretsArgs,
}

#[derive(Args)]
pub struct RetentionArgs {
    #[arg(long, env = "V1PER_DATA_DIR")]
    pub data_dir: PathBuf,
    /// Count what would be removed, and remove nothing.
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub secrets: SecretsArgs,
}

#[derive(Args)]
pub struct DiagnosticsArgs {
    #[arg(long, env = "V1PER_DATA_DIR")]
    pub data_dir: PathBuf,
    /// Print the JSON document `instance.operations` answers with.
    #[arg(long)]
    pub json: bool,
    #[command(flatten)]
    pub secrets: SecretsArgs,
}

#[derive(Args)]
pub struct UpgradeArgs {
    #[arg(long, env = "V1PER_DATA_DIR")]
    pub data_dir: PathBuf,
    /// Where the pre-upgrade backup goes, a new file. Needed unless an interrupted upgrade
    /// resumes.
    #[arg(long)]
    pub backup_to: Option<PathBuf>,
    #[command(flatten)]
    pub secrets: SecretsArgs,
}

/// `env_key` is `V1PER_SECRETS_KEY`, read by the caller from the environment only.
pub async fn backup(args: BackupArgs, env_key: Option<&str>) -> Result<String, Failure> {
    let out = std::path::absolute(&args.out).map_err(|e| Failure::storage(&args.out, e))?;
    if args.include_key {
        eprintln!(
            "warning: --include-key writes the secrets key into the backup. Anyone who has {} can read every stored secret; keep it encrypted and apart from ordinary backups.",
            out.display()
        );
    }
    let job = Job::Backup {
        out,
        include_key: args.include_key,
    };
    let value = run_job(&args.data_dir, job, &args.secrets, env_key).await?;
    let summary: Summary = serde_json::from_value(value).map_err(internal)?;
    let positions = summary.positions;
    let key = if summary.key_included {
        "INCLUDED in the archive"
    } else {
        "not included; keep it, a restore needs it"
    };
    Ok(format!(
        "wrote {} ({})\n  transfer paused {} ms; schema control {} inbox {} heartbeats {}\n  ledger at batch {}: {} records applied, {} pending\n  secrets key {}: {key}\n",
        summary.path.display(),
        size(summary.bytes),
        summary.pause_ms,
        summary.schema.control,
        summary.schema.inbox,
        summary.schema.heartbeats,
        positions.applied_batch,
        positions.applied_records,
        positions.pending_records,
        summary.key_fingerprint,
    ))
}

pub async fn restore(args: RestoreArgs, env_key: Option<&str>) -> Result<String, Failure> {
    let file = args.secrets.secrets_key_file.as_deref();
    let key = (env_key.is_some() || file.is_some())
        .then(|| load_key(Path::new(""), env_key, file))
        .transpose()?;
    let Restored {
        data_dir,
        state,
        records_applied,
        key_in_data_dir,
        ..
    } = restore::restore(&args.from, &args.data_dir, key).await?;
    let mut out = format!("restored into {}: {state}\n", data_dir.display());
    if let Some(applied) = records_applied {
        out += &format!("  {applied} inbox records moved into DuckDB after the restore\n");
    }
    if state == "upgrade_required" {
        out += "  the backup is of an older release: run `v1per upgrade` on it before `serve`\n";
    }
    out += if key_in_data_dir {
        "  secrets key written to secrets.key in the data directory\n"
    } else {
        "  the secrets key stays outside the data directory: start serve with the same V1PER_SECRETS_KEY or --secrets-key-file\n"
    };
    Ok(out)
}

pub async fn retention(args: RetentionArgs, env_key: Option<&str>) -> Result<String, Failure> {
    let job = if args.dry_run {
        Job::RetentionDryRun
    } else {
        Job::Retention
    };
    let value = run_job(&args.data_dir, job, &args.secrets, env_key).await?;
    let counts: RetentionCounts = serde_json::from_value(value).map_err(internal)?;
    let verb = if args.dry_run {
        "dry run, would remove"
    } else {
        "removed"
    };
    Ok(format!("{verb}: {}\n", diagnostics::counts(&counts)))
}

pub async fn diagnostics(args: DiagnosticsArgs, env_key: Option<&str>) -> Result<String, Failure> {
    let value = run_job(&args.data_dir, Job::Diagnostics, &args.secrets, env_key).await?;
    if args.json {
        return Ok(serde_json::to_string_pretty(&value).map_err(internal)? + "\n");
    }
    let ops: InstanceOperations = serde_json::from_value(value).map_err(internal)?;
    Ok(diagnostics::render(&ops))
}

pub async fn upgrade(args: UpgradeArgs, env_key: Option<&str>) -> Result<String, Failure> {
    let dir = &args.data_dir;
    let file = args.secrets.secrets_key_file.as_deref();
    let backup_to = args
        .backup_to
        .as_deref()
        .map(std::path::absolute)
        .transpose()
        .map_err(|e| Failure::storage(dir, e))?;
    let Upgraded {
        state,
        from,
        to,
        backup,
    } = upgrade::upgrade(dir, backup_to.as_deref(), || load_key(dir, env_key, file)).await?;
    let versions = |v: v1per_protocol::operations::instance::SchemaVersions| {
        format!(
            "control {} inbox {} heartbeats {}",
            v.control, v.inbox, v.heartbeats
        )
    };
    Ok(match backup {
        None => format!("the data is current: {}\n", versions(to)),
        Some(backup) => format!(
            "{state} from {} to {}\n  pre-upgrade backup: {}\n",
            versions(from),
            versions(to),
            backup.display()
        ),
    })
}

fn load_key(dir: &Path, env_key: Option<&str>, file: Option<&Path>) -> Result<SecretKey, Failure> {
    SecretKey::load(dir, env_key, file)
        .map_err(|e| Failure::new(Exit::Refused, "secrets_key", e.to_string()))
}

/// Runs `job` on the server that holds the data directory, or here when none does.
async fn run_job(
    dir: &Path,
    job: Job,
    secrets: &SecretsArgs,
    env_key: Option<&str>,
) -> Result<Value, Failure> {
    check_initialized(dir)?;
    let Some(_lock) = DataLock::try_acquire(dir).map_err(|e| Failure::storage(dir, e))? else {
        // A server holds the directory. Connect without migrating: it may be another release.
        let db = Db::connect(dir)
            .await
            .map_err(|e| Failure::storage(dir, e))?;
        let result = async {
            // An import holds the lock and runs no jobs, so waiting for one would hang.
            imports::refuse_while_running(&db).await?;
            let id = jobs::enqueue(&db.writer, &job, now_ms())
                .await
                .map_err(internal)?;
            loop {
                if let Some(result) = jobs::finished(&db.reader, id).await.map_err(internal)? {
                    return result;
                }
                tokio::time::sleep(POLL).await;
            }
        }
        .await;
        db.close().await;
        return result;
    };
    versions::check_dir(dir, false).await?;
    let key = load_key(dir, env_key, secrets.secrets_key_file.as_deref())?;
    let db = Db::open(dir).await.map_err(|e| Failure::storage(dir, e))?;
    if let Err(failure) = imports::refuse_while_running(&db).await {
        db.close().await;
        return Err(failure);
    }
    let storage = match Storage::open(dir, Budgets::default()).await {
        Ok(storage) => Arc::new(storage),
        Err(error) => {
            db.close().await;
            return Err(storage_failure(dir, error));
        }
    };
    let config = Config {
        data_dir: dir.to_path_buf(),
        bind: ([127, 0, 0, 1], 0).into(),
        web_dir: None,
        public_origin: None,
        instance_name: None,
    };
    let state = AppState::new(db.clone(), config, Clock::system(), key, storage.clone());
    let result = match jobs::enqueue(&db.writer, &job, now_ms()).await {
        Ok(id) => jobs::run_until(&state, id).await,
        Err(error) => Err(internal(error)),
    };
    drop(state);
    let closed = storage.shutdown().await;
    db.close().await;
    let value = result?;
    closed.map_err(|e| storage_failure(dir, e))?;
    Ok(value)
}
