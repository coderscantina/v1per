//! Notifications (milestone 07, decision 0005): channels on nodes, their links to monitors, the
//! durable outbox and its worker. Owns `channels.*`; `instance.testSmtp` queues through it too.
//!
//! A channel's config is Kuma's `notification.config` JSON sealed under `channel:<id>:config`
//! (decision 0006); answers carry the typed settings with secrets shown only as set. A monitor
//! alerts channels on its node or an ancestor; new monitors take the default channels there
//! unless their creator picks others.
//!
//! Topics: `channel.created`, `channel.updated`, `channel.deleted` and `channel.delivery` on
//! the channel node's `channels:<id>` topic.

mod certificates;
pub(crate) mod config;
mod outbox;

pub(crate) use certificates::check as check_certificates;
pub use outbox::run;
pub(crate) use outbox::{Beat, ProbePayload, queue_beat, queue_probe_offline, queue_test};
use outbox::{queue_sample_test, sample_payload};

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{ChannelId, DeliveryId, MonitorId, NodeId};
use v1per_protocol::operations::channels::{
    Channel, ChannelArgs, ChannelChoice, ChannelChoiceList, ChannelDeleted, ChannelList,
    ChannelResult, ChannelSettings, ChannelTestQueued, ChannelsCreateArgs, ChannelsHandlers,
    ChannelsTestArgs, ChannelsUpdateArgs, DeliveriesArgs, Delivery, DeliveryArgs, DeliveryEvent,
    DeliveryPage, DeliveryResult, DeliveryState, MonitorDeliveriesArgs, UnsupportedChannel,
};
use v1per_protocol::operations::nodes::NodeArgs;
use v1per_protocol::registry::Snapshot;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity::secrets::SecretKey;
use crate::identity::validate_name;
use crate::nodes::scope::{NodeScope, path_cte};
use crate::state::AppState;

const PAGE_DEFAULT: u32 = 25;
const PAGE_MAX: u32 = 100;

/// `SELECT <every Row column> FROM channels c`, for `concat!` with a condition.
macro_rules! select_row {
    () => {
        "SELECT c.id, c.node_id, c.name, c.provider, c.config, c.is_default, c.active,
                c.inactive_reason, c.revision, c.created_at, c.updated_at,
                (SELECT COUNT(*) FROM monitor_channels mc WHERE mc.channel_id = c.id) AS monitors
         FROM channels c"
    };
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    node_id: i64,
    name: String,
    provider: String,
    config: Vec<u8>,
    is_default: bool,
    active: bool,
    inactive_reason: Option<String>,
    revision: i64,
    created_at: i64,
    updated_at: i64,
    monitors: i64,
}

impl Row {
    fn channel(&self) -> ChannelId {
        ChannelId::new(self.id)
    }

    fn node(&self) -> NodeId {
        NodeId::new(self.node_id)
    }

    fn open(&self, key: &SecretKey) -> Result<config::Kuma, ApiError> {
        config::open(key, &config::channel_context(self.channel()), &self.config)
            .map_err(ApiError::internal)
    }

    fn present(self, key: &SecretKey) -> Result<Channel, ApiError> {
        let settings = config::from_kuma(&self.open(key)?).unwrap_or_else(|| {
            ChannelSettings::Unsupported(UnsupportedChannel {
                kuma_type: self.provider.clone(),
            })
        });
        Ok(Channel {
            id: self.channel(),
            node: self.node(),
            name: self.name,
            is_default: self.is_default,
            active: self.active,
            inactive_reason: self.inactive_reason,
            settings,
            monitors: self.monitors as u32,
            revision: self.revision,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// The channel before any scope check; callers resolve the scope on its node next.
async fn load(conn: &mut SqliteConnection, channel: ChannelId) -> Result<Row, ApiError> {
    sqlx::query_as(concat!(select_row!(), " WHERE c.id = ?"))
        .bind(channel.get())
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// Kuma's "apply on all existing monitors": links the channel to every monitor on its node
/// and below.
async fn apply_existing(
    conn: &mut SqliteConnection,
    channel: ChannelId,
    node: NodeId,
) -> sqlx::Result<()> {
    sqlx::query(
        "WITH RECURSIVE below(id) AS (
             SELECT ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id
         )
         INSERT OR IGNORE INTO monitor_channels (monitor_id, channel_id)
         SELECT m.id, ?2 FROM monitors m WHERE m.node_id IN (SELECT id FROM below)",
    )
    .bind(node.get())
    .bind(channel.get())
    .execute(conn)
    .await
    .map(drop)
}

/// Sets the channels of `monitor` on the node `scope` resolved. `None` keeps them, or on a
/// new monitor takes the default channels on the node and its ancestors. Each channel must
/// be on that path; anything else is `invalid_value` on `channels`, whether it exists or not.
pub(crate) async fn link(
    conn: &mut SqliteConnection,
    scope: &NodeScope,
    monitor: MonitorId,
    channels: Option<&[ChannelId]>,
    new: bool,
) -> Result<(), ApiError> {
    let wanted: Vec<i64> =
        match channels {
            None if !new => return Ok(()),
            None => sqlx::query_scalar(concat!(
                path_cte!(),
                "SELECT c.id FROM channels c JOIN path p ON p.id = c.node_id WHERE c.is_default = 1"
            ))
            .bind(scope.node().get())
            .fetch_all(&mut *conn)
            .await?,
            Some(channels) => {
                let mut ids: Vec<i64> = channels.iter().map(|c| c.get()).collect();
                ids.sort_unstable();
                ids.dedup();
                for id in &ids {
                    let node: Option<i64> =
                        sqlx::query_scalar("SELECT node_id FROM channels WHERE id = ?")
                            .bind(id)
                            .fetch_optional(&mut *conn)
                            .await?;
                    if !node.is_some_and(|node| scope.within(NodeId::new(node))) {
                        return Err(FieldErrors::one(
                            "channels",
                            FieldErrorCode::InvalidValue,
                            None,
                        ));
                    }
                }
                ids
            }
        };
    sqlx::query("DELETE FROM monitor_channels WHERE monitor_id = ?")
        .bind(monitor.get())
        .execute(&mut *conn)
        .await?;
    for id in wanted {
        sqlx::query("INSERT INTO monitor_channels (monitor_id, channel_id) VALUES (?, ?)")
            .bind(monitor.get())
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// The channels `monitor` alerts, by ID.
pub(crate) async fn of_monitor(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
) -> sqlx::Result<Vec<ChannelId>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT channel_id FROM monitor_channels WHERE monitor_id = ? ORDER BY channel_id",
    )
    .bind(monitor.get())
    .fetch_all(conn)
    .await?;
    Ok(ids.into_iter().map(ChannelId::new).collect())
}

/// After `node` moved: drops links from monitors of its subtree to channels that are no
/// longer on the monitor's path.
pub(crate) async fn prune_moved(conn: &mut SqliteConnection, node: NodeId) -> sqlx::Result<()> {
    let rows: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "WITH RECURSIVE moved(id) AS (
             SELECT ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN moved ON n.parent_id = moved.id
         )
         SELECT mc.monitor_id, mc.channel_id, m.node_id, c.node_id
         FROM monitor_channels mc
         JOIN monitors m ON m.id = mc.monitor_id
         JOIN channels c ON c.id = mc.channel_id
         WHERE m.node_id IN (SELECT id FROM moved)",
    )
    .bind(node.get())
    .fetch_all(&mut *conn)
    .await?;
    let mut paths: HashMap<i64, Vec<i64>> = HashMap::new();
    for (monitor, channel, monitor_node, channel_node) in rows {
        if let Entry::Vacant(entry) = paths.entry(monitor_node) {
            entry.insert(
                sqlx::query_scalar(concat!(path_cte!(), "SELECT id FROM path"))
                    .bind(monitor_node)
                    .fetch_all(&mut *conn)
                    .await?,
            );
        }
        if !paths[&monitor_node].contains(&channel_node) {
            sqlx::query("DELETE FROM monitor_channels WHERE monitor_id = ? AND channel_id = ?")
                .bind(monitor)
                .bind(channel)
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

async fn record(
    ctx: &mut CommandCtx,
    node: NodeId,
    kind: &str,
    channel: ChannelId,
) -> Result<(), ApiError> {
    ctx.record(&Topic::channels(node), kind, json!({ "channel": channel }))
        .await
        .map(drop)
}

/// A command's answer: the channel as stored now.
async fn answer(
    ctx: &mut CommandCtx,
    key: &SecretKey,
    channel: ChannelId,
) -> Result<ChannelResult, ApiError> {
    Ok(ChannelResult {
        channel: load(ctx.conn(), channel).await?.present(key)?,
    })
}

#[derive(sqlx::FromRow)]
struct DeliveryRow {
    id: i64,
    node_id: i64,
    channel_id: Option<i64>,
    event: String,
    monitor_id: Option<i64>,
    summary: String,
    state: String,
    attempts: i64,
    next_attempt_at: Option<i64>,
    error: Option<String>,
    detail: Option<String>,
    created_at: i64,
    finished_at: Option<i64>,
}

/// `SELECT <every DeliveryRow column> FROM notification_outbox`, for `concat!`.
macro_rules! select_delivery {
    () => {
        "SELECT id, node_id, channel_id, event, monitor_id, summary, state, attempts,
                next_attempt_at, error, detail, created_at, finished_at
         FROM notification_outbox"
    };
}

impl DeliveryRow {
    fn present(self) -> Result<Delivery, ApiError> {
        let text = |value: String| serde_json::Value::String(value);
        Ok(Delivery {
            id: DeliveryId::new(self.id),
            channel: self.channel_id.map(ChannelId::new),
            event: serde_json::from_value::<DeliveryEvent>(text(self.event))
                .map_err(ApiError::internal)?,
            monitor: self.monitor_id.map(MonitorId::new),
            summary: self.summary,
            state: serde_json::from_value::<DeliveryState>(text(self.state))
                .map_err(ApiError::internal)?,
            attempts: self.attempts as u32,
            next_attempt_at: self.next_attempt_at,
            error: self.error,
            detail: self.detail,
            created_at: self.created_at,
            finished_at: self.finished_at,
        })
    }
}

/// Whose deliveries a page lists.
enum Deliveries {
    Channel(ChannelId),
    /// A monitor's, through channels on `nodes`. The provider's error and answer show only
    /// for channels on `managed` nodes: they are channel log text, not alert history.
    Monitor {
        monitor: MonitorId,
        nodes: Vec<NodeId>,
        managed: Vec<NodeId>,
    },
}

/// A page of deliveries, newest first, before `cursor`.
async fn delivery_page(
    conn: &mut SqliteConnection,
    of: Deliveries,
    cursor: Option<&str>,
    limit: Option<u32>,
) -> Result<DeliveryPage, ApiError> {
    let before = match cursor {
        None => i64::MAX,
        Some(cursor) => cursor
            .parse::<i64>()
            .map_err(|_| FieldErrors::one("cursor", FieldErrorCode::InvalidFormat, None))?,
    };
    let limit = limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
    let rows: Vec<DeliveryRow> = match &of {
        Deliveries::Channel(channel) => sqlx::query_as(concat!(
            select_delivery!(),
            " WHERE channel_id = ? AND id < ? ORDER BY id DESC LIMIT ?"
        ))
        .bind(channel.get()),
        Deliveries::Monitor { monitor, nodes, .. } => sqlx::query_as(concat!(
            select_delivery!(),
            " WHERE monitor_id = ? AND node_id IN (SELECT value FROM json_each(?))
                AND id < ? ORDER BY id DESC LIMIT ?"
        ))
        .bind(monitor.get())
        .bind(json!(nodes.iter().map(|node| node.get()).collect::<Vec<_>>()).to_string()),
    }
    .bind(before)
    .bind(i64::from(limit) + 1)
    .fetch_all(conn)
    .await?;
    let more = rows.len() > limit as usize;
    let deliveries: Vec<Delivery> = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| {
            let full = match &of {
                Deliveries::Channel(_) => true,
                Deliveries::Monitor { managed, .. } => managed.contains(&NodeId::new(row.node_id)),
            };
            let delivery = row.present()?;
            Ok::<_, ApiError>(if full {
                delivery
            } else {
                Delivery {
                    error: None,
                    detail: None,
                    ..delivery
                }
            })
        })
        .collect::<Result<_, _>>()?;
    let next_cursor = more
        .then(|| deliveries.last().map(|last| last.id.to_string()))
        .flatten();
    Ok(DeliveryPage {
        deliveries,
        next_cursor,
    })
}

impl ChannelsHandlers for AppState {
    async fn channels_create(
        &self,
        ctx: &mut CommandCtx,
        args: ChannelsCreateArgs,
    ) -> Result<ChannelResult, ApiError> {
        ctx.node_scope(args.node, Capability::ChannelsManage)
            .await?;
        let input = args.channel;
        let name = validate_name("name", &input.name)?;
        let kuma = config::to_kuma(&name, &input.settings, None)?;
        let now = ctx.now();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO channels (node_id, name, provider, config, is_default, active, revision,
                                   created_at, updated_at)
             VALUES (?, ?, ?, x'', ?, ?, 1, ?, ?) RETURNING id",
        )
        .bind(args.node.get())
        .bind(&name)
        .bind(config::provider(&input.settings))
        .bind(input.is_default)
        .bind(input.active)
        .bind(now)
        .bind(now)
        .fetch_one(ctx.conn())
        .await?;
        let channel = ChannelId::new(id);
        let sealed = config::seal(&self.secrets, &config::channel_context(channel), &kuma);
        sqlx::query("UPDATE channels SET config = ? WHERE id = ?")
            .bind(sealed)
            .bind(id)
            .execute(ctx.conn())
            .await?;
        if args.apply_existing {
            apply_existing(ctx.conn(), channel, args.node).await?;
        }
        record(ctx, args.node, "channel.created", channel).await?;
        answer(ctx, &self.secrets, channel).await
    }

    async fn channels_update(
        &self,
        ctx: &mut CommandCtx,
        args: ChannelsUpdateArgs,
    ) -> Result<ChannelResult, ApiError> {
        let row = load(ctx.conn(), args.channel).await?;
        ctx.node_scope(row.node(), Capability::ChannelsManage)
            .await?;
        if let Some(expected) = args.expected_revision
            && expected != row.revision
        {
            return Err(ApiError::stale_revision(row.revision));
        }
        let input = args.input;
        let name = validate_name("name", &input.name)?;
        let stored = row.open(&self.secrets)?;
        let kuma = config::to_kuma(&name, &input.settings, Some(&stored))?;
        let sealed = config::seal(&self.secrets, &config::channel_context(args.channel), &kuma);
        sqlx::query(
            "UPDATE channels SET name = ?, provider = ?, config = ?, is_default = ?, active = ?,
                                 inactive_reason = NULL, revision = revision + 1, updated_at = ?
             WHERE id = ?",
        )
        .bind(&name)
        .bind(config::provider(&input.settings))
        .bind(sealed)
        .bind(input.is_default)
        .bind(input.active)
        .bind(ctx.now())
        .bind(args.channel.get())
        .execute(ctx.conn())
        .await?;
        if args.apply_existing {
            apply_existing(ctx.conn(), args.channel, row.node()).await?;
        }
        record(ctx, row.node(), "channel.updated", args.channel).await?;
        answer(ctx, &self.secrets, args.channel).await
    }

    async fn channels_delete(
        &self,
        ctx: &mut CommandCtx,
        args: ChannelArgs,
    ) -> Result<ChannelDeleted, ApiError> {
        let row = load(ctx.conn(), args.channel).await?;
        ctx.node_scope(row.node(), Capability::ChannelsManage)
            .await?;
        sqlx::query("DELETE FROM channels WHERE id = ?")
            .bind(args.channel.get())
            .execute(ctx.conn())
            .await?;
        record(ctx, row.node(), "channel.deleted", args.channel).await?;
        Ok(ChannelDeleted {
            channel: args.channel,
        })
    }

    async fn channels_test(
        &self,
        ctx: &mut CommandCtx,
        args: ChannelsTestArgs,
    ) -> Result<ChannelTestQueued, ApiError> {
        ctx.node_scope(args.node, Capability::ChannelsManage)
            .await?;
        let stored = match args.channel {
            None => None,
            Some(channel) => {
                let row = load(ctx.conn(), channel)
                    .await
                    .ok()
                    .filter(|row| row.node() == args.node)
                    .ok_or_else(|| {
                        FieldErrors::one("channel", FieldErrorCode::InvalidValue, None)
                    })?;
                Some(row.open(&self.secrets)?)
            }
        };
        let name = validate_name("name", &args.name)?;
        let kuma = config::to_kuma(&name, &args.settings, stored.as_ref())?;
        let now = ctx.now();
        let delivery = match args.sample {
            None if args.monitor.is_some() => {
                return Err(FieldErrors::one(
                    "monitor",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
            None => {
                outbox::queue_test(
                    ctx.conn(),
                    &self.secrets,
                    args.node,
                    args.channel,
                    &kuma,
                    name,
                    now,
                )
                .await?
            }
            Some(sample) => {
                let payload = sample_payload(ctx.conn(), sample, args.node, args.monitor, now)
                    .await?
                    .ok_or_else(|| {
                        FieldErrors::one("monitor", FieldErrorCode::InvalidValue, None)
                    })?;
                queue_sample_test(
                    ctx.conn(),
                    &self.secrets,
                    args.node,
                    args.channel,
                    &kuma,
                    &payload,
                    now,
                )
                .await?
            }
        };
        ctx.record(
            &Topic::channels(args.node),
            "channel.delivery",
            json!({ "channel": args.channel, "delivery": delivery }),
        )
        .await?;
        Ok(ChannelTestQueued { delivery })
    }

    async fn channels_list(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<ChannelList>, ApiError> {
        ctx.node_scope(args.node, Capability::ChannelsManage)
            .await?;
        let rows: Vec<Row> = sqlx::query_as(concat!(
            select_row!(),
            " WHERE c.node_id = ? ORDER BY c.name, c.id"
        ))
        .bind(args.node.get())
        .fetch_all(ctx.conn())
        .await?;
        let channels = rows
            .into_iter()
            .map(|row| row.present(&self.secrets))
            .collect::<Result<_, _>>()?;
        Ok(Snapshot {
            topic: Topic::channels(args.node),
            data: ChannelList { channels },
        })
    }

    async fn channels_available(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<ChannelChoiceList>, ApiError> {
        let scope = ctx.node_scope(args.node, Capability::NodeRead).await?;
        let rows: Vec<(i64, String, String, bool, bool, i64)> = sqlx::query_as(concat!(
            path_cte!(),
            "SELECT c.id, c.name, c.provider, c.is_default, c.active, p.step
             FROM channels c JOIN path p ON p.id = c.node_id
             ORDER BY p.step, c.name, c.id"
        ))
        .bind(scope.node().get())
        .fetch_all(ctx.conn())
        .await?;
        let channels = rows
            .into_iter()
            .map(
                |(id, name, provider, is_default, active, step)| ChannelChoice {
                    id: ChannelId::new(id),
                    name,
                    provider,
                    is_default,
                    active,
                    own: step == 0,
                },
            )
            .collect();
        Ok(Snapshot {
            topic: Topic::channels(args.node),
            data: ChannelChoiceList { channels },
        })
    }

    async fn channels_get(
        &self,
        ctx: &mut QueryCtx,
        args: ChannelArgs,
    ) -> Result<Snapshot<ChannelResult>, ApiError> {
        let row = load(ctx.conn(), args.channel).await?;
        ctx.node_scope(row.node(), Capability::ChannelsManage)
            .await?;
        Ok(Snapshot {
            topic: Topic::channels(row.node()),
            data: ChannelResult {
                channel: row.present(&self.secrets)?,
            },
        })
    }

    async fn channels_deliveries(
        &self,
        ctx: &mut QueryCtx,
        args: DeliveriesArgs,
    ) -> Result<Snapshot<DeliveryPage>, ApiError> {
        let row = load(ctx.conn(), args.channel).await?;
        ctx.node_scope(row.node(), Capability::ChannelsManage)
            .await?;
        let of = Deliveries::Channel(args.channel);
        Ok(Snapshot {
            topic: Topic::channels(row.node()),
            data: delivery_page(ctx.conn(), of, args.cursor.as_deref(), args.limit).await?,
        })
    }

    async fn channels_monitor_deliveries(
        &self,
        ctx: &mut QueryCtx,
        args: MonitorDeliveriesArgs,
    ) -> Result<Snapshot<DeliveryPage>, ApiError> {
        let node: i64 = sqlx::query_scalar("SELECT node_id FROM monitors WHERE id = ?")
            .bind(args.monitor.get())
            .fetch_optional(ctx.conn())
            .await?
            .ok_or_else(ApiError::not_found)?;
        let node = NodeId::new(node);
        let scope = ctx.node_scope(node, Capability::NodeRead).await?;
        // The monitor's channels sit on its node and ancestors; those on nodes the caller
        // can read count.
        let mut nodes = vec![node];
        let mut managed = Vec::new();
        if scope.can(Capability::ChannelsManage) {
            managed.push(node);
        }
        for (ancestor, visible) in scope.ancestors() {
            if !visible {
                continue;
            }
            nodes.push(ancestor);
            if ctx
                .node_scope(ancestor, Capability::ChannelsManage)
                .await
                .is_ok()
            {
                managed.push(ancestor);
            }
        }
        let of = Deliveries::Monitor {
            monitor: args.monitor,
            nodes,
            managed,
        };
        Ok(Snapshot {
            topic: Topic::channels(node),
            data: delivery_page(ctx.conn(), of, args.cursor.as_deref(), args.limit).await?,
        })
    }

    async fn channels_delivery(
        &self,
        ctx: &mut QueryCtx,
        args: DeliveryArgs,
    ) -> Result<Snapshot<DeliveryResult>, ApiError> {
        let row: DeliveryRow = sqlx::query_as(concat!(select_delivery!(), " WHERE id = ?"))
            .bind(args.delivery.get())
            .fetch_optional(ctx.conn())
            .await?
            .ok_or_else(ApiError::not_found)?;
        let node = NodeId::new(row.node_id);
        ctx.node_scope(node, Capability::ChannelsManage).await?;
        Ok(Snapshot {
            topic: Topic::channels(node),
            data: DeliveryResult {
                delivery: row.present()?,
            },
        })
    }
}
