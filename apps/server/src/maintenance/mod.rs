//! Operations and recovery (milestone 11, decision 0010): backup and restore, schema upgrade,
//! retention and diagnostics.
//!
//! Work that needs the engines runs as a job in `maintenance_jobs`. A running server holds
//! DuckDB, so `v1per backup`, `retention` and `diagnostics` queue a job for it and wait for the
//! outcome; without a server the command opens the engines itself and runs the queue. Restore
//! and upgrade work on a directory no server holds.

pub mod backup;
pub mod commands;
pub mod diagnostics;
pub mod jobs;
pub mod restore;
pub mod retention;
pub mod upgrade;
pub mod versions;

use std::io;
use std::path::Path;

use crate::init::{Exit, Failure};

/// A failed write or read. A full volume says so, since the operator's next step differs.
pub(crate) fn io_failure(path: &Path, error: io::Error) -> Failure {
    if error.kind() == io::ErrorKind::StorageFull {
        return Failure::new(
            Exit::StorageUnavailable,
            "disk_full",
            format!(
                "{}: {error}; free space and run the command again",
                path.display()
            ),
        );
    }
    Failure::storage(path, error)
}

/// A heartbeat storage error, as [`io_failure`] reports it.
pub(crate) fn storage_failure(path: &Path, error: v1per_storage::Error) -> Failure {
    match error {
        v1per_storage::Error::Io(error) => io_failure(path, error),
        error => Failure::storage(path, error),
    }
}

pub(crate) fn internal(error: impl std::fmt::Display) -> Failure {
    Failure::new(Exit::Internal, "internal", error.to_string())
}
