//! `v1per backup` (decision 0010): one tar archive. The manifest comes first, then the secrets
//! key when the operator asked for it, then `control.sqlite`, `inbox.sqlite` and
//! `heartbeats.duckdb`. The manifest holds the schema versions, the ledger positions, the
//! key's fingerprint and the size and BLAKE3 hash of every file.
//!
//! On a running server the heartbeat file and the inbox are copied while the DuckDB writer
//! waits, so the pair matches; admission and reads go on and the inbox absorbs new results.
//! The control database is copied right after, so it may hold a few seconds more of state
//! than the heartbeats. The files are staged next to the archive,
//! hashed, written into `.<name>.partial`, verified by reading it back, and only then renamed.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use v1per_protocol::operations::instance::SchemaVersions;
use v1per_storage::{HEARTBEATS_FILE, INBOX_FILE, Positions};

use super::{internal, io_failure, storage_failure, versions};
use crate::db::CONTROL_DB_FILE;
use crate::identity::secrets::{SECRETS_KEY_FILE, SecretKey};
use crate::init::{Exit, Failure};
use crate::state::AppState;

pub const MANIFEST_FILE: &str = "manifest.json";
const FORMAT: i64 = 1;
/// Engine files in archive order. One that did not exist yet is left out.
pub(crate) const ENGINE_FILES: [&str; 3] = [CONTROL_DB_FILE, INBOX_FILE, HEARTBEATS_FILE];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: i64,
    pub created_at: i64,
    /// The release that wrote it.
    pub version: String,
    pub schema: SchemaVersions,
    pub positions: Positions,
    /// How long transfer waited for the copy; zero for a stopped installation.
    pub pause_ms: u64,
    pub key: KeyReference,
    /// Every other entry, in archive order.
    pub files: Vec<FileEntry>,
}

/// Which secrets key the backup needs. The key itself is in the archive only when `included`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyReference {
    pub fingerprint: String,
    /// The instance kept it as `secrets.key` in the data directory, so a restore writes it
    /// there. Otherwise it came from `V1PER_SECRETS_KEY` or a key file, and stays there.
    pub in_data_dir: bool,
    pub included: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub bytes: u64,
    pub blake3: String,
}

/// What an operator sees after a backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub path: PathBuf,
    pub bytes: u64,
    pub pause_ms: u64,
    pub schema: SchemaVersions,
    pub positions: Positions,
    pub key_fingerprint: String,
    pub key_included: bool,
}

pub enum Source<'a> {
    /// A serving installation: transfer pauses while heartbeats and the inbox are copied.
    Running(&'a AppState),
    /// The data directory of a stopped installation at any schema, for the pre-upgrade backup.
    Stopped(&'a Path),
}

/// Writes a verified archive to `out`, which must not exist yet.
pub async fn take(
    source: Source<'_>,
    key: &SecretKey,
    out: &Path,
    include_key: bool,
) -> Result<Summary, Failure> {
    let failed = |e: io::Error| io_failure(out, e);
    if out.exists() {
        return Err(Failure::new(
            Exit::Refused,
            "backup_exists",
            format!("{} exists; choose a new file", out.display()),
        ));
    }
    let (Some(parent), Some(name)) = (out.parent(), out.file_name()) else {
        return Err(Failure::new(
            Exit::Usage,
            "usage",
            format!("{} is not a file path", out.display()),
        ));
    };
    let name = name.to_string_lossy();
    fs::create_dir_all(parent).map_err(failed)?;
    let staging = parent.join(format!(".{name}.staging"));
    let partial = parent.join(format!(".{name}.partial"));
    // Leftovers of an interrupted backup to the same file.
    remove(&staging).map_err(failed)?;
    remove(&partial).map_err(failed)?;
    fs::create_dir(&staging).map_err(failed)?;
    let written = write(source, key, &staging, &partial, include_key).await;
    let _ = fs::remove_dir_all(&staging);
    let manifest = match written {
        Ok(manifest) => manifest,
        Err(failure) => {
            let _ = fs::remove_file(&partial);
            return Err(failure);
        }
    };
    fs::rename(&partial, out).map_err(failed)?;
    let bytes = fs::metadata(out).map_err(failed)?.len();
    Ok(Summary {
        path: out.to_path_buf(),
        bytes,
        pause_ms: manifest.pause_ms,
        schema: manifest.schema,
        positions: manifest.positions,
        key_fingerprint: manifest.key.fingerprint,
        key_included: manifest.key.included,
    })
}

async fn write(
    source: Source<'_>,
    key: &SecretKey,
    staging: &Path,
    partial: &Path,
    include_key: bool,
) -> Result<Manifest, Failure> {
    let (data_dir, schema, pause, now) = match source {
        Source::Running(state) => {
            let dir = &state.config.data_dir;
            let pause = state
                .storage
                .snapshot(staging)
                .await
                .map_err(|e| storage_failure(dir, e))?;
            (
                dir.as_path(),
                versions::release(),
                pause,
                state.clock.now_ms(),
            )
        }
        Source::Stopped(dir) => {
            let schema = versions::stored(dir).await?;
            v1per_storage::snapshot_stopped(dir, staging)
                .await
                .map_err(|e| storage_failure(dir, e))?;
            (dir, schema, Duration::ZERO, crate::db::now_ms())
        }
    };
    copy_control(data_dir, staging).await?;
    // Folds a copied DuckDB WAL into its file before anything is hashed.
    let positions = v1per_storage::inspect(staging)
        .await
        .map_err(|e| storage_failure(staging, e))?;
    if include_key {
        fs::write(staging.join(SECRETS_KEY_FILE), key.bytes())
            .map_err(|e| io_failure(staging, e))?;
    }
    let names: Vec<&'static str> = include_key
        .then_some(SECRETS_KEY_FILE)
        .into_iter()
        .chain(ENGINE_FILES)
        .filter(|name| staging.join(name).exists())
        .collect();
    let manifest = Manifest {
        format: FORMAT,
        created_at: now,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        schema,
        positions,
        pause_ms: pause.as_millis() as u64,
        key: KeyReference {
            fingerprint: key.fingerprint(),
            in_data_dir: key.in_data_dir(),
            included: include_key,
        },
        files: Vec::new(),
    };
    let (from, to) = (staging.to_path_buf(), partial.to_path_buf());
    let manifest = tokio::task::spawn_blocking(move || {
        let mut manifest = manifest;
        for name in &names {
            let mut file = File::open(from.join(name))?;
            let (bytes, blake3) = copy_hashed(&mut file, &mut io::sink())?;
            manifest.files.push(FileEntry {
                name: (*name).to_owned(),
                bytes,
                blake3,
            });
        }
        archive(&from, &to, &manifest)?;
        Ok::<_, io::Error>(manifest)
    })
    .await
    .expect("archive task")
    .map_err(|e| io_failure(partial, e))?;
    let archived = verify(partial).await?;
    if archived.files != manifest.files {
        return Err(internal("the archive does not read back as written"));
    }
    Ok(manifest)
}

/// Writes the manifest and the staged files into a new archive at `to`, readable by its owner
/// only: it holds every account and, with `--include-key`, the key to every secret. Each
/// staged file is removed once archived, so the copy needs room for one file at a time.
fn archive(staging: &Path, to: &Path, manifest: &Manifest) -> io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(to)?;
    let mut builder = tar::Builder::new(BufWriter::new(file));
    let mtime = (manifest.created_at / 1000).max(0) as u64;
    let mut append = |name: &str, bytes: u64, data: &mut dyn Read| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(bytes);
        header.set_mode(0o600);
        header.set_mtime(mtime);
        builder.append_data(&mut header, name, data)
    };
    let text = serde_json::to_vec_pretty(manifest).expect("manifests serialize");
    append(MANIFEST_FILE, text.len() as u64, &mut text.as_slice())?;
    for entry in &manifest.files {
        let path = staging.join(&entry.name);
        append(&entry.name, entry.bytes, &mut File::open(&path)?)?;
        fs::remove_file(path)?;
    }
    let file = builder
        .into_inner()?
        .into_inner()
        .map_err(|e| e.into_error())?;
    file.sync_all()
}

/// Reads an archive and checks every file against its manifest.
pub async fn verify(path: &Path) -> Result<Manifest, Failure> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut archive = open(&path)?;
        let mut entries = archive.entries().map_err(|e| corrupt(&path, e))?;
        let manifest = read_manifest(&path, &mut entries)?;
        let mut found = Vec::new();
        for entry in entries {
            let mut entry = entry.map_err(|e| corrupt(&path, e))?;
            let name = entry_name(&path, &entry, &manifest)?;
            let (bytes, blake3) =
                copy_hashed(&mut entry, &mut io::sink()).map_err(|e| corrupt(&path, e))?;
            found.push(FileEntry {
                name,
                bytes,
                blake3,
            });
        }
        check_files(&path, &manifest, &found)?;
        Ok(manifest)
    })
    .await
    .expect("verify task")
}

pub(crate) type Entries<'a> = tar::Entries<'a, BufReader<File>>;

pub(crate) fn open(path: &Path) -> Result<tar::Archive<BufReader<File>>, Failure> {
    let file = File::open(path).map_err(|e| {
        Failure::new(
            Exit::Refused,
            "backup_invalid",
            format!("{}: {e}", path.display()),
        )
    })?;
    Ok(tar::Archive::new(BufReader::new(file)))
}

/// The first entry, which must be a manifest of a format this release reads.
pub(crate) fn read_manifest(path: &Path, entries: &mut Entries) -> Result<Manifest, Failure> {
    let invalid = |reason: String| {
        Failure::new(
            Exit::Refused,
            "backup_invalid",
            format!("{}: {reason}; this is not a v1per backup", path.display()),
        )
    };
    let mut entry = entries
        .next()
        .ok_or_else(|| invalid("the archive is empty".into()))?
        .map_err(|e| invalid(e.to_string()))?;
    let name = entry.path().map_err(|e| invalid(e.to_string()))?;
    if name != Path::new(MANIFEST_FILE) {
        return Err(invalid(format!("it starts with {}", name.display())));
    }
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| invalid(e.to_string()))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| invalid(format!("{MANIFEST_FILE}: {e}")))?;
    if manifest.format != FORMAT {
        return Err(Failure::new(
            Exit::Refused,
            "backup_unsupported",
            format!(
                "{}: backup format {} is not {FORMAT}; restore it with the release that wrote it ({})",
                path.display(),
                manifest.format,
                manifest.version
            ),
        ));
    }
    Ok(manifest)
}

/// The name of an entry the manifest lists. Only listed names are ever written anywhere.
pub(crate) fn entry_name(
    path: &Path,
    entry: &tar::Entry<BufReader<File>>,
    manifest: &Manifest,
) -> Result<String, Failure> {
    let name = entry
        .path()
        .map_err(|e| corrupt(path, e))?
        .to_string_lossy()
        .into_owned();
    if manifest.files.iter().any(|file| file.name == name) {
        Ok(name)
    } else {
        Err(Failure::new(
            Exit::Refused,
            "backup_corrupt",
            format!(
                "{}: {name} is not in the manifest; this backup cannot be restored",
                path.display()
            ),
        ))
    }
}

pub(crate) fn check_files(
    path: &Path,
    manifest: &Manifest,
    found: &[FileEntry],
) -> Result<(), Failure> {
    if let Some(listed) = manifest.files.iter().find(|listed| !found.contains(listed)) {
        return Err(Failure::new(
            Exit::Refused,
            "backup_corrupt",
            format!(
                "{}: {} does not match its checksum in the manifest; this backup cannot be restored",
                path.display(),
                listed.name
            ),
        ));
    }
    Ok(())
}

pub(crate) fn corrupt(path: &Path, error: impl std::fmt::Display) -> Failure {
    Failure::new(
        Exit::Refused,
        "backup_corrupt",
        format!(
            "{}: {error}; this backup cannot be restored",
            path.display()
        ),
    )
}

/// Copies `from` into `to` and returns the bytes copied and their BLAKE3 hash.
pub(crate) fn copy_hashed(from: &mut dyn Read, to: &mut dyn Write) -> io::Result<(u64, String)> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buffer[..read]);
        to.write_all(&buffer[..read])?;
        total += read as u64;
    }
    to.flush()?;
    Ok((total, hasher.finalize().to_hex().to_string()))
}

/// A consistent copy of the control database through a connection of its own: a read, so
/// commands go on.
async fn copy_control(data_dir: &Path, staging: &Path) -> Result<(), Failure> {
    let target = staging.join(CONTROL_DB_FILE);
    let target = target
        .to_str()
        .ok_or_else(|| internal(format!("{} is not UTF-8", staging.display())))?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(data_dir.join(CONTROL_DB_FILE))
                .read_only(true),
        )
        .await
        .map_err(|e| Failure::storage(data_dir, e))?;
    let copied = sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(&pool)
        .await;
    pool.close().await;
    copied.map_err(|e| super::retention::sql_failure(staging, e))?;
    Ok(())
}

fn remove(path: &Path) -> io::Result<()> {
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match removed {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
