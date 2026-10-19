//! Kuma rows read by column name. SQLite is dynamically typed and Kuma wrote some columns
//! through knex defaults like `'0'`, so the accessors convert leniently.

use std::collections::BTreeMap;
use std::fmt;

use rusqlite::Connection;
use rusqlite::types::Value;
use serde_json::{Map, Value as Json};

/// One row, keyed by column name.
pub(crate) struct Record {
    table: &'static str,
    columns: BTreeMap<String, Value>,
}

/// Every row of `table` in primary key order.
pub(crate) fn read_table(conn: &Connection, table: &'static str) -> rusqlite::Result<Vec<Record>> {
    let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY id"))?;
    let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    stmt.query_map([], |row| {
        let columns = names
            .iter()
            .enumerate()
            .map(|(i, name)| Ok((name.clone(), row.get::<_, Value>(i)?)))
            .collect::<rusqlite::Result<_>>()?;
        Ok(Record { table, columns })
    })?
    .collect()
}

impl Record {
    pub fn table(&self) -> &'static str {
        self.table
    }

    pub fn id(&self) -> i64 {
        self.int("id").unwrap_or_default()
    }

    /// The value as text; numbers are formatted. `None` for NULL.
    pub fn text(&self, column: &str) -> Option<String> {
        match self.columns.get(column)? {
            Value::Text(text) => Some(text.clone()),
            Value::Integer(n) => Some(n.to_string()),
            Value::Real(n) => Some(n.to_string()),
            Value::Blob(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            Value::Null => None,
        }
    }

    /// Text that is not empty or whitespace.
    pub fn filled(&self, column: &str) -> Option<String> {
        self.text(column).filter(|text| !text.trim().is_empty())
    }

    pub fn int(&self, column: &str) -> Option<i64> {
        match self.columns.get(column)? {
            Value::Integer(n) => Some(*n),
            Value::Real(n) if n.fract() == 0.0 => Some(*n as i64),
            Value::Text(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn real(&self, column: &str) -> Option<f64> {
        match self.columns.get(column)? {
            Value::Integer(n) => Some(*n as f64),
            Value::Real(n) => Some(*n),
            Value::Text(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    /// Kuma booleans: 0/1, or `'true'`/`'false'` from older code paths.
    pub fn flag(&self, column: &str) -> bool {
        match self.columns.get(column) {
            Some(Value::Integer(n)) => *n != 0,
            Some(Value::Real(n)) => *n != 0.0,
            Some(Value::Text(text)) => matches!(text.trim(), "1" | "true"),
            _ => false,
        }
    }

    /// The whole row, for the inventory.
    pub fn source(&self) -> SourceRecord {
        let columns = self
            .columns
            .iter()
            .filter(|(_, value)| !matches!(value, Value::Null))
            .map(|(name, value)| (name.clone(), json(value)))
            .collect();
        SourceRecord {
            table: self.table,
            id: self.id(),
            columns,
        }
    }

    /// Only the named columns of the row, for inventory entries about part of a row.
    pub fn source_of(&self, names: &[&str]) -> SourceRecord {
        let mut source = self.source();
        source
            .columns
            .retain(|name, _| name == "id" || names.contains(&name.as_str()));
        source
    }
}

fn json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Integer(n) => Json::from(*n),
        Value::Real(n) => serde_json::Number::from_f64(*n).map_or(Json::Null, Json::Number),
        Value::Text(text) => Json::String(text.clone()),
        Value::Blob(bytes) => Json::String(String::from_utf8_lossy(bytes).into_owned()),
    }
}

/// A Kuma row as stored, non-null columns only. Kuma keeps credentials in plain text, so the
/// server stores this sealed and `Debug` names the columns, never their values.
#[derive(Clone, PartialEq)]
pub struct SourceRecord {
    table: &'static str,
    id: i64,
    columns: Map<String, Json>,
}

impl SourceRecord {
    pub fn table(&self) -> &'static str {
        self.table
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    /// The column values. Seal them before storing.
    pub fn expose(&self) -> &Map<String, Json> {
        &self.columns
    }
}

impl fmt::Debug for SourceRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceRecord")
            .field("table", &self.table)
            .field("id", &self.id)
            .field("columns", &self.columns.keys().collect::<Vec<_>>())
            .finish()
    }
}
