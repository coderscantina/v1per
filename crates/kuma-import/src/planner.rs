//! Builds the [`ImportPlan`] from the configuration tables.

use std::collections::{BTreeMap, BTreeSet};

use chrono_tz::Tz;
use rusqlite::Connection;
use serde_json::{Map, Value as Json};
use v1per_checks::Secret;
use v1per_maintenance_schedule::Definition;

use crate::plan::{
    CertificateReminder, ImportPlan, InventoryItem, InventoryKind, MaintenanceId, MaintenancePlan,
    MonitorId, MonitorKind, MonitorPlan, MonitorTag, NodePlan, NotificationId, RunState,
    SecretCounts, SettingsPlan, StatusPageId, TagId, TagPlan, UserId, UserPlan,
};
use crate::record::{Record, read_table};
use crate::{channels, monitors, pages};

/// A source that verified as Kuma 2.5.0 but holds rows no import could make sense of.
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("{table} {id}: {reason}")]
    InvalidSource {
        table: &'static str,
        id: i64,
        reason: String,
    },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Collects inventory entries and secret counts while the tables are mapped.
#[derive(Default)]
pub(crate) struct Ledger {
    pub inventory: Vec<InventoryItem>,
    pub secrets: SecretCounts,
}

impl Ledger {
    pub fn add(
        &mut self,
        kind: InventoryKind,
        label: impl Into<String>,
        reason: impl Into<String>,
        record: &Record,
    ) {
        self.add_source(kind, label, reason, record.source());
    }

    pub fn add_source(
        &mut self,
        kind: InventoryKind,
        label: impl Into<String>,
        reason: impl Into<String>,
        source: crate::record::SourceRecord,
    ) {
        self.inventory.push(InventoryItem {
            kind,
            label: label.into(),
            reason: reason.into(),
            source,
        });
    }

    /// Groups link rows by their `owner` column. Rows whose `target` is missing are
    /// inventoried as orphans; SQLite only enforces Kuma's foreign keys when asked to.
    pub fn links<'a>(
        &mut self,
        rows: &'a [Record],
        owner: &str,
        owners: &BTreeSet<i64>,
        target: &str,
        targets: &BTreeSet<i64>,
    ) -> BTreeMap<i64, Vec<&'a Record>> {
        let mut links: BTreeMap<i64, Vec<&Record>> = BTreeMap::new();
        for row in rows {
            let owner_id = row.int(owner).filter(|id| owners.contains(id));
            let target_id = row.int(target).filter(|id| targets.contains(id));
            match (owner_id, target_id) {
                (Some(owner_id), Some(_)) => links.entry(owner_id).or_default().push(row),
                (missing_owner, _) => {
                    let missing = if missing_owner.is_none() {
                        owner
                    } else {
                        target
                    };
                    self.add(
                        InventoryKind::Orphan,
                        format!("{} {}", row.table(), row.id()),
                        format!("links a deleted {}", missing.trim_end_matches("_id")),
                        row,
                    );
                }
            }
        }
        links
    }
}

pub(crate) fn plan(conn: &Connection) -> Result<ImportPlan, PlanError> {
    let mut ledger = Ledger::default();

    let settings = settings(&read_table(conn, "setting")?, &mut ledger);
    let users = users(&read_table(conn, "user")?, &mut ledger);
    let tags: Vec<TagPlan> = read_table(conn, "tag")?
        .iter()
        .map(|row| TagPlan {
            id: TagId(row.id()),
            name: row.text("name").unwrap_or_default(),
            color: row.text("color").unwrap_or_default(),
        })
        .collect();

    let channels: Vec<_> = read_table(conn, "notification")?
        .iter()
        .map(|row| {
            let channel = channels::channel(row);
            // Kuma never writes or reads the column; it sends to every linked channel.
            if !row.flag("active") {
                ledger.add(
                    InventoryKind::Channel,
                    &channel.name,
                    "Kuma's unused `active` flag is off; the channel imports as Kuma used it",
                    row,
                );
            }
            channel
        })
        .collect();
    ledger.secrets.channel_configs = channels.len() as u64;

    let proxies = read_table(conn, "proxy")?;
    let active_proxies: BTreeSet<i64> = proxies
        .iter()
        .filter(|row| row.flag("active"))
        .map(Record::id)
        .collect();
    for row in &proxies {
        ledger.secrets.inventory_credentials += u64::from(row.filled("password").is_some());
        ledger.add(
            InventoryKind::Proxy,
            format!("{} proxy", row.text("protocol").unwrap_or_default()),
            "proxies are not supported yet (open in decision 0007); http monitors that use an active one import paused",
            row,
        );
    }

    let tag_ids = tags.iter().map(|tag| tag.id.0).collect();
    let channel_ids = channels.iter().map(|channel| channel.id.0).collect();
    let (nodes, monitors) = monitors(conn, &active_proxies, &tag_ids, &channel_ids, &mut ledger)?;
    let monitor_ids: BTreeSet<i64> = monitors.iter().map(|m| m.id.0).collect();

    let (status_pages, incidents) = pages::pages(conn, &monitor_ids, &mut ledger)?;
    let page_ids = status_pages.iter().map(|page| page.id.0).collect();
    let maintenance = maintenance(
        conn,
        settings.timezone,
        &monitor_ids,
        &page_ids,
        &mut ledger,
    )?;

    let mut certificate_reminders = Vec::new();
    for row in read_table(conn, "notification_sent_history")? {
        let monitor = row.int("monitor_id").filter(|id| monitor_ids.contains(id));
        let days = row.int("days").and_then(|days| u32::try_from(days).ok());
        match (row.text("type").as_deref(), monitor, days) {
            (Some("certificate"), Some(monitor), Some(days)) => {
                certificate_reminders.push(CertificateReminder {
                    monitor: MonitorId(monitor),
                    days,
                });
            }
            (Some("certificate"), None, _) => ledger.add(
                InventoryKind::Orphan,
                format!("notification_sent_history {}", row.id()),
                "links a deleted monitor",
                &row,
            ),
            (kind, _, _) => ledger.add(
                InventoryKind::SentHistory,
                kind.unwrap_or_default(),
                "only certificate reminders carry over",
                &row,
            ),
        }
    }

    let column = |name: &'static str| move |row: &Record| row.text(name).unwrap_or_default();
    inventory_table(
        conn,
        "api_key",
        InventoryKind::ApiKey,
        column("name"),
        "Kuma API keys are not imported; issue a new V1Per API key",
        &mut ledger,
    )?;
    inventory_table(
        conn,
        "docker_host",
        InventoryKind::DockerHost,
        column("name"),
        "docker monitors are not supported",
        &mut ledger,
    )?;
    let browsers = inventory_table(
        conn,
        "remote_browser",
        InventoryKind::RemoteBrowser,
        column("name"),
        "real-browser monitors are not supported",
        &mut ledger,
    )?;
    // A remote browser URL usually carries an access token.
    ledger.secrets.inventory_credentials += browsers;
    inventory_table(
        conn,
        "domain_expiry",
        InventoryKind::DomainExpiry,
        column("domain"),
        "domain expiry alerts are not a V1Per event yet (open in the notifications contract)",
        &mut ledger,
    )?;
    inventory_table(
        conn,
        "monitor_tls_info",
        InventoryKind::TlsInfo,
        |row| format!("monitor {}", row.int("monitor_id").unwrap_or_default()),
        "cached certificate details; the first check reads them again",
        &mut ledger,
    )?;

    Ok(ImportPlan {
        nodes,
        monitors,
        tags,
        channels,
        maintenance,
        status_pages,
        incidents,
        users,
        settings,
        certificate_reminders,
        inventory: ledger.inventory,
        secrets: ledger.secrets,
    })
}

fn inventory_table(
    conn: &Connection,
    table: &'static str,
    kind: InventoryKind,
    label: impl Fn(&Record) -> String,
    reason: &str,
    ledger: &mut Ledger,
) -> rusqlite::Result<u64> {
    let rows = read_table(conn, table)?;
    for row in &rows {
        ledger.add(kind, label(row), reason, row);
    }
    Ok(rows.len() as u64)
}

fn monitors(
    conn: &Connection,
    active_proxies: &BTreeSet<i64>,
    tag_ids: &BTreeSet<i64>,
    channel_ids: &BTreeSet<i64>,
    ledger: &mut Ledger,
) -> Result<(Vec<NodePlan>, Vec<MonitorPlan>), PlanError> {
    let rows = read_table(conn, "monitor")?;
    let ids: BTreeSet<i64> = rows.iter().map(Record::id).collect();
    let groups: BTreeSet<i64> = rows
        .iter()
        .filter(|row| row.text("type").as_deref() == Some("group"))
        .map(Record::id)
        .collect();
    let parents: BTreeMap<i64, i64> = rows
        .iter()
        .filter_map(|row| Some((row.id(), row.int("parent")?)))
        .collect();
    for (&child, &parent) in &parents {
        if !groups.contains(&parent) {
            return Err(PlanError::InvalidSource {
                table: "monitor",
                id: child,
                reason: format!("its parent {parent} is not a group monitor"),
            });
        }
    }
    let depth = |id: i64| -> Result<usize, PlanError> {
        let mut depth = 0;
        let mut current = id;
        while let Some(&parent) = parents.get(&current) {
            depth += 1;
            current = parent;
            if depth > parents.len() {
                return Err(PlanError::InvalidSource {
                    table: "monitor",
                    id,
                    reason: "its group parents form a cycle".to_owned(),
                });
            }
        }
        Ok(depth)
    };

    let tag_rows = read_table(conn, "monitor_tag")?;
    let mut tags = ledger.links(&tag_rows, "monitor_id", &ids, "tag_id", tag_ids);
    let channel_rows = read_table(conn, "monitor_notification")?;
    let mut channels = ledger.links(
        &channel_rows,
        "monitor_id",
        &ids,
        "notification_id",
        channel_ids,
    );

    let mut nodes = Vec::new();
    let mut plans = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = row.id();
        let kuma_type = row.text("type").unwrap_or_default();
        let name = row.text("name").unwrap_or_default();
        let parent = parents.get(&id).copied().map(MonitorId);
        if groups.contains(&id) {
            nodes.push((
                depth(id)?,
                NodePlan {
                    group: MonitorId(id),
                    name: name.clone(),
                    parent,
                },
            ));
        }

        let (interval, interval_problem) = match monitors::interval(row) {
            Ok(interval) => (interval, None),
            Err(reason) => (20, Some(reason)),
        };
        let timeout = monitors::timeout(row, interval);
        let upside_down = row.flag("upside_down");
        let mapped = match interval_problem {
            Some(reason) => Err(reason),
            None => monitors::kind(row, timeout, upside_down),
        };
        let (kind, held) = match mapped {
            Ok(kind) => {
                let held = monitors::hold_reason(row, &kind, active_proxies);
                (kind, held)
            }
            Err(reason) => (MonitorKind::Unsupported, Some(reason)),
        };
        let inventoried = held.is_some();
        ledger.secrets.monitor_credentials += monitors::credential_count(row, &kind, inventoried);
        let run = match held {
            Some(reason) => {
                ledger.add(InventoryKind::Monitor, &name, &reason, row);
                RunState::PausedUnsupported { reason }
            }
            None if row.flag("active") => RunState::Active,
            None => RunState::Paused,
        };

        let count = |column: &str| {
            row.int(column)
                .map_or(0, |n| n.clamp(0, i64::from(u32::MAX)) as u32)
        };
        plans.push(MonitorPlan {
            id: MonitorId(id),
            name,
            description: row.text("description").unwrap_or_default(),
            node: if groups.contains(&id) {
                Some(MonitorId(id))
            } else {
                parent
            },
            group: parent,
            kind,
            run,
            interval,
            retry_interval: match count("retry_interval") {
                0 => interval,
                retry => retry,
            },
            max_retries: count("maxretries"),
            resend_interval: count("resend_interval"),
            timeout,
            upside_down,
            retry_only_on_status_code_failure: kuma_type == "json-query"
                && row.flag("retry_only_on_status_code_failure"),
            expiry_notification: row.flag("expiry_notification"),
            kuma_type,
            tags: tags
                .remove(&id)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|link| {
                    Some(MonitorTag {
                        tag: TagId(link.int("tag_id")?),
                        value: link.text("value").unwrap_or_default(),
                    })
                })
                .collect(),
            channels: channels
                .remove(&id)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|link| Some(NotificationId(link.int("notification_id")?)))
                .collect(),
        });
    }
    nodes.sort_by_key(|(depth, node)| (*depth, node.group));
    Ok((nodes.into_iter().map(|(_, node)| node).collect(), plans))
}

fn maintenance(
    conn: &Connection,
    server_timezone: Option<Tz>,
    monitor_ids: &BTreeSet<i64>,
    page_ids: &BTreeSet<i64>,
    ledger: &mut Ledger,
) -> Result<Vec<MaintenancePlan>, PlanError> {
    let rows = read_table(conn, "maintenance")?;
    let ids: BTreeSet<i64> = rows.iter().map(Record::id).collect();
    let monitor_rows = read_table(conn, "monitor_maintenance")?;
    let mut monitors = ledger.links(
        &monitor_rows,
        "maintenance_id",
        &ids,
        "monitor_id",
        monitor_ids,
    );
    let page_rows = read_table(conn, "maintenance_status_page")?;
    let mut pages = ledger.links(
        &page_rows,
        "maintenance_id",
        &ids,
        "status_page_id",
        page_ids,
    );

    let mut plans = Vec::new();
    for row in &rows {
        let id = row.id();
        let title = row.text("title").unwrap_or_default();
        let monitor_links = monitors.remove(&id).unwrap_or_default();
        let page_links = pages.remove(&id).unwrap_or_default();
        match definition(row, server_timezone.unwrap_or(Tz::UTC)) {
            Ok(definition) => plans.push(MaintenancePlan {
                id: MaintenanceId(id),
                title,
                description: row.text("description").unwrap_or_default(),
                definition,
                monitors: monitor_links
                    .iter()
                    .filter_map(|link| link.int("monitor_id").map(MonitorId))
                    .collect(),
                status_pages: page_links
                    .iter()
                    .filter_map(|link| link.int("status_page_id").map(StatusPageId))
                    .collect(),
            }),
            Err(reason) => {
                ledger.add(InventoryKind::Maintenance, &title, &reason, row);
                for link in monitor_links.into_iter().chain(page_links) {
                    ledger.add(InventoryKind::Maintenance, &title, &reason, link);
                }
            }
        }
    }
    Ok(plans)
}

/// The schedule crate's definition, validated, or why Kuma could never activate it.
fn definition(row: &Record, server_timezone: Tz) -> Result<Definition, String> {
    let text = |column: &str| row.text(column).map_or(Json::Null, Json::String);
    let int = |column: &str| row.int(column).map_or(Json::Null, Json::from);
    let fields = Map::from_iter([
        ("strategy".to_owned(), text("strategy")),
        ("active".to_owned(), Json::Bool(row.flag("active"))),
        ("start_date".to_owned(), text("start_date")),
        ("end_date".to_owned(), text("end_date")),
        ("start_time".to_owned(), text("start_time")),
        ("end_time".to_owned(), text("end_time")),
        ("weekdays".to_owned(), text("weekdays")),
        ("days_of_month".to_owned(), text("days_of_month")),
        ("interval_day".to_owned(), int("interval_day")),
        ("cron".to_owned(), text("cron")),
        ("duration".to_owned(), int("duration")),
        ("timezone".to_owned(), text("timezone")),
    ]);
    let mut definition: Definition = serde_json::from_value(Json::Object(fields))
        .map_err(|error| format!("schedule does not parse: {error}"))?;
    definition.server_timezone = server_timezone;
    definition
        .validate()
        .map_err(|error| format!("schedule never starts in Kuma: {error}"))?;
    Ok(definition)
}

/// Settings Kuma keeps for itself; none of them describes the monitoring setup.
const INTERNAL_SETTINGS: [&str; 5] = [
    "database_version",
    "databasePatchedFiles",
    "migrateAggregateTableState",
    "jwtSecret",
    "initServerTimezone",
];

fn settings(rows: &[Record], ledger: &mut Ledger) -> SettingsPlan {
    let mut plan = SettingsPlan::default();
    for row in rows {
        let key = row.text("key").unwrap_or_default();
        // Kuma stores values JSON encoded, so a string keeps its quotes.
        let value: Option<Json> = row
            .text("value")
            .and_then(|text| serde_json::from_str(&text).ok());
        let mapped = match key.as_str() {
            "primaryBaseURL" => value
                .and_then(|v| v.as_str().map(str::to_owned))
                .filter(|url| !url.is_empty())
                .map(|url| plan.primary_base_url = Some(url)),
            "tlsExpiryNotifyDays" => value
                .and_then(|v| serde_json::from_value::<Vec<u32>>(v).ok())
                .map(|days| plan.tls_expiry_notify_days = Some(days)),
            "keepDataPeriodDays" => value
                .and_then(|v| match v {
                    Json::String(text) => text.trim().parse().ok(),
                    other => other.as_i64(),
                })
                .map(|days| plan.keep_data_period_days = Some(days)),
            "serverTimezone" => value
                .and_then(|v| v.as_str()?.parse::<Tz>().ok())
                .map(|tz| plan.timezone = Some(tz)),
            _ => {
                let reason = if INTERNAL_SETTINGS.contains(&key.as_str()) {
                    "Kuma internal state"
                } else {
                    "no V1Per equivalent"
                };
                ledger.add(InventoryKind::Setting, &key, reason, row);
                continue;
            }
        };
        if mapped.is_none() {
            ledger.add(
                InventoryKind::Setting,
                &key,
                "value is empty or does not parse",
                row,
            );
        }
    }
    plan
}

fn users(rows: &[Record], ledger: &mut Ledger) -> Vec<UserPlan> {
    rows.iter()
        .enumerate()
        .map(|(position, row)| {
            let username = row.text("username").unwrap_or_default();
            let password_bcrypt = match row.filled("password") {
                Some(hash) if hash.starts_with("sha1") => {
                    ledger.add_source(
                        InventoryKind::Password,
                        &username,
                        "legacy sha1 password hash; set a new password after import",
                        row.source_of(&["username"]),
                    );
                    None
                }
                hash => hash.map(Secret::new),
            };
            let totp_secret = match (row.flag("twofa_status"), row.filled("twofa_secret")) {
                (true, Some(secret)) => {
                    ledger.secrets.totp_secrets += 1;
                    Some(Secret::new(secret))
                }
                (false, Some(_)) => {
                    ledger.add_source(
                        InventoryKind::TwoFactor,
                        &username,
                        "2FA is off; the leftover secret is not imported",
                        row.source_of(&["username"]),
                    );
                    None
                }
                _ => None,
            };
            UserPlan {
                id: UserId(row.id()),
                username,
                password_bcrypt,
                active: row.flag("active"),
                totp_secret,
                instance_admin: position == 0,
                timezone: row.filled("timezone"),
            }
        })
        .collect()
}
