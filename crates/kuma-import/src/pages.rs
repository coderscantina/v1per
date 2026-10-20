//! Status pages with their groups, components and domains, and Kuma's page banners.

use std::collections::BTreeSet;

use rusqlite::Connection;
use v1per_checks::Secret;

use crate::history::parse_time;
use crate::plan::{
    ComponentPlan, IncidentId, IncidentPlan, InventoryKind, MonitorId, PageGroupId, PageGroupPlan,
    StatusPageId, StatusPagePlan,
};
use crate::planner::{Ledger, PlanError};
use crate::record::{Record, read_table};

const ANALYTICS: [&str; 3] = ["analytics_type", "analytics_id", "analytics_script_url"];

pub(crate) fn pages(
    conn: &Connection,
    monitor_ids: &BTreeSet<i64>,
    ledger: &mut Ledger,
) -> Result<(Vec<StatusPagePlan>, Vec<IncidentPlan>), PlanError> {
    let rows = read_table(conn, "status_page")?;
    let page_ids: BTreeSet<i64> = rows.iter().map(Record::id).collect();
    let cname_rows = read_table(conn, "status_page_cname")?;
    let cname_ids: BTreeSet<i64> = cname_rows.iter().map(Record::id).collect();
    let mut domains = ledger.links(&cname_rows, "status_page_id", &page_ids, "id", &cname_ids);

    let mut group_rows = read_table(conn, "group")?;
    group_rows.sort_by_key(|row| (row.int("weight").unwrap_or_default(), row.id()));
    let group_ids: BTreeSet<i64> = group_rows.iter().map(Record::id).collect();
    let mut groups = ledger.links(&group_rows, "status_page_id", &page_ids, "id", &group_ids);

    let mut member_rows = read_table(conn, "monitor_group")?;
    member_rows.sort_by_key(|row| (row.int("weight").unwrap_or_default(), row.id()));
    let mut members = ledger.links(
        &member_rows,
        "group_id",
        &group_ids,
        "monitor_id",
        monitor_ids,
    );
    // Members of groups that were just inventoried go with them.
    let mapped_groups: BTreeSet<i64> = groups.values().flatten().map(|row| row.id()).collect();
    for (group, links) in members.extract_if(.., |group, _| !mapped_groups.contains(group)) {
        for link in links {
            ledger.add(
                InventoryKind::PageComponent,
                format!("group {group}"),
                "its status page group links a deleted status page",
                link,
            );
        }
    }

    let mut pages = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = row.id();
        let slug = row.text("slug").unwrap_or_default();
        if row.filled("custom_css").is_some() {
            ledger.add_source(
                InventoryKind::CustomCss,
                &slug,
                "custom CSS is not applied to V1Per pages",
                row.source_of(&["slug", "custom_css"]),
            );
        }
        if ANALYTICS.iter().any(|column| row.filled(column).is_some()) {
            let mut columns = ANALYTICS.to_vec();
            columns.push("slug");
            ledger.add_source(
                InventoryKind::Analytics,
                &slug,
                "analytics scripts are not added to V1Per pages",
                row.source_of(&columns),
            );
        }
        let password = row.filled("password").map(Secret::new);
        ledger.secrets.page_passwords += u64::from(password.is_some());
        pages.push(StatusPagePlan {
            id: StatusPageId(id),
            slug,
            title: row.text("title").unwrap_or_default(),
            description: row.filled("description"),
            icon: row.text("icon").unwrap_or_default(),
            theme: row.text("theme").unwrap_or_default(),
            published: row.flag("published"),
            search_engine_index: row.flag("search_engine_index"),
            show_tags: row.flag("show_tags"),
            show_powered_by: row.flag("show_powered_by"),
            show_certificate_expiry: row.flag("show_certificate_expiry"),
            show_only_last_heartbeat: row.flag("show_only_last_heartbeat"),
            footer_text: row.filled("footer_text"),
            auto_refresh_interval: row
                .int("auto_refresh_interval")
                .and_then(|seconds| u32::try_from(seconds).ok()),
            rss_title: row.filled("rss_title"),
            password,
            domains: domains
                .remove(&id)
                .unwrap_or_default()
                .iter()
                .filter_map(|link| link.filled("domain"))
                .collect(),
            groups: groups
                .remove(&id)
                .unwrap_or_default()
                .iter()
                .map(|group| PageGroupPlan {
                    id: PageGroupId(group.id()),
                    name: group.text("name").unwrap_or_default(),
                    components: members
                        .remove(&group.id())
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|link| {
                            Some(ComponentPlan {
                                monitor: MonitorId(link.int("monitor_id")?),
                                send_url: link.flag("send_url"),
                                custom_url: link.filled("custom_url"),
                            })
                        })
                        .collect(),
                })
                .collect(),
        });
    }

    let incidents = read_table(conn, "incident")?
        .iter()
        .map(|row| {
            let id = row.id();
            let time = |column| row.text(column).as_deref().and_then(parse_time);
            Ok(IncidentPlan {
                id: IncidentId(id),
                page: row
                    .int("status_page_id")
                    .filter(|page| page_ids.contains(page))
                    .map(StatusPageId),
                title: row.text("title").unwrap_or_default(),
                content: row.text("content").unwrap_or_default(),
                style: row.text("style").unwrap_or_default(),
                created_ms: time("created_date").ok_or_else(|| PlanError::InvalidSource {
                    table: "incident",
                    id,
                    reason: "created_date is not a date".to_owned(),
                })?,
                updated_ms: time("last_updated_date"),
                // Kuma's page shows banners with `pin = 1 AND active = 1`.
                resolved: !(row.flag("pin") && row.flag("active")),
            })
        })
        .collect::<Result<_, PlanError>>()?;
    Ok((pages, incidents))
}
