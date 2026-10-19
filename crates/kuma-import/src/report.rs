//! The dry-run report: counts, support status, secrets to seal and the inventory. Holds no
//! secret values, so it is safe to print and to log.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::history::{count_heartbeats, time_text};
use crate::plan::{ChannelState, ImportPlan, InventoryKind, RunState, SecretCounts};
use crate::source::ReadMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunReport {
    pub path: PathBuf,
    pub read_mode: ReadMode,
    pub tables: Vec<TableCount>,
    /// Child nodes made from group monitors.
    pub nodes: u64,
    pub monitors_by_type: Vec<MonitorTypeCount>,
    /// Monitors imported paused because v1per cannot run them, by reason.
    pub held_reasons: BTreeMap<String, u64>,
    pub channels_by_provider: Vec<ProviderCount>,
    pub secrets: SecretCounts,
    pub heartbeats: HeartbeatCounts,
    pub inventory: Vec<InventoryGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCount {
    pub table: String,
    pub rows: u64,
    /// Rows the plan maps or inventories; only for configuration and inventory tables.
    pub accounted: Option<u64>,
    pub disposition: Disposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Mapped to v1per records, some rows possibly inventoried.
    Config,
    /// Every row goes to the inventory.
    Inventory,
    /// Heartbeats and rollups, streamed into heartbeat storage.
    History,
    /// Checked for the 2.5.0 signature, not imported.
    Signature,
    /// SQLite's own bookkeeping.
    Internal,
    /// Not part of Kuma 2.5.0; not imported.
    Unknown,
}

impl Disposition {
    fn of(table: &str) -> Self {
        match table {
            "monitor"
            | "monitor_tag"
            | "monitor_notification"
            | "tag"
            | "notification"
            | "maintenance"
            | "monitor_maintenance"
            | "maintenance_status_page"
            | "status_page"
            | "status_page_cname"
            | "group"
            | "monitor_group"
            | "incident"
            | "user"
            | "setting"
            | "notification_sent_history" => Self::Config,
            "api_key" | "proxy" | "docker_host" | "remote_browser" | "domain_expiry"
            | "monitor_tls_info" => Self::Inventory,
            "heartbeat" | "stat_minutely" | "stat_hourly" | "stat_daily" => Self::History,
            "knex_migrations" | "knex_migrations_lock" => Self::Signature,
            table if table.starts_with("sqlite_") => Self::Internal,
            _ => Self::Unknown,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Config => "mapped",
            Self::Inventory => "inventory",
            Self::History => "history",
            Self::Signature => "signature only",
            Self::Internal => "sqlite internal",
            Self::Unknown => "unknown, not imported",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MonitorTypeCount {
    pub kuma_type: String,
    pub total: u64,
    pub active: u64,
    pub paused_in_kuma: u64,
    pub paused_unsupported: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderCount {
    pub provider: String,
    pub active: u64,
    pub inactive: u64,
    pub reasons: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatCounts {
    /// Start of the raw retention window, UTC Unix milliseconds.
    pub since_ms: i64,
    /// Imported as raw heartbeats.
    pub inside: u64,
    /// Older; only their rollups are imported.
    pub outside: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryGroup {
    pub kind: InventoryKind,
    pub reason: String,
    pub labels: Vec<String>,
}

pub(crate) fn build(
    conn: &Connection,
    path: PathBuf,
    read_mode: ReadMode,
    plan: &ImportPlan,
    since_ms: i64,
) -> rusqlite::Result<DryRunReport> {
    let accounted = plan.rows_by_table();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut tables = Vec::with_capacity(names.len());
    for table in names {
        let rows = conn.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
            row.get::<_, i64>(0)
        })? as u64;
        let disposition = Disposition::of(&table);
        let accounted = matches!(disposition, Disposition::Config | Disposition::Inventory)
            .then(|| accounted.get(table.as_str()).copied().unwrap_or_default());
        tables.push(TableCount {
            table,
            rows,
            accounted,
            disposition,
        });
    }

    let mut types: BTreeMap<&str, MonitorTypeCount> = BTreeMap::new();
    let mut held_reasons = BTreeMap::new();
    for monitor in &plan.monitors {
        let count = types.entry(&monitor.kuma_type).or_default();
        count.total += 1;
        match &monitor.run {
            RunState::Active => count.active += 1,
            RunState::Paused => count.paused_in_kuma += 1,
            RunState::PausedUnsupported { reason } => {
                count.paused_unsupported += 1;
                *held_reasons.entry(reason.clone()).or_default() += 1;
            }
        }
    }

    let mut providers: BTreeMap<&str, ProviderCount> = BTreeMap::new();
    for channel in &plan.channels {
        let count = providers.entry(&channel.provider).or_default();
        match &channel.state {
            ChannelState::Active => count.active += 1,
            ChannelState::Inactive { reason } => {
                count.inactive += 1;
                count.reasons.insert(reason.clone());
            }
        }
    }

    let mut inventory: BTreeMap<(InventoryKind, &str), Vec<String>> = BTreeMap::new();
    for item in &plan.inventory {
        inventory
            .entry((item.kind, &item.reason))
            .or_default()
            .push(item.label.clone());
    }

    let (inside, outside) = count_heartbeats(conn, since_ms)?;
    Ok(DryRunReport {
        path,
        read_mode,
        tables,
        nodes: plan.nodes.len() as u64,
        monitors_by_type: types
            .into_iter()
            .map(|(kuma_type, count)| MonitorTypeCount {
                kuma_type: kuma_type.to_owned(),
                ..count
            })
            .collect(),
        held_reasons,
        channels_by_provider: providers
            .into_iter()
            .map(|(provider, count)| ProviderCount {
                provider: provider.to_owned(),
                ..count
            })
            .collect(),
        secrets: plan.secrets,
        heartbeats: HeartbeatCounts {
            since_ms,
            inside,
            outside,
        },
        inventory: inventory
            .into_iter()
            .map(|((kind, reason), labels)| InventoryGroup {
                kind,
                reason: reason.to_owned(),
                labels,
            })
            .collect(),
    })
}

impl fmt::Display for DryRunReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.read_mode {
            ReadMode::Immutable => "read as immutable",
            ReadMode::ReadOnlyWal => "read only, including its WAL",
        };
        writeln!(f, "Uptime Kuma 2.5.0 import, dry run")?;
        writeln!(f, "Source: {} ({mode})", self.path.display())?;

        writeln!(f, "\nTables{:>32}  accounted", "rows")?;
        for table in &self.tables {
            let accounted = match table.accounted {
                Some(n) if n == table.rows => format!("{n} {}", table.disposition.label()),
                Some(n) => format!(
                    "{n} {}, {} NOT ACCOUNTED",
                    table.disposition.label(),
                    table.rows.abs_diff(n)
                ),
                None => table.disposition.label().to_owned(),
            };
            writeln!(f, "  {:<26}{:>10}  {accounted}", table.table, table.rows)?;
        }

        writeln!(f, "\nNodes from group monitors: {}", self.nodes)?;
        writeln!(
            f,
            "\nMonitors by type{:>16}{:>8}{:>8}{:>13}",
            "total", "run", "paused", "unsupported"
        )?;
        for count in &self.monitors_by_type {
            writeln!(
                f,
                "  {:<26}{:>4}{:>8}{:>8}{:>13}",
                count.kuma_type,
                count.total,
                count.active,
                count.paused_in_kuma,
                count.paused_unsupported
            )?;
        }
        if !self.held_reasons.is_empty() {
            writeln!(f, "\nMonitors imported paused")?;
            for (reason, count) in &self.held_reasons {
                writeln!(f, "  {count:>4}  {reason}")?;
            }
        }

        writeln!(
            f,
            "\nChannels by provider{:>18}{:>10}",
            "active", "inactive"
        )?;
        for count in &self.channels_by_provider {
            let provider = if count.provider.is_empty() {
                "(none)"
            } else {
                &count.provider
            };
            let line = format!("  {provider:<26}{:>10}{:>10}", count.active, count.inactive);
            let reasons = count.reasons.iter().cloned().collect::<Vec<_>>().join("; ");
            writeln!(f, "{}", format!("{line}  {reasons}").trim_end())?;
        }

        let s = &self.secrets;
        writeln!(f, "\nSecrets (counts only)")?;
        for (count, what) in [
            (s.monitor_credentials, "monitor credentials to seal"),
            (s.channel_configs, "channel configs to seal"),
            (s.totp_secrets, "TOTP secrets to seal"),
            (s.inventory_credentials, "inventoried credentials to seal"),
            (s.page_passwords, "status page passwords to hash"),
        ] {
            writeln!(f, "  {count:>6}  {what}")?;
        }

        let h = &self.heartbeats;
        let since = time_text(h.since_ms);
        let since = if since.is_empty() {
            "the start"
        } else {
            &since
        };
        writeln!(
            f,
            "\nHeartbeats since {since} UTC: {} imported raw, {} older (rollups only)",
            h.inside, h.outside
        )?;

        let total: usize = self.inventory.iter().map(|group| group.labels.len()).sum();
        writeln!(f, "\nInventory ({total} entries)")?;
        for group in &self.inventory {
            writeln!(
                f,
                "  {} ({}): {}",
                group.kind.label(),
                group.labels.len(),
                group.reason
            )?;
            writeln!(f, "      {}", group.labels.join(", "))?;
        }
        Ok(())
    }
}
