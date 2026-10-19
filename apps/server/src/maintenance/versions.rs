//! Schema versions: what this release runs on, what a data directory holds, and whether
//! `serve` may start on it. `serve` migrates only with `--migrate`; `v1per upgrade` takes a
//! backup first.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use v1per_protocol::operations::instance::SchemaVersions;
use v1per_storage::HEARTBEAT_SCHEMA;

use crate::db::{CONTROL_DB_FILE, MIGRATOR};
use crate::init::{Exit, Failure};

/// Written before the first migration of an upgrade and removed after the last. While it
/// exists the directory is half upgraded and `serve` refuses it.
pub const UPGRADE_MARKER: &str = "v1per.upgrade-in-progress";

/// The versions this binary runs on.
pub fn release() -> SchemaVersions {
    SchemaVersions {
        control: MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0),
        inbox: v1per_storage::inbox_schema(),
        heartbeats: HEARTBEAT_SCHEMA,
    }
}

fn pairs(data: SchemaVersions, release: SchemaVersions) -> [(&'static str, i64, i64); 3] {
    [
        (CONTROL_DB_FILE, data.control, release.control),
        (v1per_storage::INBOX_FILE, data.inbox, release.inbox),
        (
            v1per_storage::HEARTBEATS_FILE,
            data.heartbeats,
            release.heartbeats,
        ),
    ]
}

/// Which file of `data`, if any, is newer than `release`.
pub fn newer(data: SchemaVersions, release: SchemaVersions) -> Option<String> {
    pairs(data, release)
        .into_iter()
        .find(|(_, data, release)| data > release)
        .map(|(file, data, release)| {
            format!("{file} has schema {data}, this release runs {release}")
        })
}

pub fn older(data: SchemaVersions, release: SchemaVersions) -> bool {
    pairs(data, release)
        .into_iter()
        .any(|(_, data, release)| data < release)
}

/// Versions as stored in a directory no process holds. A file that does not exist yet counts
/// as current: the first `serve` creates it.
pub async fn stored(dir: &Path) -> Result<SchemaVersions, Failure> {
    let release = release();
    let storage = v1per_storage::stored_schemas(dir)
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    let control = dir.join(CONTROL_DB_FILE);
    let control = if control.exists() {
        let failed = |e: sqlx::Error| Failure::storage(&control, e);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&control)
                    .read_only(true),
            )
            .await
            .map_err(failed)?;
        // `init` migrates before it marks the directory, so the table exists.
        let version = sqlx::query_scalar(
            "SELECT coalesce(max(version), 0) FROM _sqlx_migrations WHERE success",
        )
        .fetch_one(&pool)
        .await;
        pool.close().await;
        version.map_err(failed)?
    } else {
        release.control
    };
    Ok(SchemaVersions {
        control,
        inbox: storage.inbox.unwrap_or(release.inbox),
        heartbeats: storage.heartbeats.unwrap_or(release.heartbeats),
    })
}

/// Whether `serve` may start on data at `stored`. Older data needs `migrate`, a half-done
/// upgrade needs `v1per upgrade` again, newer data a newer release. All exit 3.
pub fn check(
    stored: SchemaVersions,
    release: SchemaVersions,
    upgrading: bool,
    migrate: bool,
) -> Result<(), Failure> {
    if let Some(newer) = newer(stored, release) {
        return Err(Failure::new(
            Exit::Refused,
            "data_newer_than_binary",
            format!("{newer}; install the matching or a newer release"),
        ));
    }
    if upgrading {
        return Err(Failure::new(
            Exit::Refused,
            "upgrade_interrupted",
            "an upgrade stopped before it finished; run `v1per upgrade` again to resume it",
        ));
    }
    if older(stored, release) && !migrate {
        return Err(Failure::new(
            Exit::Refused,
            "upgrade_required",
            "the data is older than this release; run `v1per upgrade --backup-to <file>`, or start with --migrate to upgrade without a backup",
        ));
    }
    Ok(())
}

/// [`check`] on a data directory no process holds.
pub async fn check_dir(dir: &Path, migrate: bool) -> Result<(), Failure> {
    check(
        stored(dir).await?,
        release(),
        dir.join(UPGRADE_MARKER).exists(),
        migrate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: SchemaVersions = SchemaVersions {
        control: 10,
        inbox: 1,
        heartbeats: 1,
    };

    fn code(stored: SchemaVersions, upgrading: bool, migrate: bool) -> Option<String> {
        check(stored, RELEASE, upgrading, migrate).err().map(|f| {
            assert_eq!(f.exit, Exit::Refused);
            f.code.into_owned()
        })
    }

    #[test]
    fn serve_starts_on_current_data_or_migrates_when_told() {
        assert_eq!(code(RELEASE, false, false), None);
        let older = SchemaVersions {
            control: 9,
            ..RELEASE
        };
        assert_eq!(
            code(older, false, false).as_deref(),
            Some("upgrade_required")
        );
        assert_eq!(code(older, false, true), None);
        let newer = SchemaVersions {
            heartbeats: 2,
            ..RELEASE
        };
        assert_eq!(
            code(newer, false, true).as_deref(),
            Some("data_newer_than_binary")
        );
        assert_eq!(
            code(RELEASE, true, true).as_deref(),
            Some("upgrade_interrupted")
        );
    }
}
