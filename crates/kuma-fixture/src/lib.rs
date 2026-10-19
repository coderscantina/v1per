//! Deterministic synthetic Uptime Kuma 2.5.0 SQLite database.
//!
//! [`generate`] builds the database from `fixtures/kuma-2.5.0/schema.sql` with a fixed seed and
//! a fixed clock, so two runs with the same build produce byte-identical files. [`manifest`]
//! describes a generated file; `fixtures/kuma-2.5.0/manifest.json` is its committed output.

mod catalog;
mod history;
mod rng;
mod schema;

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;
use serde_json::{Map, Value, json};

pub use schema::{SCHEMA_SQL, migration_names, schema_statements};

/// Kuma release and commit the fixture imitates.
pub const KUMA_VERSION: &str = "2.5.0";
pub const KUMA_COMMIT: &str = "d9a60dfc73140d15111752e4e8910ed4b54bd9a3";

/// Seed of every pseudo-random choice.
pub const SEED: u64 = 0x5631_7065_725f_6b75;

/// Days of synthetic history before [`now`].
pub const HISTORY_DAYS: i64 = 400;

/// `keepDataPeriodDays` setting. Lower than [`HISTORY_DAYS`] on purpose: the oldest 35 days
/// lie outside the retention window, as if the setting was lowered after the last cleanup.
pub const KEEP_DATA_PERIOD_DAYS: i64 = 365;

/// Login of the fixture user. The password is stored as a bcrypt hash (cost 10).
pub const USER_NAME: &str = "fixture-admin";
pub const USER_PASSWORD: &str = "fixture-admin-password";

/// TOTP key of the fixture user. Kuma uses the ASCII bytes of `twofa_secret` as the HMAC key.
pub const USER_TOTP_SECRET: &str =
    "FixtureTotpSecretNotRealFixtureTotpSecretNotRealFixtureTotp00001";

/// Clear API keys. Kuma shows them as `uk<id>_<key>` and stores a bcrypt hash of `<key>`.
pub const API_KEYS: [&str; 2] = [
    "fixtureApiKeyNotReal0000000000000000001",
    "fixtureApiKeyNotReal0000000000000000002",
];

/// Plain-text password of the `client-acme` status page.
pub const STATUS_PAGE_PASSWORD: &str = "fixture-page-password";

/// The fixed clock: the moment the fixture database was "taken".
pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0)
        .single()
        .expect("fixed fixture clock is a valid UTC time")
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} already exists")]
    Exists(PathBuf),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("bcrypt: {0}")]
    Bcrypt(#[from] bcrypt::BcryptError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Write the fixture database to `path`, which must not exist yet.
pub fn generate(path: &Path) -> Result<()> {
    if path.exists() {
        return Err(Error::Exists(path.to_owned()));
    }

    let mut conn = Connection::open(path)?;
    // Same file format as a Kuma database: 4 KiB pages, incremental auto vacuum, WAL.
    conn.execute_batch(
        "PRAGMA page_size = 4096;
         PRAGMA auto_vacuum = INCREMENTAL;
         PRAGMA journal_mode = MEMORY;
         PRAGMA synchronous = OFF;",
    )?;

    let tx = conn.transaction()?;
    schema::apply(&tx)?;
    let ids = catalog::insert(&tx)?;
    history::insert(&tx, &ids)?;
    tx.commit()?;

    // Kuma's daily retention job runs `PRAGMA optimize`, which leaves `sqlite_stat1` behind.
    // The bundled SQLite also writes `sqlite_stat4`, which Kuma's build does not have.
    conn.execute_batch(
        "ANALYZE;
         DROP TABLE IF EXISTS sqlite_stat4;
         PRAGMA incremental_vacuum;",
    )?;
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    conn.close().map_err(|(_, e)| e)?;
    Ok(())
}

/// Open a database read-only without locks or WAL side files, the way the importer reads a
/// copy of a stopped Kuma.
pub fn open_immutable(path: &Path) -> Result<Connection> {
    use rusqlite::OpenFlags;
    let uri = format!("file:{}?immutable=1", path.display());
    Ok(Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?)
}

/// Describe a generated database: source pins, documented credentials and row counts.
pub fn manifest(path: &Path) -> Result<Value> {
    let conn = open_immutable(path)?;

    let mut tables = Map::new();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for name in names {
        let count: i64 = conn.query_row(&format!("SELECT count(*) FROM \"{name}\""), [], |r| {
            r.get(0)
        })?;
        tables.insert(name, count.into());
    }

    let grouped = |sql: &str| -> Result<Value> {
        let mut map = Map::new();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (key, count) = row?;
            map.insert(key, count.into());
        }
        Ok(Value::Object(map))
    };

    Ok(json!({
        "kuma": { "version": KUMA_VERSION, "commit": KUMA_COMMIT },
        "generator": {
            "crate": "v1per-kuma-fixture",
            "seed": format!("{SEED:#x}"),
            "now": now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "historyDays": HISTORY_DAYS,
            "keepDataPeriodDays": KEEP_DATA_PERIOD_DAYS,
        },
        "credentials": {
            "username": USER_NAME,
            "password": USER_PASSWORD,
            "totpSecret": USER_TOTP_SECRET,
            "apiKeys": API_KEYS.iter().enumerate().map(|(i, key)| format!("uk{}_{key}", i + 1)).collect::<Vec<_>>(),
            "statusPagePassword": STATUS_PAGE_PASSWORD,
        },
        "counts": {
            "tables": tables,
            "monitorsByType": grouped("SELECT type, count(*) FROM monitor GROUP BY type ORDER BY type")?,
            "notificationsByType": grouped(
                "SELECT json_extract(config, '$.type'), count(*) FROM notification GROUP BY 1 ORDER BY 1",
            )?,
            "maintenanceByStrategy": grouped(
                "SELECT strategy, count(*) FROM maintenance GROUP BY strategy ORDER BY strategy",
            )?,
            "heartbeatsByStatus": grouped(
                "SELECT CAST(status AS TEXT), count(*) FROM heartbeat GROUP BY status ORDER BY status",
            )?,
            "heartbeatsByMonitor": grouped(
                "SELECT m.name, count(*) FROM heartbeat h JOIN monitor m ON m.id = h.monitor_id
                 GROUP BY m.id ORDER BY m.id",
            )?,
            "heartbeatsOutsideRetention": conn.query_row(
                "SELECT count(*) FROM heartbeat WHERE time < ?1",
                [history::sql_time_ms((now() - chrono::Duration::days(KEEP_DATA_PERIOD_DAYS)).timestamp_millis())],
                |r| r.get::<_, i64>(0),
            )?,
        },
    }))
}
