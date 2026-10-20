//! `probes.*` operations: registration, revocation and health of the probes on a node.

use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{MonitorId, NodeId, ProbeId};
use v1per_protocol::operations::nodes::NodeArgs;
use v1per_protocol::operations::probes::{
    Probe, ProbeArgs, ProbeChoice, ProbeChoiceList, ProbeCreated, ProbeList, ProbeLocation,
    ProbeResult, ProbeStatus, ProbesCreateArgs, ProbesHandlers, ProbesUpdateArgs,
};
use v1per_protocol::registry::Snapshot;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity::{new_session_token, validate_name};
use crate::monitors;
use crate::state::AppState;
use v1per_protocol::limits::NAME_MAX_CHARS;

/// Larger differences between the server's and an agent's clock are flagged.
const SKEW_FLAG_MS: i64 = 5_000;

/// `SELECT <every Row column> FROM probes p`, for `concat!` with a condition.
macro_rules! select_row {
    () => {
        "SELECT p.id, p.node_id, p.name, p.builtin, p.created_at, p.revoked_at, p.last_seen_at,
                p.offline_since, p.version, p.clock_skew_ms, p.checks_started, p.checks_late,
                p.buffered_results, p.dropped_results, p.country, p.city, p.provider, p.platform,
                (SELECT COUNT(*) FROM monitor_probes mp WHERE mp.probe_id = p.id) AS monitors
         FROM probes p"
    };
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    node_id: i64,
    name: String,
    builtin: bool,
    created_at: i64,
    revoked_at: Option<i64>,
    last_seen_at: Option<i64>,
    offline_since: Option<i64>,
    version: Option<String>,
    clock_skew_ms: Option<i64>,
    checks_started: i64,
    checks_late: i64,
    buffered_results: i64,
    dropped_results: i64,
    country: Option<String>,
    city: Option<String>,
    provider: Option<String>,
    platform: Option<String>,
    monitors: i64,
}

impl Row {
    fn node(&self) -> NodeId {
        NodeId::new(self.node_id)
    }

    fn location(&self) -> ProbeLocation {
        ProbeLocation {
            country: self.country.clone(),
            city: self.city.clone(),
            provider: self.provider.clone(),
        }
    }

    fn status(&self) -> ProbeStatus {
        match self {
            Row { builtin: true, .. } => ProbeStatus::Online,
            Row {
                revoked_at: Some(_),
                ..
            } => ProbeStatus::Revoked,
            Row {
                last_seen_at: None, ..
            } => ProbeStatus::Waiting,
            Row {
                offline_since: Some(_),
                ..
            } => ProbeStatus::Offline,
            _ => ProbeStatus::Online,
        }
    }
}

/// `location` with blanks dropped, or a field error. A country is two ASCII letters, stored in
/// upper case.
fn validate_location(location: &ProbeLocation) -> Result<ProbeLocation, ApiError> {
    let text = |field: &str, value: &Option<String>| -> Result<Option<String>, ApiError> {
        let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        if value.chars().count() > NAME_MAX_CHARS {
            return Err(FieldErrors::one(
                field,
                FieldErrorCode::TooLong,
                Some(NAME_MAX_CHARS as u32),
            ));
        }
        Ok(Some(value.to_owned()))
    };
    let country = text("location.country", &location.country)?;
    if country
        .as_deref()
        .is_some_and(|code| code.len() != 2 || !code.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return Err(FieldErrors::one(
            "location.country",
            FieldErrorCode::InvalidFormat,
            None,
        ));
    }
    Ok(ProbeLocation {
        country: country.map(|code| code.to_ascii_uppercase()),
        city: text("location.city", &location.city)?,
        provider: text("location.provider", &location.provider)?,
    })
}

async fn load(conn: &mut SqliteConnection, probe: ProbeId) -> Result<Row, ApiError> {
    sqlx::query_as(concat!(select_row!(), " WHERE p.id = ?"))
        .bind(probe.get())
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// Every probe that is not revoked, the built-in one first, as the API shows them.
pub(crate) async fn all(state: &AppState, conn: &mut SqliteConnection) -> sqlx::Result<Vec<Probe>> {
    let rows: Vec<Row> = sqlx::query_as(concat!(
        select_row!(),
        " WHERE p.revoked_at IS NULL ORDER BY p.builtin DESC, p.name, p.id"
    ))
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|row| state.present(row)).collect())
}

impl AppState {
    /// The probe as the API shows it; the built-in probe reports the server's own figures.
    fn present(&self, row: Row) -> Probe {
        let status = row.status();
        let location = row.location();
        let (version, platform, checks_started, checks_late) = if row.builtin {
            let (started, late) = self.builtin_lag.read();
            (
                Some(env!("CARGO_PKG_VERSION").to_owned()),
                Some(v1per_probe_core::wire::platform()),
                started,
                late,
            )
        } else {
            (
                row.version,
                row.platform,
                row.checks_started as u64,
                row.checks_late as u64,
            )
        };
        Probe {
            id: ProbeId::new(row.id),
            node: NodeId::new(row.node_id),
            name: row.name,
            builtin: row.builtin,
            status,
            location,
            platform,
            created_at: row.created_at,
            revoked_at: row.revoked_at,
            last_seen_at: row.last_seen_at,
            version,
            clock_skew_ms: row.clock_skew_ms,
            clock_skewed: row
                .clock_skew_ms
                .is_some_and(|skew| skew.abs() > SKEW_FLAG_MS),
            checks_started,
            checks_late,
            buffered_results: row.buffered_results as u64,
            dropped_results: row.dropped_results as u64,
            monitors: row.monitors as u32,
        }
    }
}

impl ProbesHandlers for AppState {
    async fn probes_create(
        &self,
        ctx: &mut CommandCtx,
        args: ProbesCreateArgs,
    ) -> Result<ProbeCreated, ApiError> {
        ctx.node_scope(args.node, Capability::ProbesManage).await?;
        let name = validate_name("name", &args.name)?;
        let location = validate_location(&args.location)?;
        let (token, digest) = new_session_token();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO probes (node_id, name, builtin, created_at, token_hash,
                                 country, city, provider)
             VALUES (?, ?, 0, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(args.node.get())
        .bind(&name)
        .bind(ctx.now())
        .bind(digest)
        .bind(&location.country)
        .bind(&location.city)
        .bind(&location.provider)
        .fetch_one(ctx.conn())
        .await?;
        let probe = ProbeId::new(id);
        ctx.record(
            &Topic::probes(args.node),
            "probe.created",
            json!({ "probe": probe }),
        )
        .await?;
        let row = load(ctx.conn(), probe).await?;
        Ok(ProbeCreated {
            probe: self.present(row),
            secret: Some(token),
            secret_withheld: false,
        })
    }

    async fn probes_update(
        &self,
        ctx: &mut CommandCtx,
        args: ProbesUpdateArgs,
    ) -> Result<ProbeResult, ApiError> {
        let row = load(ctx.conn(), args.probe).await?;
        ctx.node_scope(row.node(), Capability::ProbesManage).await?;
        if row.revoked_at.is_some() {
            return Err(FieldErrors::one(
                "probe",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let name = validate_name("name", &args.name)?;
        let location = validate_location(&args.location)?;
        sqlx::query("UPDATE probes SET name = ?, country = ?, city = ?, provider = ? WHERE id = ?")
            .bind(&name)
            .bind(&location.country)
            .bind(&location.city)
            .bind(&location.provider)
            .bind(args.probe.get())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::probes(row.node()),
            "probe.updated",
            json!({ "probe": args.probe }),
        )
        .await?;
        let row = load(ctx.conn(), args.probe).await?;
        Ok(ProbeResult {
            probe: self.present(row),
        })
    }

    async fn probes_revoke(
        &self,
        ctx: &mut CommandCtx,
        args: ProbeArgs,
    ) -> Result<ProbeResult, ApiError> {
        let row = load(ctx.conn(), args.probe).await?;
        ctx.node_scope(row.node(), Capability::ProbesManage).await?;
        if row.builtin {
            return Err(FieldErrors::one(
                "probe",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        if row.revoked_at.is_none() {
            sqlx::query("UPDATE probes SET revoked_at = ? WHERE id = ?")
                .bind(ctx.now())
                .bind(args.probe.get())
                .execute(ctx.conn())
                .await?;
            let served: Vec<(i64, i64)> = sqlx::query_as(
                "DELETE FROM monitor_probes WHERE probe_id = ?
                 RETURNING monitor_id, (SELECT node_id FROM monitors WHERE id = monitor_id)",
            )
            .bind(args.probe.get())
            .fetch_all(ctx.conn())
            .await?;
            for (monitor, node) in served {
                monitors::record_event(
                    ctx,
                    NodeId::new(node),
                    "monitor.updated",
                    json!({ "monitor": MonitorId::new(monitor) }),
                )
                .await?;
            }
            ctx.record(
                &Topic::probes(row.node()),
                "probe.revoked",
                json!({ "probe": args.probe }),
            )
            .await?;
        }
        let row = load(ctx.conn(), args.probe).await?;
        Ok(ProbeResult {
            probe: self.present(row),
        })
    }

    async fn probes_list(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<ProbeList>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        let rows: Vec<Row> =
            sqlx::query_as(concat!(select_row!(), " WHERE p.node_id = ? ORDER BY p.id"))
                .bind(args.node.get())
                .fetch_all(ctx.conn())
                .await?;
        Ok(Snapshot {
            topic: Topic::probes(args.node),
            data: ProbeList {
                probes: rows.into_iter().map(|row| self.present(row)).collect(),
            },
        })
    }

    async fn probes_available(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<ProbeChoiceList>, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeRead).await?;
        // Nearest first: the node, then its ancestors up to the root.
        let mut path = vec![scope.node()];
        path.extend(
            scope
                .ancestors()
                .map(|(node, _)| node)
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
        let ids: Vec<i64> = path.iter().map(|node| node.get()).collect();
        let mut rows: Vec<Row> = sqlx::query_as(concat!(
            select_row!(),
            " WHERE p.revoked_at IS NULL AND p.node_id IN (SELECT value FROM json_each(?))
              ORDER BY p.id"
        ))
        .bind(serde_json::to_string(&ids).map_err(ApiError::internal)?)
        .fetch_all(ctx.conn())
        .await?;
        rows.sort_by_key(|row| path.iter().position(|node| *node == row.node()));
        let readable: Vec<NodeId> = std::iter::once(scope.node())
            .chain(
                scope
                    .ancestors()
                    .filter_map(|(node, readable)| readable.then_some(node)),
            )
            .collect();
        Ok(Snapshot {
            topic: Topic::probes(args.node),
            data: ProbeChoiceList {
                probes: rows
                    .into_iter()
                    .map(|row| ProbeChoice {
                        id: ProbeId::new(row.id),
                        status: row.status(),
                        location: row.location(),
                        own: row.node() == scope.node(),
                        node: readable.contains(&row.node()).then(|| row.node()),
                        last_seen_at: row.last_seen_at,
                        name: row.name,
                        builtin: row.builtin,
                    })
                    .collect(),
            },
        })
    }
}
