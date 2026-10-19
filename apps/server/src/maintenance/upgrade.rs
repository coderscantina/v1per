//! `v1per upgrade`: a verified backup of the stopped installation first, then the control
//! database, then the inbox. The marker names the backup from the first migration until the
//! version check, so a failed or killed upgrade is a state `serve` refuses, and a rerun
//! resumes without a second backup.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use v1per_protocol::operations::instance::SchemaVersions;
use v1per_storage::{Budgets, Storage};

use super::backup::{self, Source};
use super::versions::{self, UPGRADE_MARKER};
use super::{io_failure, storage_failure};
use crate::db::Db;
use crate::identity::secrets::SecretKey;
use crate::init::{Exit, Failure, check_initialized};
use crate::lock::DataLock;

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    backup: PathBuf,
    from: SchemaVersions,
}

#[derive(Debug)]
pub struct Upgraded {
    /// `current` when there was nothing to do.
    pub state: String,
    pub from: SchemaVersions,
    pub to: SchemaVersions,
    pub backup: Option<PathBuf>,
}

/// `key` seals the secrets of this installation; the backup records its fingerprint.
pub async fn upgrade(
    data_dir: &Path,
    backup_to: Option<&Path>,
    key: impl FnOnce() -> Result<SecretKey, Failure>,
) -> Result<Upgraded, Failure> {
    check_initialized(data_dir)?;
    let _lock = DataLock::acquire(data_dir)?;
    let release = versions::release();
    let marker_path = data_dir.join(UPGRADE_MARKER);
    let marker = match fs::read_to_string(&marker_path) {
        Ok(text) => {
            let marker: Marker = serde_json::from_str(&text).map_err(|e| {
                Failure::new(
                    Exit::Refused,
                    "upgrade_interrupted",
                    format!("{}: {e}", marker_path.display()),
                )
            })?;
            // Resuming relies on the backup the first run took.
            backup::verify(&marker.backup).await?;
            marker
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let stored = versions::stored(data_dir).await?;
            if let Some(newer) = versions::newer(stored, release) {
                return Err(Failure::new(
                    Exit::Refused,
                    "data_newer_than_binary",
                    format!("{newer}; install the matching or a newer release"),
                ));
            }
            if !versions::older(stored, release) {
                return Ok(Upgraded {
                    state: "current".into(),
                    from: stored,
                    to: release,
                    backup: None,
                });
            }
            let to = backup_to.ok_or_else(|| {
                Failure::new(
                    Exit::Usage,
                    "usage",
                    "--backup-to <file> is required: the upgrade takes a backup first",
                )
            })?;
            let taken = backup::take(Source::Stopped(data_dir), &key()?, to, false).await?;
            let marker = Marker {
                backup: taken.path,
                from: stored,
            };
            fs::write(
                &marker_path,
                serde_json::to_string(&marker).expect("markers serialize"),
            )
            .map_err(|e| io_failure(data_dir, e))?;
            marker
        }
        Err(e) => return Err(io_failure(data_dir, e)),
    };

    let failed = |step: &str, error: &dyn std::fmt::Display| {
        Failure::new(
            Exit::Internal,
            "upgrade_failed",
            format!(
                "{step} failed: {error}. The data directory is half upgraded and is not served. \
                 Fix the cause and run `v1per upgrade --data-dir {dir}` again to resume, or \
                 restore the pre-upgrade backup into a new directory: \
                 v1per restore --from {backup} --data-dir <new directory>",
                dir = data_dir.display(),
                backup = marker.backup.display(),
            ),
        )
    };
    Db::open(data_dir)
        .await
        .map_err(|e| failed("control.sqlite migration", &e))?
        .close()
        .await;
    Storage::open(data_dir, Budgets::default())
        .await
        .map_err(|e| failed("inbox migration", &e))?
        .close()
        .await
        .map_err(|e| storage_failure(data_dir, e))?;
    let upgraded = versions::stored(data_dir).await?;
    if upgraded != release {
        return Err(failed(
            "version check",
            &format!("the data is at {upgraded:?}, the release at {release:?}"),
        ));
    }
    fs::remove_file(&marker_path).map_err(|e| io_failure(data_dir, e))?;
    Ok(Upgraded {
        state: "upgraded".into(),
        from: marker.from,
        to: release,
        backup: Some(marker.backup),
    })
}
