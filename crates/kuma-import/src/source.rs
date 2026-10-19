//! Opening a Kuma database read-only and checking that it is exactly Kuma 2.5.0.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode, OpenFlags};

/// `.schema` of a Kuma 2.5.0 database plus its knex migration names, shared with the fixture.
const SCHEMA_SQL: &str = include_str!("../../../fixtures/kuma-2.5.0/schema.sql");

/// How the source was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadMode {
    /// `immutable=1`: no locks, no side files, the WAL is ignored. Used when no WAL with
    /// content sits next to the file, which is the case for a cleanly stopped Kuma.
    Immutable,
    /// `mode=ro` with locking, so committed WAL content is read. SQLite may create the
    /// `-shm` index next to the file; the database and its WAL are never written.
    ReadOnlyWal,
}

/// Why a file is refused. Every message says what to do next.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0} is not a SQLite database")]
    NotSqlite(PathBuf),
    #[error("{0} is not an Uptime Kuma database: it has no `monitor` table")]
    NotKuma(PathBuf),
    #[error(
        "Kuma 1.x found (no knex migrations); v1per imports exactly Kuma 2.5.0. Upgrade Kuma to 2.5.0, start it once so it migrates the database, stop it, then import."
    )]
    Kuma1,
    #[error(
        "Kuma older than 2.5.0 found: {missing} of {expected} migrations are not applied, the latest applied is `{latest}`. v1per imports exactly 2.5.0. Upgrade Kuma to 2.5.0, start it once, stop it, then import."
    )]
    Older {
        missing: usize,
        expected: usize,
        latest: String,
    },
    #[error(
        "Kuma newer than 2.5.0 found: {} migration(s) v1per does not know, the newest is `{}`. v1per imports exactly 2.5.0 and Kuma does not support downgrades; import with a v1per release that supports this Kuma version.",
        unknown.len(),
        unknown.last().map_or("", String::as_str)
    )]
    Newer { unknown: Vec<String> },
    #[error(
        "the knex migrations differ from Kuma 2.5.0 (missing {missing:?}, unknown {unknown:?}). This looks like a fork or a hand-edited database; v1per imports only an unmodified Kuma 2.5.0."
    )]
    Migrations {
        missing: Vec<String>,
        unknown: Vec<String>,
    },
    #[error(
        "the schema differs from Kuma 2.5.0, missing: {}. Restore an unmodified Kuma 2.5.0 database and import that.",
        .0.join(", ")
    )]
    Schema(Vec<String>),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Opens `path` read-only and verifies the 2.5.0 signature.
pub(crate) fn open(path: &Path) -> Result<(Connection, ReadMode), OpenError> {
    std::fs::metadata(path).map_err(|source| OpenError::Io {
        path: path.to_owned(),
        source,
    })?;
    let wal = sibling(path, "-wal");
    let mode = match std::fs::metadata(&wal) {
        Ok(meta) if meta.len() > 0 => ReadMode::ReadOnlyWal,
        _ => ReadMode::Immutable,
    };
    let query = match mode {
        ReadMode::Immutable => "immutable=1",
        ReadMode::ReadOnlyWal => "mode=ro",
    };
    let uri = format!("file:{}?{query}", uri_path(path));
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // Belt and braces: even a bug in a query cannot write.
    let checked = conn
        .pragma_update(None, "query_only", true)
        .map_err(OpenError::from)
        .and_then(|()| verify(&conn, path));
    match checked {
        Ok(()) => Ok((conn, mode)),
        Err(OpenError::Sqlite(rusqlite::Error::SqliteFailure(error, _)))
            if error.code == ErrorCode::NotADatabase =>
        {
            Err(OpenError::NotSqlite(path.to_owned()))
        }
        Err(error) => Err(error),
    }
}

fn verify(conn: &Connection, path: &Path) -> Result<(), OpenError> {
    let tables = table_names(conn)?;
    if !tables.contains("monitor") {
        return Err(OpenError::NotKuma(path.to_owned()));
    }
    if !tables.contains("knex_migrations") {
        return Err(OpenError::Kuma1);
    }

    let applied: Vec<String> = conn
        .prepare("SELECT name FROM knex_migrations ORDER BY id")?
        .query_map([], |row| row.get::<_, Option<String>>(0))?
        .filter_map(Result::transpose)
        .collect::<rusqlite::Result<_>>()?;
    let expected = migration_names();
    let applied_set: BTreeSet<&str> = applied.iter().map(String::as_str).collect();
    let expected_set: BTreeSet<&str> = expected.iter().copied().collect();
    let missing: Vec<String> = expected
        .iter()
        .filter(|name| !applied_set.contains(*name))
        .map(|name| (*name).to_owned())
        .collect();
    let mut unknown: Vec<String> = applied_set
        .difference(&expected_set)
        .map(|name| (*name).to_owned())
        .collect();
    unknown.sort();
    match (missing.is_empty(), unknown.is_empty()) {
        (true, true) => {}
        (false, true) => {
            return Err(match applied_set.iter().max() {
                Some(latest) => OpenError::Older {
                    missing: missing.len(),
                    expected: expected.len(),
                    latest: (*latest).to_owned(),
                },
                None => OpenError::Kuma1,
            });
        }
        (true, false) => return Err(OpenError::Newer { unknown }),
        (false, false) => return Err(OpenError::Migrations { missing, unknown }),
    }

    let missing = missing_columns(conn)?;
    if missing.is_empty() {
        Ok(())
    } else {
        Err(OpenError::Schema(missing))
    }
}

/// Tables and columns of the 2.5.0 schema the source lacks, as `table` or `table.column`.
/// Matched by name: column order differs between upgrade paths.
fn missing_columns(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let reference = Connection::open_in_memory()?;
    for statement in statements() {
        if !statement.starts_with("CREATE TABLE sqlite_") {
            reference.execute_batch(&statement)?;
        }
    }
    let mut missing = Vec::new();
    for table in table_names(&reference)? {
        let have = columns(conn, &table)?;
        if have.is_empty() {
            missing.push(table);
            continue;
        }
        for column in columns(&reference, &table)? {
            if !have.contains(&column) {
                missing.push(format!("{table}.{column}"));
            }
        }
    }
    Ok(missing)
}

fn table_names(conn: &Connection) -> rusqlite::Result<BTreeSet<String>> {
    conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
    )?
    .query_map([], |row| row.get(0))?
    .collect()
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<BTreeSet<String>> {
    conn.prepare("SELECT name FROM pragma_table_info(?1)")?
        .query_map([table], |row| row.get(0))?
        .collect()
}

/// Kuma 2.5.0's knex migration names in filename order.
pub(crate) fn migration_names() -> Vec<&'static str> {
    SCHEMA_SQL
        .lines()
        .filter_map(|line| line.strip_prefix("-- migration: "))
        .map(str::trim_end)
        .collect()
}

/// The CREATE statements of the reference schema.
fn statements() -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    for line in SCHEMA_SQL.lines().filter(|line| !line.starts_with("--")) {
        if current.is_empty() && line.trim().is_empty() {
            continue;
        }
        current.push_str(line);
        current.push('\n');
        if line.trim_end().ends_with(';') {
            statements.push(std::mem::take(&mut current));
        }
    }
    statements
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The path as SQLite's URI parser reads it back: `%`, `?` and `#` are escaped.
fn uri_path(path: &Path) -> String {
    let mut out = String::new();
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3f"),
            '#' => out.push_str("%23"),
            c => out.push(c),
        }
    }
    out
}
