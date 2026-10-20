//! Probes (decision 0003, milestones 05 and 06): the built-in probe inside the server, remote
//! agents registered on nodes, and the agent protocol under `/api/probe/*` that `v1per-probe`
//! (`apps/probe`, decision 0011) speaks. Owns `probes.*`.
//!
//! A probe registered on a node serves monitors on that node and below; instance-wide probes,
//! the built-in one included, live on the root. A monitor runs on the probes it names, the
//! built-in probe by default. Checks and the scheduler are shared: the built-in probe and
//! agents run the same [`schedule`] code from `v1per_probe_core` over `v1per_checks`.
//!
//! Topics: `probe.created`, `probe.revoked`, `probe.online` and `probe.offline` on the probe
//! node's `probes:<id>` topic, each with the probe's ID.

mod api;
mod builtin;
pub(crate) mod registry;

pub use v1per_probe_core::{schedule, wire};

pub(crate) use api::{assignments, results};
pub use builtin::run;

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{MonitorId, NodeId, ProbeId};
use v1per_protocol::operations::monitors::MonitorSettings;

use crate::error::{ApiError, FieldErrors};
use crate::events;
use crate::monitors;
use crate::nodes::scope::{NodeScope, path_cte};

/// Without contact for this long a remote probe is offline: three missed contacts of an agent,
/// which polls for assignments at least every 30 seconds.
pub const OFFLINE_AFTER_MS: i64 = 90_000;

/// The built-in probe's ID.
pub async fn builtin(conn: &mut SqliteConnection) -> sqlx::Result<ProbeId> {
    sqlx::query_scalar("SELECT id FROM probes WHERE builtin = 1")
        .fetch_one(conn)
        .await
        .map(ProbeId::new)
}

/// The probes assigned to `monitor`, by ID.
pub(crate) async fn of_monitor(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
) -> sqlx::Result<Vec<ProbeId>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT probe_id FROM monitor_probes WHERE monitor_id = ? ORDER BY probe_id",
    )
    .bind(monitor.get())
    .fetch_all(conn)
    .await?;
    Ok(ids.into_iter().map(ProbeId::new).collect())
}

/// Sets the probes of `monitor` on the node `scope` resolved: none for monitors the server runs,
/// the built-in probe when `probes` is empty. Each must be registered on that node or an
/// ancestor and not revoked; anything else is `invalid_value` on `probes`, whether the probe
/// exists or not.
pub(crate) async fn assign(
    conn: &mut SqliteConnection,
    scope: &NodeScope,
    monitor: MonitorId,
    settings: &MonitorSettings,
    probes: &[ProbeId],
) -> Result<(), ApiError> {
    let invalid = || FieldErrors::one("probes", FieldErrorCode::InvalidValue, None);
    let mut wanted = probes.to_vec();
    wanted.sort();
    wanted.dedup();
    if !monitors::runs_on_probes(settings) {
        if !wanted.is_empty() {
            return Err(invalid());
        }
    } else if wanted.is_empty() {
        wanted.push(builtin(conn).await?);
    }
    for probe in &wanted {
        let node: Option<i64> =
            sqlx::query_scalar("SELECT node_id FROM probes WHERE id = ? AND revoked_at IS NULL")
                .bind(probe.get())
                .fetch_optional(&mut *conn)
                .await?;
        if !node.is_some_and(|node| scope.within(NodeId::new(node))) {
            return Err(invalid());
        }
    }
    sqlx::query("DELETE FROM monitor_probes WHERE monitor_id = ?")
        .bind(monitor.get())
        .execute(&mut *conn)
        .await?;
    for probe in wanted {
        sqlx::query("INSERT INTO monitor_probes (monitor_id, probe_id) VALUES (?, ?)")
            .bind(monitor.get())
            .bind(probe.get())
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// After `monitors` moved, alone or with their node: unassigns probes they named that no longer
/// serve them, because the probe's node is not on the monitor's new path.
pub(crate) async fn prune_moved(
    conn: &mut SqliteConnection,
    monitors: &[MonitorId],
) -> sqlx::Result<()> {
    let ids: Vec<i64> = monitors.iter().map(|monitor| monitor.get()).collect();
    let rows: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT mp.monitor_id, mp.probe_id, m.node_id, p.node_id
         FROM monitor_probes mp
         JOIN monitors m ON m.id = mp.monitor_id
         JOIN probes p ON p.id = mp.probe_id
         WHERE m.id IN (SELECT value FROM json_each(?1)) AND p.builtin = 0",
    )
    .bind(serde_json::to_string(&ids).expect("numbers serialize"))
    .fetch_all(&mut *conn)
    .await?;
    let mut paths: HashMap<i64, Vec<i64>> = HashMap::new();
    for (monitor, probe, monitor_node, probe_node) in rows {
        if let Entry::Vacant(entry) = paths.entry(monitor_node) {
            entry.insert(
                sqlx::query_scalar(concat!(path_cte!(), "SELECT id FROM path"))
                    .bind(monitor_node)
                    .fetch_all(&mut *conn)
                    .await?,
            );
        }
        if !paths[&monitor_node].contains(&probe_node) {
            sqlx::query("DELETE FROM monitor_probes WHERE monitor_id = ? AND probe_id = ?")
                .bind(monitor)
                .bind(probe)
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

/// Records a contact from an agent at `now` (server clock) that it sent at `sent_at` (its
/// clock), with the health it reported, if any. A probe that was offline or never connected
/// comes online; the topic to wake is returned then.
pub(crate) async fn touch(
    conn: &mut SqliteConnection,
    probe: ProbeId,
    now: i64,
    sent_at: i64,
    health: Option<&wire::Health>,
) -> sqlx::Result<Option<Topic>> {
    let row: Option<(i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT node_id, last_seen_at, offline_since FROM probes
         WHERE id = ? AND revoked_at IS NULL",
    )
    .bind(probe.get())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((node, last_seen_at, offline_since)) = row else {
        return Ok(None);
    };
    sqlx::query(
        "UPDATE probes SET last_seen_at = ?, offline_since = NULL, clock_skew_ms = ? WHERE id = ?",
    )
    .bind(now)
    .bind(now - sent_at)
    .bind(probe.get())
    .execute(&mut *conn)
    .await?;
    if let Some(health) = health {
        sqlx::query(
            "UPDATE probes SET version = ?, platform = ?, checks_started = ?, checks_late = ?,
                               buffered_results = ?, dropped_results = ?
             WHERE id = ?",
        )
        .bind(&health.version)
        .bind(&health.platform)
        .bind(health.checks_started as i64)
        .bind(health.checks_late as i64)
        .bind(health.buffered_results as i64)
        .bind(health.dropped_results as i64)
        .bind(probe.get())
        .execute(&mut *conn)
        .await?;
    }
    if offline_since.is_some() {
        sqlx::query("INSERT INTO probe_transitions (probe_id, time, online) VALUES (?, ?, 1)")
            .bind(probe.get())
            .bind(now)
            .execute(&mut *conn)
            .await?;
    } else if last_seen_at.is_some() {
        return Ok(None);
    }
    let topic = Topic::probes(NodeId::new(node));
    events::record(
        conn,
        &topic,
        "probe.online",
        &json!({ "probe": probe }),
        now,
    )
    .await?;
    Ok(Some(topic))
}
