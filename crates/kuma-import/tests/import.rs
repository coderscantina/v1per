use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::TimeDelta;
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;
use v1per_checks::{Check, HttpAuth};
use v1per_kuma_fixture::{
    KEEP_DATA_PERIOD_DAYS, STATUS_PAGE_PASSWORD, USER_TOTP_SECRET, generate, migration_names, now,
    schema_statements,
};
use v1per_kuma_import::{
    ChannelState, InventoryKind, MonitorId, MonitorKind, OpenError, ReadMode, RunState, Source,
    StatTable, Status,
};
use v1per_maintenance_schedule::Strategy;

/// One generated fixture shared by the tests that only read it.
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

fn manifest() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/kuma-2.5.0/manifest.json"))
        .expect("manifest JSON")
}

fn counts(manifest: &Value, key: &str) -> BTreeMap<String, u64> {
    manifest["counts"][key]
        .as_object()
        .expect("count map")
        .iter()
        .map(|(name, count)| (name.clone(), count.as_u64().expect("count")))
        .collect()
}

fn tally<'a>(keys: impl Iterator<Item = &'a str>) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    for key in keys {
        *map.entry(key.to_owned()).or_default() += 1;
    }
    map
}

/// An empty Kuma 2.5.0 schema with the given migrations recorded.
fn kuma_schema(path: &Path, migrations: &[&str]) -> Connection {
    let conn = Connection::open(path).expect("create database");
    for statement in schema_statements() {
        if !statement.starts_with("CREATE TABLE sqlite_") {
            conn.execute_batch(&statement).expect("schema statement");
        }
    }
    for name in migrations {
        conn.execute(
            "INSERT INTO knex_migrations (name, batch, migration_time) VALUES (?1, 1, 0)",
            [name],
        )
        .expect("migration row");
    }
    conn
}

#[test]
fn refuses_files_that_are_not_kuma() {
    let dir = TempDir::new().expect("temp dir");
    let text = dir.path().join("notes.txt");
    std::fs::write(&text, "not a database, just some text that is long enough").expect("write");
    assert!(matches!(Source::open(&text), Err(OpenError::NotSqlite(_))));

    let other = dir.path().join("other.db");
    Connection::open(&other)
        .and_then(|conn| conn.execute_batch("CREATE TABLE note (id INTEGER PRIMARY KEY)"))
        .expect("other database");
    assert!(matches!(Source::open(&other), Err(OpenError::NotKuma(_))));

    let kuma1 = dir.path().join("kuma1.db");
    Connection::open(&kuma1)
        .and_then(|conn| conn.execute_batch("CREATE TABLE monitor (id INTEGER PRIMARY KEY)"))
        .expect("kuma 1 database");
    assert!(matches!(Source::open(&kuma1), Err(OpenError::Kuma1)));

    assert!(matches!(
        Source::open(dir.path().join("missing.db")),
        Err(OpenError::Io { .. })
    ));
}

#[test]
fn refuses_other_migration_sets() {
    let dir = TempDir::new().expect("temp dir");
    let all = migration_names();
    let open = |name: &str, migrations: &[&str]| {
        let path = dir.path().join(name);
        drop(kuma_schema(&path, migrations));
        Source::open(&path).err().expect("refused")
    };

    match open("older.db", &all[..50]) {
        OpenError::Older {
            missing, latest, ..
        } => {
            assert_eq!(missing, 4);
            assert_eq!(latest, all[49]);
        }
        other => panic!("expected Older, got {other}"),
    }

    let mut newer = all.clone();
    newer.push("2026-09-01-0000-add-something.js");
    let error = open("newer.db", &newer);
    assert!(matches!(error, OpenError::Newer { .. }));
    let message = error.to_string();
    assert!(message.contains("2026-09-01-0000-add-something.js"));
    assert!(message.contains("does not support downgrades"));

    let mut fork = all[..53].to_vec();
    fork.push("2026-08-01-0000-fork.js");
    assert!(matches!(
        open("fork.db", &fork),
        OpenError::Migrations { missing, unknown } if missing.len() == 1 && unknown.len() == 1
    ));
}

#[test]
fn refuses_a_missing_column() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("kuma.db");
    kuma_schema(&path, &migration_names())
        .execute_batch("ALTER TABLE monitor DROP COLUMN bearer_token")
        .expect("drop column");
    match Source::open(&path) {
        Err(OpenError::Schema(missing)) => assert_eq!(missing, ["monitor.bearer_token"]),
        other => panic!("expected a schema refusal, got {:?}", other.err()),
    }
}

#[test]
fn reads_committed_wal_content() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("kuma.db");
    let writer = kuma_schema(&path, &migration_names());
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA wal_autocheckpoint = 0;
             INSERT INTO monitor (name, type, interval, url) VALUES ('Late', 'http', 60, 'https://example.com');",
        )
        .expect("write into the WAL");
    let wal = dir.path().join("kuma.db-wal");
    assert!(std::fs::metadata(&wal).expect("wal").len() > 0);
    let wal_before = std::fs::read(&wal).expect("read wal");

    let source = Source::open(&path).expect("open");
    assert_eq!(source.read_mode(), ReadMode::ReadOnlyWal);
    let plan = source.plan().expect("plan");
    assert_eq!(plan.monitors.len(), 1);
    assert_eq!(plan.monitors[0].name, "Late");
    drop(source);
    assert_eq!(std::fs::read(&wal).expect("read wal"), wal_before);
    drop(writer);
}

#[test]
fn plan_accounts_for_every_fixture_row() {
    let manifest = manifest();
    let source = Source::open(fixture()).expect("open fixture");
    assert_eq!(source.read_mode(), ReadMode::Immutable);
    let plan = source.plan().expect("plan");
    let since = (now() - TimeDelta::days(KEEP_DATA_PERIOD_DAYS)).timestamp_millis();
    let report = source.report(&plan, since).expect("report");

    let tables: BTreeMap<String, u64> = report
        .tables
        .iter()
        .map(|table| (table.table.clone(), table.rows))
        .collect();
    assert_eq!(tables, counts(&manifest, "tables"));
    for table in &report.tables {
        if let Some(accounted) = table.accounted {
            assert_eq!(
                accounted, table.rows,
                "rows of {} not accounted for",
                table.table
            );
        }
    }

    assert_eq!(
        tally(plan.monitors.iter().map(|m| m.kuma_type.as_str())),
        counts(&manifest, "monitorsByType")
    );
    assert_eq!(
        tally(plan.channels.iter().map(|c| c.provider.as_str())),
        counts(&manifest, "notificationsByType")
    );
    let strategy = |strategy| match strategy {
        Strategy::Manual => "manual",
        Strategy::Single => "single",
        Strategy::RecurringInterval => "recurring-interval",
        Strategy::RecurringWeekday => "recurring-weekday",
        Strategy::RecurringDayOfMonth => "recurring-day-of-month",
        Strategy::Cron => "cron",
    };
    assert_eq!(
        tally(
            plan.maintenance
                .iter()
                .map(|m| strategy(m.definition.strategy))
        ),
        counts(&manifest, "maintenanceByStrategy")
    );

    let outside = manifest["counts"]["heartbeatsOutsideRetention"]
        .as_u64()
        .expect("count");
    assert_eq!(report.heartbeats.outside, outside);
    assert_eq!(report.heartbeats.inside, tables["heartbeat"] - outside);
}

#[test]
fn streams_history_matching_the_fixture() {
    let manifest = manifest();
    let source = Source::open(fixture()).expect("open fixture");
    let plan = source.plan().expect("plan");
    let names: BTreeMap<MonitorId, &str> = plan
        .monitors
        .iter()
        .map(|m| (m.id, m.name.as_str()))
        .collect();

    let chunks: Vec<_> = source
        .heartbeats(i64::MIN, None, 50_000)
        .collect::<Result<_, _>>()
        .expect("heartbeats");
    let beats = || chunks.iter().flat_map(|chunk| &chunk.heartbeats);
    assert_eq!(
        tally(beats().map(|b| names[&b.monitor])),
        counts(&manifest, "heartbeatsByMonitor")
    );
    let status = |status| match status {
        Status::Down => "0",
        Status::Up => "1",
        Status::Pending => "2",
        Status::Maintenance => "3",
    };
    assert_eq!(
        tally(beats().map(|b| status(b.status))),
        counts(&manifest, "heartbeatsByStatus")
    );

    let resumed: usize = source
        .heartbeats(i64::MIN, Some(chunks[0].last_id), 50_000)
        .map(|chunk| chunk.expect("chunk").heartbeats.len())
        .sum();
    assert_eq!(resumed + chunks[0].heartbeats.len(), beats().count());

    // The newest beat per monitor, with the last important one as its change time.
    let mut newest: BTreeMap<MonitorId, &v1per_kuma_import::KumaHeartbeat> = BTreeMap::new();
    for beat in beats() {
        let slot = newest.entry(beat.monitor).or_insert(beat);
        if (beat.time_ms, beat.id) > (slot.time_ms, slot.id) {
            *slot = beat;
        }
    }
    let latest = source.latest_heartbeats().expect("latest heartbeats");
    assert_eq!(latest.len(), newest.len());
    for (monitor, beat) in &newest {
        let seeded = &latest[monitor];
        assert_eq!(&&seeded.heartbeat, beat);
        assert!(seeded.changed_ms <= beat.time_ms);
    }

    let since = (now() - TimeDelta::days(KEEP_DATA_PERIOD_DAYS)).timestamp_millis();
    let inside: usize = source
        .heartbeats(since, None, 50_000)
        .map(|chunk| chunk.expect("chunk").heartbeats.len())
        .sum();
    let outside = manifest["counts"]["heartbeatsOutsideRetention"]
        .as_u64()
        .expect("count");
    assert_eq!(inside as u64, beats().count() as u64 - outside);

    let tables = counts(&manifest, "tables");
    for table in StatTable::ALL {
        let chunks: Vec<_> = source
            .stats(table, None, 500)
            .collect::<Result<_, _>>()
            .expect("stats");
        let rows: usize = chunks.iter().map(|chunk| chunk.rows.len()).sum();
        assert_eq!(rows as u64, tables[table.name()], "{}", table.name());
        for pair in chunks.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(
                (a.monitor, a.rows.last().map(|r| r.bucket_ms))
                    < (b.monitor, b.rows.first().map(|r| r.bucket_ms)),
                "chunks overlap or go backwards"
            );
        }
        let resumed: usize = source
            .stats(table, Some(chunks[0].cursor), 500)
            .map(|chunk| chunk.expect("chunk").rows.len())
            .sum();
        assert_eq!(resumed + chunks[0].rows.len(), rows);
    }
}

#[test]
fn maps_the_fixture_configuration() {
    let source = Source::open(fixture()).expect("open fixture");
    let plan = source.plan().expect("plan");
    let monitor = |name: &str| {
        plan.monitors
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("monitor {name}"))
    };

    let node_names: Vec<(&str, Option<&str>)> = plan
        .nodes
        .iter()
        .map(|node| {
            let parent = node.parent.map(|id| {
                plan.monitors
                    .iter()
                    .find(|m| m.id == id)
                    .expect("parent")
                    .name
                    .as_str()
            });
            (node.name.as_str(), parent)
        })
        .collect();
    assert_eq!(
        node_names,
        [
            ("Production", None),
            ("Staging", None),
            ("Empty group", None),
            ("EU", Some("Production")),
            ("Web", Some("EU")),
            ("Edge", Some("Web")),
        ]
    );
    let (eu, edge) = (monitor("EU"), monitor("Edge"));
    assert_eq!(
        (eu.node, eu.group),
        (Some(eu.id), Some(monitor("Production").id))
    );
    assert_eq!(
        (monitor("SSH").node, monitor("SSH").group),
        (Some(edge.id), Some(edge.id))
    );

    match &monitor("Nightly backup").kind {
        MonitorKind::Push { token: Some(token) } => {
            assert_eq!(token.expose(), "fixturePushToken0000000000000001");
        }
        other => panic!("expected a push monitor, got {other:?}"),
    }
    let api = monitor("API");
    assert_eq!(
        (
            api.interval,
            api.retry_interval,
            api.max_retries,
            api.resend_interval
        ),
        (300, 60, 2, 3)
    );
    assert!(
        matches!(&api.kind, MonitorKind::Check(def) if matches!(&def.check, Check::Http(r) if matches!(r.auth, HttpAuth::Basic { .. })))
    );
    assert_eq!(api.tags.len(), 3);
    assert_eq!(api.channels.len(), 3);
    assert_eq!(monitor("Paused http").run, RunState::Paused);
    assert_eq!(monitor("Status JSON").run, RunState::Active);
    assert!(monitor("Status JSON").retry_only_on_status_code_failure);
    assert!(matches!(
        monitor("Docker app").kind,
        MonitorKind::Unsupported
    ));

    let defaults: Vec<&str> = plan
        .channels
        .iter()
        .filter(|c| c.is_default)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(defaults, ["Email (default)"]);
    let inactive = plan
        .channels
        .iter()
        .filter(|c| matches!(c.state, ChannelState::Inactive { .. }))
        .count();
    assert_eq!(inactive, 3);
    assert!(
        plan.channels
            .iter()
            .all(|c| !c.sealed_config.expose().contains("isDefault"))
    );

    let user = &plan.users[0];
    assert!(user.instance_admin);
    assert!(
        user.password_bcrypt
            .as_ref()
            .is_some_and(|hash| hash.expose().starts_with("$2a$10$"))
    );
    assert_eq!(
        user.totp_secret.as_ref().map(|s| s.expose()),
        Some(USER_TOTP_SECRET)
    );

    let page = |slug: &str| {
        plan.status_pages
            .iter()
            .find(|p| p.slug == slug)
            .expect("page")
    };
    assert_eq!(
        page("client-acme").password.as_ref().map(|s| s.expose()),
        Some(STATUS_PAGE_PASSWORD)
    );
    assert_eq!(
        page("status").domains,
        ["status.example.com", "status.example.org"]
    );
    let groups: Vec<(&str, usize)> = page("internal")
        .groups
        .iter()
        .map(|g| (g.name.as_str(), g.components.len()))
        .collect();
    assert_eq!(groups, [("Staging", 3), ("Empty section", 0)]);
    let resolved: Vec<(&str, bool)> = plan
        .incidents
        .iter()
        .map(|i| (i.title.as_str(), i.resolved))
        .collect();
    assert_eq!(resolved, [("Slow checkout", false), ("Login outage", true)]);

    assert_eq!(
        plan.settings.primary_base_url.as_deref(),
        Some("https://uptime.example.com")
    );
    assert_eq!(plan.settings.tls_expiry_notify_days, Some(vec![7, 14, 21]));
    assert_eq!(
        plan.settings.keep_data_period_days,
        Some(KEEP_DATA_PERIOD_DAYS)
    );
    assert_eq!(plan.settings.timezone, Some(chrono_tz::Asia::Kolkata));
    assert_eq!(plan.certificate_reminders.len(), 2);
}

#[test]
fn pauses_a_json_query_that_does_not_parse() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("kuma.db");
    kuma_schema(&path, &migration_names())
        .execute_batch(
            "INSERT INTO monitor (name, type, interval, url, json_path, expected_value)
             VALUES ('Good', 'json-query', 60, 'https://example.com', '$.status', 'ok'),
                    ('Bad', 'json-query', 60, 'https://example.com', '$.status[', 'ok');",
        )
        .expect("monitors");
    let plan = Source::open(&path).expect("open").plan().expect("plan");
    assert_eq!(plan.monitors[0].run, RunState::Active);
    match &plan.monitors[1].run {
        RunState::PausedUnsupported { reason } => {
            assert!(reason.starts_with("JSON query does not parse"))
        }
        other => panic!("expected paused, got {other:?}"),
    }
    let inventoried: Vec<&str> = plan
        .inventory
        .iter()
        .filter(|i| i.kind == InventoryKind::Monitor)
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(inventoried, ["Bad"]);
}

#[test]
fn inventories_a_schedule_that_never_starts() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("kuma.db");
    kuma_schema(&path, &migration_names())
        .execute_batch(
            "INSERT INTO monitor (id, name, type, interval, url) VALUES (1, 'Web', 'http', 60, 'https://example.com');
             INSERT INTO maintenance (id, title, description, strategy, cron, duration, timezone)
             VALUES (1, 'Broken', '', 'cron', 'every tuesday', 600, 'UTC'),
                    (2, 'Nightly', '', 'cron', '0 3 * * *', 600, 'UTC');
             INSERT INTO monitor_maintenance (monitor_id, maintenance_id) VALUES (1, 1), (1, 2);",
        )
        .expect("rows");
    let plan = Source::open(&path).expect("open").plan().expect("plan");
    assert_eq!(plan.maintenance.len(), 1);
    assert_eq!(plan.maintenance[0].title, "Nightly");
    let broken: Vec<(&str, &str)> = plan
        .inventory
        .iter()
        .filter(|i| i.kind == InventoryKind::Maintenance)
        .map(|i| (i.source.table(), i.reason.as_str()))
        .collect();
    assert_eq!(broken.len(), 2);
    assert_eq!(broken[0].0, "maintenance");
    assert_eq!(broken[1].0, "monitor_maintenance");
    assert!(broken[0].1.starts_with("schedule never starts in Kuma"));
    let rows = plan.rows_by_table();
    assert_eq!((rows["maintenance"], rows["monitor_maintenance"]), (2, 2));
}

#[test]
fn secrets_stay_out_of_debug_and_display() {
    let source = Source::open(fixture()).expect("open fixture");
    let plan = source.plan().expect("plan");
    let report = source.report(&plan, i64::MIN).expect("report");
    let shown = format!("{plan:?}\n{report:?}\n{report}");
    for secret in [
        USER_TOTP_SECRET,
        STATUS_PAGE_PASSWORD,
        "$2a$",
        "fixturePushToken",
        "fixture-smtp-password",
        "FIXTURE-NOT-A-REAL-KEY",
        "fixture-webhook-token",
        "fixture-webhook-secret",
        "FIXTURE-not-a-real-bot-token",
        "tk_fixture_not_a_real_token",
        "ufixture000",
        "afixture000",
        "fixture-gotify-token",
        "fixture-matrix-token",
        "fixture-basic-password",
        "fixture-client-secret",
        "fixture-bearer-token",
        "fixture-ntlm-password",
        "fixture-db-password",
        "fixture-kafka-password",
        "fixture-mqtt-password",
        "fixture-rabbit-password",
        "fixture-community",
        "fixture-radius-password",
        "fixture-radius-secret",
        "fixture-redis-password",
        "fixture-browser-token",
        "fixture-proxy-password",
        "fixture-umami-id",
    ] {
        assert!(!shown.contains(secret), "{secret} is shown");
    }
}

#[test]
fn leaves_the_source_untouched() {
    let before = std::fs::read(fixture()).expect("read fixture");
    let source = Source::open(fixture()).expect("open fixture");
    let plan = source.plan().expect("plan");
    source.report(&plan, i64::MIN).expect("report");
    source.heartbeats(i64::MIN, None, 100_000).for_each(drop);
    drop(source);
    assert!(
        std::fs::read(fixture()).expect("read fixture") == before,
        "source changed"
    );
    let dir = fixture().parent().expect("dir");
    assert!(!dir.join("kuma.db-wal").exists() && !dir.join("kuma.db-shm").exists());
}
