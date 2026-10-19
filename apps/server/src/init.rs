//! `v1per init` and the data-directory state `serve` requires.

use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use v1per_protocol::limits::{PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS};

use crate::db::{CONTROL_DB_FILE, Db, now_ms};
use crate::identity::{self, secrets::SecretKey};
use crate::lock::DataLock;

pub const IN_PROGRESS_MARKER: &str = "v1per.init-in-progress";
pub const DONE_MARKER: &str = "v1per.initialized";

/// Exit codes shared by every `v1per` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Internal = 1,
    Usage = 2,
    Refused = 3,
    StorageUnavailable = 4,
}

#[derive(Debug)]
pub struct Failure {
    pub exit: Exit,
    /// A fixed outcome name such as `already_initialized`.
    pub code: Cow<'static, str>,
    pub message: String,
}

impl Failure {
    pub fn new(exit: Exit, code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Self {
        Self {
            exit,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn storage(path: &Path, error: impl std::fmt::Display) -> Self {
        Self::new(
            Exit::StorageUnavailable,
            "storage_unavailable",
            format!("{}: {error}", path.display()),
        )
    }
}

pub struct InitArgs {
    pub data_dir: PathBuf,
    pub admin_email: String,
    pub admin_password: Option<String>,
    /// `V1PER_SECRETS_KEY`. With it or `secrets_key_file`, no `secrets.key` is written.
    pub secrets_key: Option<String>,
    pub secrets_key_file: Option<PathBuf>,
}

/// What a successful init reports. `generated_password` is printed once and stored nowhere.
pub struct Initialized {
    pub generated_password: Option<String>,
}

/// Creates the data directory with the control database, the secrets key unless one is
/// configured, and the instance admin. There is no default password: without one given, a
/// random one is generated.
pub async fn init(args: InitArgs) -> Result<Initialized, Failure> {
    let email = identity::normalize_email(&args.admin_email);
    if !email.contains('@') {
        return Err(Failure::new(
            Exit::Usage,
            "usage",
            "--admin-email must be an email address",
        ));
    }
    if let Some(password) = &args.admin_password {
        let chars = password.chars().count();
        if !(PASSWORD_MIN_CHARS..=PASSWORD_MAX_CHARS).contains(&chars) {
            return Err(Failure::new(
                Exit::Usage,
                "usage",
                format!(
                    "the admin password needs {PASSWORD_MIN_CHARS} to {PASSWORD_MAX_CHARS} characters"
                ),
            ));
        }
    }

    let dir = &args.data_dir;
    prepare_directory(dir)?;
    let _lock = DataLock::acquire(dir)?;
    fs::write(dir.join(IN_PROGRESS_MARKER), b"").map_err(|e| Failure::storage(dir, e))?;

    // The key lives beside the database or outside the data directory, never in it.
    SecretKey::load_or_create(
        dir,
        args.secrets_key.as_deref(),
        args.secrets_key_file.as_deref(),
    )
    .map_err(|e| Failure::new(Exit::Usage, "secrets_key", e.to_string()))?;

    let generated_password = args
        .admin_password
        .is_none()
        .then(identity::generate_password);
    let password = args
        .admin_password
        .as_deref()
        .or(generated_password.as_deref())
        .unwrap_or_default();
    let name = email.split('@').next().unwrap_or_default().to_owned();

    let db = Db::open(dir).await.map_err(|e| Failure::storage(dir, e))?;
    let created = async {
        let mut tx = db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let hash = identity::hash_password(password);
        identity::create_user(&mut tx, &email, &name, &hash, true, now_ms()).await?;
        tx.commit().await
    }
    .await;
    db.close().await;
    created.map_err(|e| Failure::new(Exit::Internal, "internal", e.to_string()))?;

    fs::write(dir.join(DONE_MARKER), b"").map_err(|e| Failure::storage(dir, e))?;
    fs::remove_file(dir.join(IN_PROGRESS_MARKER)).map_err(|e| Failure::storage(dir, e))?;
    Ok(Initialized { generated_password })
}

/// Creates the directory, or clears the leftovers of an interrupted init.
fn prepare_directory(dir: &Path) -> Result<(), Failure> {
    if dir.join(DONE_MARKER).exists() {
        return Err(Failure::new(
            Exit::Refused,
            "already_initialized",
            dir.display().to_string(),
        ));
    }
    if dir.join(IN_PROGRESS_MARKER).exists() {
        // `secrets.key` stays: a key that was written once may already be backed up.
        for file in [CONTROL_DB_FILE, "control.sqlite-wal", "control.sqlite-shm"] {
            match fs::remove_file(dir.join(file)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    return Err(Failure::storage(dir, e));
                }
                _ => {}
            }
        }
        return Ok(());
    }
    match fs::read_dir(dir) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(Failure::new(
                    Exit::Refused,
                    "directory_not_empty",
                    dir.display().to_string(),
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dir).map_err(|e| Failure::storage(dir, e))
        }
        Err(e) => Err(Failure::storage(dir, e)),
    }
}

/// `serve` starts only on a directory init finished.
pub fn check_initialized(dir: &Path) -> Result<(), Failure> {
    if dir.join(DONE_MARKER).exists() {
        return Ok(());
    }
    let code = if dir.join(IN_PROGRESS_MARKER).exists() {
        "init_incomplete"
    } else {
        "not_initialized"
    };
    Err(Failure::new(
        Exit::Refused,
        code,
        format!(
            "{} is not an initialized data directory; run `v1per init`",
            dir.display()
        ),
    ))
}
