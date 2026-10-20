//! The `reconcile` step: what the import wrote, compared with the source table by table, row
//! count by row count and uptime window by uptime window (decision 0008: imported rollups
//! are kept as they are, so V1Per's 24 h, 30 d and 1 y counts must equal Kuma's).

use std::collections::BTreeMap;

use sqlx::SqliteConnection;
use v1per_kuma_import::{Disposition, ImportPlan, Source, StatTable};
use v1per_protocol::ids::{MonitorId, NodeId};
use v1per_protocol::operations::imports::{
    CountCheck, HistoryCheck, ImportManifest, ImportSource, MonitorUptimeCheck, TableCheck,
    UptimeCheck, UptimeCounts, UptimeWindowName, WindowCheck,
};
use v1per_storage::{Counts, Resolution, Series, Storage, UptimeWindow};

use super::control::Written;
use super::history::resolution;

const DAY_MS: i64 = 86_400_000;

/// Every node below the target, the target included, as `sub(id)`.
macro_rules! subtree_cte {
    () => {
        "WITH RECURSIVE sub(id) AS (
             SELECT ?1 UNION ALL SELECT n.id FROM nodes n JOIN sub ON n.parent_id = sub.id
         ) "
    };
}

/// What the manifest compares, beyond the source and storage.
pub struct Inputs<'a> {
    pub run: i64,
    pub node: NodeId,
    pub plan: &'a ImportPlan,
    pub written: Written,
    pub source: ImportSource,
    pub started_at: i64,
    pub raw_since: i64,
    /// Kuma monitor ID to V1Per monitor ID.
    pub monitors: &'a BTreeMap<i64, i64>,
}

pub async fn manifest(
    conn: &mut SqliteConnection,
    storage: &Storage,
    source: &Source,
    inputs: Inputs<'_>,
    finished_at: i64,
) -> anyhow::Result<ImportManifest> {
    let plan = inputs.plan;
    let node = inputs.node;

    let nodes: i64 = sqlx::query_scalar(concat!(subtree_cte!(), "SELECT count(*) - 1 FROM sub"))
        .bind(node.get())
        .fetch_one(&mut *conn)
        .await?;
    let nodes = CountCheck {
        expected: plan.nodes.len() as u64,
        created: nodes as u64,
    };

    let accounted = plan.rows_by_table();
    let report = source.report(plan, inputs.raw_since)?;
    let mut tables = Vec::new();
    for table in &report.tables {
        if !matches!(
            table.disposition,
            Disposition::Config | Disposition::Inventory
        ) {
            continue;
        }
        let name = table.table.as_str();
        let items = plan.inventory.iter().filter(|i| i.source.table() == name);
        let inventoried = items.clone().count() as u64;
        let standalone = items.filter(|i| !i.annotates()).count() as u64;
        let accounted = accounted.get(name).copied().unwrap_or(0);
        let mapped = accounted - standalone;
        let destination = destination(conn, inputs.run, node, name).await?;
        tables.push(TableCheck {
            table: name.to_owned(),
            source: table.rows,
            mapped,
            inventoried,
            destination,
            reconciled: table.rows == accounted && destination.is_none_or(|d| d == mapped),
        });
    }

    let ids: Vec<v1per_storage::MonitorId> = inputs
        .monitors
        .values()
        .map(|id| v1per_storage::MonitorId(*id))
        .collect();
    let imported = storage.imported_rows(&ids).await?.value;
    let mut history = Vec::new();
    for table in StatTable::ALL {
        let (rows, skipped) = split(&source.stat_rows(table)?, inputs.monitors, |n| *n);
        let imported = imported.stats(resolution(table));
        history.push(HistoryCheck {
            table: table.name().to_owned(),
            source: rows,
            imported,
            skipped,
            outside_window: 0,
            reconciled: rows == imported + skipped,
        });
    }
    let beats = source.heartbeat_rows(inputs.raw_since)?;
    let (inside, skipped) = split(&beats, inputs.monitors, |(inside, _)| *inside);
    let outside: u64 = beats.values().map(|(_, outside)| outside).sum();
    history.push(HistoryCheck {
        table: "heartbeat".to_owned(),
        source: inside + outside,
        imported: imported.heartbeats,
        skipped,
        outside_window: outside,
        reconciled: inside == imported.heartbeats + skipped,
    });

    let uptime = uptime(storage, source, plan, inputs.monitors, finished_at).await?;

    let reconciled = inputs.source.unchanged
        && nodes.expected == nodes.created
        && tables.iter().all(|t| t.reconciled)
        && history.iter().all(|h| h.reconciled)
        && uptime.matched == uptime.monitors;
    Ok(ImportManifest {
        reconciled,
        node,
        source: inputs.source,
        started_at: inputs.started_at,
        finished_at,
        raw_since: inputs.raw_since,
        nodes,
        tables,
        history,
        uptime,
        logins: inputs.written.logins,
        notes: inputs.written.notes,
    })
}

/// Sums `counts` into rows of imported monitors and rows of monitors the import skipped.
fn split<T>(
    counts: &BTreeMap<v1per_kuma_import::MonitorId, T>,
    imported: &BTreeMap<i64, i64>,
    rows: impl Fn(&T) -> u64,
) -> (u64, u64) {
    let mut total = 0;
    let mut skipped = 0;
    for (monitor, count) in counts {
        let n = rows(count);
        total += n;
        if !imported.contains_key(&monitor.0) {
            skipped += n;
        }
    }
    (total, skipped)
}

/// The V1Per rows a Kuma table became, counted in the target's subtree, where one V1Per
/// table holds them.
async fn destination(
    conn: &mut SqliteConnection,
    run: i64,
    node: NodeId,
    table: &str,
) -> sqlx::Result<Option<u64>> {
    let sql = match table {
        "monitor" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM monitors WHERE node_id IN sub"
        ),
        "monitor_tag" => concat!(
            subtree_cte!(),
            "SELECT coalesce(sum(json_array_length(tags)), 0) FROM monitors WHERE node_id IN sub"
        ),
        "monitor_notification" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM monitor_channels mc JOIN monitors m ON m.id = mc.monitor_id
             WHERE m.node_id IN sub"
        ),
        "notification" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM channels WHERE node_id IN sub"
        ),
        "notification_sent_history" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM certificate_alerts c JOIN monitors m ON m.id = c.monitor_id
             WHERE m.node_id IN sub"
        ),
        "maintenance" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM maintenances WHERE node_id IN sub"
        ),
        "monitor_maintenance" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM maintenance_monitors mm
             JOIN maintenances m ON m.id = mm.maintenance_id WHERE m.node_id IN sub"
        ),
        "status_page" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM pages WHERE node_id IN sub"
        ),
        "group" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM page_groups g JOIN pages p ON p.id = g.page_id
             WHERE p.node_id IN sub"
        ),
        "monitor_group" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM page_components c JOIN pages p ON p.id = c.page_id
             WHERE p.node_id IN sub"
        ),
        "status_page_cname" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM page_hostnames h JOIN pages p ON p.id = h.page_id
             WHERE p.node_id IN sub"
        ),
        "incident" => concat!(
            subtree_cte!(),
            "SELECT count(*) FROM incidents WHERE node_id IN sub"
        ),
        "user" => {
            return sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM import_objects WHERE run_id = ? AND kind = 'user'",
            )
            .bind(run)
            .fetch_one(conn)
            .await
            .map(|n| Some(n as u64));
        }
        _ => return Ok(None),
    };
    let count: i64 = sqlx::query_scalar(sql)
        .bind(node.get())
        .fetch_one(conn)
        .await?;
    Ok(Some(count as u64))
}

/// Kuma's and V1Per's counts of every imported monitor over the 24 hour, 30 day and 1 year
/// windows, ending with the bucket that holds the newest bucket Kuma wrote.
async fn uptime(
    storage: &Storage,
    source: &Source,
    plan: &ImportPlan,
    monitors: &BTreeMap<i64, i64>,
    fallback_ms: i64,
) -> anyhow::Result<UptimeCheck> {
    let as_of = source.newest_bucket_ms()?.unwrap_or(fallback_ms);
    let ids: Vec<v1per_storage::MonitorId> = monitors
        .values()
        .map(|id| v1per_storage::MonitorId(*id))
        .collect();
    let windows = [
        (
            UptimeWindowName::Day,
            UptimeWindow::Day,
            StatTable::Minutely,
            DAY_MS,
        ),
        (
            UptimeWindowName::Month,
            UptimeWindow::Month,
            StatTable::Daily,
            30 * DAY_MS,
        ),
        (
            UptimeWindowName::Year,
            UptimeWindow::Year,
            StatTable::Daily,
            365 * DAY_MS,
        ),
    ];
    let mut counted = Vec::new();
    for (name, window, table, span) in windows {
        let step = Resolution::step_ms(resolution(table));
        let to = as_of.div_euclid(step) * step + step;
        let kuma = source.uptime_counts(table, to - span, to)?;
        let ours = storage
            .uptime(&ids, Series::Monitor, window, as_of)
            .await?
            .value;
        counted.push((name, kuma, ours));
    }

    let mut per_monitor = Vec::new();
    for monitor in &plan.monitors {
        let Some(id) = monitors.get(&monitor.id.0) else {
            continue;
        };
        let windows: Vec<WindowCheck> = counted
            .iter()
            .map(|(name, kuma, ours)| {
                let (up, down) = kuma.get(&monitor.id).copied().unwrap_or_default();
                let ours = ours
                    .get(&v1per_storage::MonitorId(*id))
                    .copied()
                    .unwrap_or_default();
                WindowCheck {
                    window: *name,
                    kuma: counts(up, down),
                    v1per: v1per_counts(&ours),
                }
            })
            .collect();
        per_monitor.push(MonitorUptimeCheck {
            monitor: MonitorId::new(*id),
            kuma_id: monitor.id.0,
            name: monitor.name.clone(),
            matched: windows.iter().all(|w| w.kuma == w.v1per),
            windows,
        });
    }
    Ok(UptimeCheck {
        as_of,
        monitors: per_monitor.len() as u64,
        matched: per_monitor.iter().filter(|m| m.matched).count() as u64,
        per_monitor,
    })
}

fn counts(up: u64, down: u64) -> UptimeCounts {
    UptimeCounts {
        up,
        down,
        uptime: (up + down > 0).then(|| up as f64 / (up + down) as f64),
    }
}

/// Pending counts as down, as in Kuma; imported rows already fold it into `down`.
fn v1per_counts(counts: &Counts) -> UptimeCounts {
    let mut checked = self::counts(counts.up, counts.down + counts.pending);
    checked.uptime = counts.uptime();
    checked
}
