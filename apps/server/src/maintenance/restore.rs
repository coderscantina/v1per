//! `v1per restore` into a new data directory (decision 0010). The manifest is read first:
//! a backup of a newer release, or without its secrets key, is refused before anything is
//! written. The files are extracted with their checksums checked; then the engines open once
//! as `serve` opens them, transfer applies what the backup's inbox held, and the ledger must
//! end at the manifest's applied records plus its pending ones. Only then the directory is
//! marked initialized.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use v1per_storage::{Budgets, Positions, Storage};

use super::backup::{self, FileEntry, Manifest};
use super::{internal, io_failure, storage_failure, versions};
use crate::db::Db;
use crate::identity::secrets::{SECRETS_KEY_FILE, SecretKey};
use crate::init::{DONE_MARKER, Exit, Failure};
use crate::lock::{DataLock, LOCK_FILE};

/// Written before the first file is extracted and removed once the restore is checked. A
/// directory with it is not served, and a rerun starts over.
pub const IN_PROGRESS_MARKER: &str = "v1per.restore-in-progress";

#[derive(Debug, Clone)]
pub struct Restored {
    pub data_dir: PathBuf,
    /// `ready`, or `upgrade_required` for a backup of an older schema.
    pub state: String,
    pub positions: Positions,
    /// Inbox rows transfer applied after the restore.
    pub records_applied: Option<i64>,
    /// Whether the key was written to `secrets.key`; otherwise configure it as before.
    pub key_in_data_dir: bool,
}

/// Restores `from` into `data_dir`, which must be missing or empty. `key` is the key the
/// operator configured; without one the backup must hold its own.
pub async fn restore(
    from: &Path,
    data_dir: &Path,
    key: Option<SecretKey>,
) -> Result<Restored, Failure> {
    let lock = claim(data_dir)?;
    let (from, dir) = (from.to_path_buf(), data_dir.to_path_buf());
    let extracted = tokio::task::spawn_blocking(move || extract(&from, &dir, key))
        .await
        .expect("restore task");
    let manifest = match extracted {
        Ok(manifest) => manifest,
        Err(failure) => {
            // Nothing of a refused backup stays behind; the directory is empty again.
            let _ = clear(data_dir, false);
            return Err(failure);
        }
    };
    let ready = manifest.schema == versions::release();
    let records_applied = if ready {
        Some(validate(data_dir, &manifest).await?)
    } else {
        None
    };
    let failed = |e| io_failure(data_dir, e);
    fs::write(data_dir.join(DONE_MARKER), b"").map_err(failed)?;
    fs::remove_file(data_dir.join(IN_PROGRESS_MARKER)).map_err(failed)?;
    drop(lock);
    Ok(Restored {
        data_dir: data_dir.to_path_buf(),
        state: if ready { "ready" } else { "upgrade_required" }.to_owned(),
        positions: manifest.positions,
        records_applied,
        key_in_data_dir: manifest.key.in_data_dir,
    })
}

/// Locks a missing or empty directory, or one with the leftovers of an interrupted restore,
/// which are removed.
fn claim(dir: &Path) -> Result<DataLock, Failure> {
    let failed = |e| io_failure(dir, e);
    if dir.join(IN_PROGRESS_MARKER).exists() {
        let lock = DataLock::acquire(dir)?;
        clear(dir, true).map_err(failed)?;
        return Ok(lock);
    }
    match fs::read_dir(dir).map(|mut entries| entries.next().is_none()) {
        Ok(true) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dir).map_err(failed)?;
        }
        Ok(false) => {
            return Err(Failure::new(
                Exit::Refused,
                if dir.join(DONE_MARKER).exists() {
                    "already_initialized"
                } else {
                    "directory_not_empty"
                },
                format!("{}: restore needs a new, empty directory", dir.display()),
            ));
        }
        Err(e) => return Err(failed(e)),
    }
    DataLock::acquire(dir)
}

/// Removes everything in `dir`, the lock file only without `keep_lock`.
fn clear(dir: &Path, keep_lock: bool) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if keep_lock && path.file_name() == Some(LOCK_FILE.as_ref()) {
            continue;
        }
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Checks the manifest and the key, then extracts into `dir`, which this process locked.
fn extract(from: &Path, dir: &Path, given: Option<SecretKey>) -> Result<Manifest, Failure> {
    let mut archive = backup::open(from)?;
    let mut entries = archive.entries().map_err(|e| backup::corrupt(from, e))?;
    let manifest = backup::read_manifest(from, &mut entries)?;
    if let Some(newer) = versions::newer(manifest.schema, versions::release()) {
        return Err(Failure::new(
            Exit::Refused,
            "data_newer_than_binary",
            format!(
                "{newer}; restore with release {} or newer",
                manifest.version
            ),
        ));
    }
    let mut found = Vec::new();
    let included = if manifest.key.included {
        let mut entry = entries
            .next()
            .ok_or_else(|| backup::corrupt(from, "the archive ends before the secrets key"))?
            .map_err(|e| backup::corrupt(from, e))?;
        if backup::entry_name(from, &entry, &manifest)? != SECRETS_KEY_FILE {
            return Err(backup::corrupt(
                from,
                "the secrets key is not where the manifest says",
            ));
        }
        let mut bytes = Vec::new();
        let (size, blake3) =
            backup::copy_hashed(&mut entry, &mut bytes).map_err(|e| backup::corrupt(from, e))?;
        found.push(FileEntry {
            name: SECRETS_KEY_FILE.to_owned(),
            bytes: size,
            blake3,
        });
        Some(
            SecretKey::from_key_file(&bytes, "the backup's secrets key")
                .map_err(|e| backup::corrupt(from, e))?,
        )
    } else {
        None
    };
    let fingerprint = &manifest.key.fingerprint;
    let key = given.or(included).ok_or_else(|| {
        Failure::new(
            Exit::Refused,
            "secrets_key_required",
            format!(
                "this backup does not hold its secrets key (fingerprint {fingerprint}), and every stored secret is sealed with it. Pass the key the instance used with --secrets-key-file <file> or V1PER_SECRETS_KEY"
            ),
        )
    })?;
    if key.fingerprint() != *fingerprint {
        return Err(Failure::new(
            Exit::Refused,
            "secrets_key_mismatch",
            format!(
                "the given secrets key has fingerprint {}, this backup needs {fingerprint}; the stored secrets would not open with it",
                key.fingerprint()
            ),
        ));
    }

    let failed = |e| io_failure(dir, e);
    fs::write(dir.join(IN_PROGRESS_MARKER), b"").map_err(failed)?;
    for entry in entries {
        let mut entry = entry.map_err(|e| backup::corrupt(from, e))?;
        let name = backup::entry_name(from, &entry, &manifest)?;
        if name == SECRETS_KEY_FILE {
            return Err(backup::corrupt(from, "the secrets key appears twice"));
        }
        let mut file = BufWriter::new(File::create(dir.join(&name)).map_err(failed)?);
        let (bytes, blake3) = backup::copy_hashed(&mut entry, &mut file).map_err(|e| {
            if e.kind() == io::ErrorKind::StorageFull {
                io_failure(dir, e)
            } else {
                backup::corrupt(from, e)
            }
        })?;
        file.into_inner()
            .map_err(|e| failed(e.into_error()))?
            .sync_all()
            .map_err(failed)?;
        found.push(FileEntry {
            name,
            bytes,
            blake3,
        });
    }
    backup::check_files(from, &manifest, &found)?;
    if manifest.key.in_data_dir {
        write_key(&dir.join(SECRETS_KEY_FILE), &key).map_err(failed)?;
    }
    Ok(manifest)
}

fn write_key(path: &Path, key: &SecretKey) -> io::Result<()> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(key.bytes())?;
    file.sync_all()
}

/// Opens the restored engines, requires an instance admin, lets transfer apply the backup's
/// inbox and checks the ledger. Returns the records applied.
async fn validate(dir: &Path, manifest: &Manifest) -> Result<i64, Failure> {
    let db = Db::open(dir).await.map_err(|e| Failure::storage(dir, e))?;
    let admins: Result<i64, _> =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE instance_admin = 1")
            .fetch_one(&db.reader)
            .await;
    db.close().await;
    if admins.map_err(internal)? == 0 {
        return Err(Failure::new(
            Exit::Refused,
            "restore_invalid",
            "the backup has no instance admin who could sign in",
        ));
    }
    let storage = Storage::open(dir, Budgets::default())
        .await
        .map_err(|e| storage_failure(dir, e))?;
    storage.drain().await;
    storage.close().await.map_err(|e| storage_failure(dir, e))?;
    let positions = v1per_storage::inspect(dir)
        .await
        .map_err(|e| storage_failure(dir, e))?;
    let expected = manifest.positions.applied_records + manifest.positions.pending_records;
    if positions.applied_records != expected || positions.pending_records != 0 {
        return Err(Failure::new(
            Exit::Refused,
            "restore_mismatch",
            format!(
                "the restored ledger holds {} records with {} pending, the backup accepted {expected}",
                positions.applied_records, positions.pending_records
            ),
        ));
    }
    Ok(positions.applied_records - manifest.positions.applied_records)
}
