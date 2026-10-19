use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rusqlite::Connection;
use tempfile::TempDir;
use v1per_kuma_fixture::{
    API_KEYS, STATUS_PAGE_PASSWORD, USER_NAME, USER_PASSWORD, USER_TOTP_SECRET, generate, manifest,
    migration_names, open_immutable, schema_statements,
};

/// One generated database shared by the tests that only read it.
fn fixture() -> &'static Path {
    static FIXTURE: OnceLock<(TempDir, PathBuf)> = OnceLock::new();
    let (_, path) = FIXTURE.get_or_init(|| {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("kuma.db");
        generate(&path).expect("generate fixture");
        (dir, path)
    });
    path
}

fn open() -> Connection {
    open_immutable(fixture()).expect("open fixture")
}

#[test]
fn two_runs_write_identical_files() {
    let dir = TempDir::new().expect("temp dir");
    let second = dir.path().join("kuma.db");
    generate(&second).expect("generate second fixture");

    let first = std::fs::read(fixture()).expect("read first");
    assert!(
        first == std::fs::read(&second).expect("read second"),
        "fixture files differ"
    );
    assert!(
        !dir.path().join("kuma.db-wal").exists(),
        "WAL file left behind"
    );
}

#[test]
fn counts_match_the_committed_manifest() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/kuma-2.5.0/manifest.json"))
            .expect("manifest JSON");
    let actual = manifest(fixture()).expect("manifest");
    assert_eq!(
        actual, expected,
        "regenerate with `cargo run -p v1per-kuma-fixture -- --force --manifest fixtures/kuma-2.5.0/manifest.json <out.db>`"
    );
}

#[test]
fn schema_matches_schema_sql() {
    let conn = open();
    let stored: BTreeSet<String> = conn
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
        .and_then(|mut stmt| stmt.query_map([], |row| row.get(0))?.collect())
        .expect("read sqlite_master");
    let expected: BTreeSet<String> = schema_statements().into_iter().collect();
    assert_eq!(stored, expected);

    let applied: Vec<String> = conn
        .prepare("SELECT name FROM knex_migrations ORDER BY id")
        .and_then(|mut stmt| stmt.query_map([], |row| row.get(0))?.collect())
        .expect("read knex_migrations");
    assert_eq!(applied.len(), 54);
    assert_eq!(applied, migration_names());
    assert_eq!(
        applied.last().map(String::as_str),
        Some("2026-07-22-0000-fix-stat-daily-overflow.js")
    );
}

#[test]
fn documented_credentials_match_the_stored_hashes() {
    let conn = open();
    let (password, totp): (String, String) = conn
        .query_row(
            "SELECT password, twofa_secret FROM user WHERE username = ?1",
            [USER_NAME],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("fixture user");
    assert!(password.starts_with("$2a$10$"));
    assert!(bcrypt::verify(USER_PASSWORD, &password).expect("bcrypt hash"));
    assert_eq!(totp, USER_TOTP_SECRET);

    let keys: Vec<String> = conn
        .prepare("SELECT key FROM api_key ORDER BY id")
        .and_then(|mut stmt| stmt.query_map([], |row| row.get(0))?.collect())
        .expect("api keys");
    for (hash, clear) in keys.iter().zip(API_KEYS) {
        assert!(bcrypt::verify(clear, hash).expect("bcrypt hash"));
    }

    let page_password: String = conn
        .query_row(
            "SELECT password FROM status_page WHERE slug = 'client-acme'",
            [],
            |row| row.get(0),
        )
        .expect("password page");
    assert_eq!(page_password, STATUS_PAGE_PASSWORD);
}
