//! `components.*`: static components that incidents name and status pages place.

use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{ComponentId, NodeId};
use v1per_protocol::operations::components::{
    Component, ComponentArgs, ComponentDeleted, ComponentInput, ComponentList, ComponentResult,
    ComponentsCreateArgs, ComponentsHandlers, ComponentsListArgs, ComponentsUpdateArgs,
};
use v1per_protocol::registry::Snapshot;

use super::{record_tree_ctx, text};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::ApiError;
use crate::pages::{self, Change};
use crate::state::AppState;
use v1per_protocol::limits::NAME_MAX_CHARS;

const DESCRIPTION_MAX: usize = 1_000;

type Row = (i64, i64, String, String, i64, i64);

fn present((id, node, name, description, created_at, updated_at): Row) -> Component {
    Component {
        id: ComponentId::new(id),
        node: NodeId::new(node),
        name,
        description,
        created_at,
        updated_at,
    }
}

async fn load(conn: &mut SqliteConnection, component: ComponentId) -> Result<Row, ApiError> {
    sqlx::query_as(
        "SELECT id, node_id, name, description, created_at, updated_at
         FROM components WHERE id = ?",
    )
    .bind(component.get())
    .fetch_optional(conn)
    .await?
    .ok_or_else(ApiError::not_found)
}

fn validate(input: &ComponentInput) -> Result<(String, String), ApiError> {
    Ok((
        text("name", &input.name, true, NAME_MAX_CHARS)?,
        text("description", &input.description, false, DESCRIPTION_MAX)?,
    ))
}

async fn record(
    ctx: &mut CommandCtx,
    node: NodeId,
    kind: &str,
    component: ComponentId,
) -> Result<(), ApiError> {
    pages::touch_in(ctx, Change::Component(component)).await?;
    record_tree_ctx(
        ctx,
        Topic::components,
        node,
        kind,
        json!({ "component": component }),
    )
    .await
}

impl ComponentsHandlers for AppState {
    async fn components_create(
        &self,
        ctx: &mut CommandCtx,
        args: ComponentsCreateArgs,
    ) -> Result<ComponentResult, ApiError> {
        ctx.node_scope(args.node, Capability::PagesManage).await?;
        let (name, description) = validate(&args.component)?;
        let now = ctx.now();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO components (node_id, name, description, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(args.node.get())
        .bind(name)
        .bind(description)
        .bind(now)
        .bind(now)
        .fetch_one(ctx.conn())
        .await?;
        let component = ComponentId::new(id);
        record(ctx, args.node, "component.created", component).await?;
        Ok(ComponentResult {
            component: present(load(ctx.conn(), component).await?),
        })
    }

    async fn components_update(
        &self,
        ctx: &mut CommandCtx,
        args: ComponentsUpdateArgs,
    ) -> Result<ComponentResult, ApiError> {
        let node = NodeId::new(load(ctx.conn(), args.component).await?.1);
        ctx.node_scope(node, Capability::PagesManage).await?;
        let (name, description) = validate(&args.input)?;
        sqlx::query("UPDATE components SET name = ?, description = ?, updated_at = ? WHERE id = ?")
            .bind(name)
            .bind(description)
            .bind(ctx.now())
            .bind(args.component.get())
            .execute(ctx.conn())
            .await?;
        record(ctx, node, "component.updated", args.component).await?;
        Ok(ComponentResult {
            component: present(load(ctx.conn(), args.component).await?),
        })
    }

    async fn components_delete(
        &self,
        ctx: &mut CommandCtx,
        args: ComponentArgs,
    ) -> Result<ComponentDeleted, ApiError> {
        let node = NodeId::new(load(ctx.conn(), args.component).await?.1);
        ctx.node_scope(node, Capability::PagesManage).await?;
        pages::touch_in(ctx, Change::Component(args.component)).await?;
        sqlx::query("DELETE FROM components WHERE id = ?")
            .bind(args.component.get())
            .execute(ctx.conn())
            .await?;
        record(ctx, node, "component.deleted", args.component).await?;
        Ok(ComponentDeleted {
            component: args.component,
        })
    }

    async fn components_list(
        &self,
        ctx: &mut QueryCtx,
        args: ComponentsListArgs,
    ) -> Result<Snapshot<ComponentList>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        let rows: Vec<Row> = sqlx::query_as(
            "WITH RECURSIVE below(id) AS (
                 SELECT ?1
                 UNION ALL
                 SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id WHERE ?2
             )
             SELECT id, node_id, name, description, created_at, updated_at FROM components
             WHERE node_id IN (SELECT id FROM below) ORDER BY name, id",
        )
        .bind(args.node.get())
        .bind(args.subtree)
        .fetch_all(ctx.conn())
        .await?;
        Ok(Snapshot {
            topic: Topic::components(args.node),
            data: ComponentList {
                components: rows.into_iter().map(present).collect(),
            },
        })
    }
}
