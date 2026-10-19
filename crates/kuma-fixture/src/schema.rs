use rusqlite::Connection;

/// `fixtures/kuma-2.5.0/schema.sql`: the `.schema` of a Kuma 2.5.0 database plus the knex
/// migration names as `-- migration: <name>` comment lines.
pub const SCHEMA_SQL: &str = include_str!("../../../fixtures/kuma-2.5.0/schema.sql");

/// The CREATE statements of [`SCHEMA_SQL`] without their trailing `;`, in file order. This is
/// the text SQLite stores in `sqlite_master.sql`.
pub fn schema_statements() -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    // Split on `\n` only: four statements from Kuma's original template keep CRLF line
    // endings, and SQLite stores them byte for byte.
    for line in SCHEMA_SQL
        .split('\n')
        .filter(|line| !line.starts_with("--"))
    {
        if current.is_empty() && line.trim().is_empty() {
            continue;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        if let Some(statement) = current.strip_suffix(';') {
            statements.push(statement.to_owned());
            current.clear();
        }
    }
    statements
}

/// Knex migration names of Kuma 2.5.0 in filename order, as a fresh install applies them.
pub fn migration_names() -> Vec<&'static str> {
    SCHEMA_SQL
        .lines()
        .filter_map(|line| line.strip_prefix("-- migration: "))
        .collect()
}

/// Create every table and index. SQLite creates `sqlite_sequence` itself and `ANALYZE`
/// creates `sqlite_stat1`, so their statements are skipped.
pub(crate) fn apply(conn: &Connection) -> rusqlite::Result<()> {
    for statement in schema_statements() {
        if !statement.starts_with("CREATE TABLE sqlite_") {
            conn.execute_batch(&statement)?;
        }
    }
    Ok(())
}
