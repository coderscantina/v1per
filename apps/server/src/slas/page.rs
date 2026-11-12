//! The SLA section of a status page snapshot (decision 0012): per selected SLA the running
//! period and the last 12 frozen ones, as targets and aggregates. Frozen periods are read as
//! stored; the running one is computed at most once a minute per SLA and shared by every page,
//! audience and locale.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::SqliteConnection;
use v1per_protocol::ids::{PageComponentId, PageId, SlaId};
use v1per_protocol::operations::slas::{Sla, SlaClockSummary, SlaPeriod, SlaTimeTarget};
use v1per_protocol::status_page::{
    PageSla, PageSlaAvailability, PageSlaClock, PageSlaLatency, PageSlaMonitor, PageSlaPeriod,
    PageSlaTargets,
};
use v1per_sla::Period;

use super::{DATE, load, period, present};
use crate::error::ApiError;
use crate::state::AppState;

/// Closed periods a page lists.
const HISTORY: i64 = 12;
/// How long a computed running period serves pages.
const RUNNING_TTL_MS: i64 = 60_000;

/// Running periods by SLA, valid for their SLA revision and period for [`RUNNING_TTL_MS`].
#[derive(Default)]
pub struct RunningCache(Mutex<HashMap<i64, Running>>);

struct Running {
    revision: i64,
    computed_at: i64,
    period: Arc<SlaPeriod>,
}

/// The running period of `sla`, from the cache while it is current.
async fn running(
    state: &AppState,
    conn: &mut SqliteConnection,
    sla: &Sla,
    due: &Period,
    now: i64,
) -> Result<Arc<SlaPeriod>, ApiError> {
    let start = due.first_day.format(DATE).to_string();
    {
        let cache = state.running_slas.0.lock().expect("running SLA cache");
        if let Some(entry) = cache.get(&sla.id.get())
            && entry.revision == sla.revision
            && entry.period.start == start
            && (0..RUNNING_TTL_MS).contains(&(now - entry.computed_at))
        {
            return Ok(entry.period.clone());
        }
    }
    let period = Arc::new(period::compute(conn, &state.storage, sla, due, now).await?);
    let mut cache = state.running_slas.0.lock().expect("running SLA cache");
    cache.retain(|_, entry| now - entry.computed_at < RUNNING_TTL_MS);
    cache.insert(
        sla.id.get(),
        Running {
            revision: sla.revision,
            computed_at: now,
            period: period.clone(),
        },
    );
    Ok(period)
}

/// The SLAs `page` shows, in its order. `shown` maps the monitors of the components the
/// audience sees to those components; per-monitor rows are for the `internal` audience only.
pub(crate) async fn for_page(
    state: &AppState,
    page: PageId,
    shown: &HashMap<i64, PageComponentId>,
    internal: bool,
    now: i64,
) -> Result<Vec<PageSla>, ApiError> {
    let mut conn = state.db.reader.acquire().await?;
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT sla_id FROM page_slas WHERE page_id = ? ORDER BY position")
            .bind(page.get())
            .fetch_all(&mut *conn)
            .await?;
    let mut slas = Vec::with_capacity(ids.len());
    for id in ids {
        let row = match load(&mut conn, SlaId::new(id)).await {
            Ok(row) => row,
            // Deleted since the selection was read.
            Err(error) if error == ApiError::not_found() => continue,
            Err(error) => return Err(error),
        };
        let sla = present(&mut conn, row).await?;
        let calendar = period::calendar(&sla.terms)?;
        let due = period::resolve(&calendar, None, now)?;
        let current = if due.starts_at.timestamp_millis() <= now {
            let period = running(state, &mut conn, &sla, &due, now).await?;
            Some(project(&period, false, shown, internal))
        } else {
            None
        };
        let frozen: Vec<String> = sqlx::query_scalar(
            "SELECT result FROM sla_periods WHERE sla_id = ? AND start < ?
             ORDER BY start DESC LIMIT ?",
        )
        .bind(sla.id.get())
        .bind(due.first_day.format(DATE).to_string())
        .bind(HISTORY)
        .fetch_all(&mut *conn)
        .await?;
        let history = frozen
            .iter()
            .map(|result| {
                let period: SlaPeriod = serde_json::from_str(result).map_err(ApiError::internal)?;
                Ok(project(&period, true, shown, internal))
            })
            .collect::<Result<_, ApiError>>()?;
        slas.push(PageSla {
            name: sla.terms.name,
            timezone: sla.terms.timezone,
            period: sla.terms.period,
            combine: sla.terms.combine,
            current,
            history,
        });
    }
    Ok(slas)
}

/// `period` as a page shows it: targets from the terms it was computed with, aggregates, and
/// for `internal` the covered monitors in `shown`.
fn project(
    period: &SlaPeriod,
    closed: bool,
    shown: &HashMap<i64, PageComponentId>,
    internal: bool,
) -> PageSlaPeriod {
    let terms = &period.terms;
    let clock = |summary: Option<SlaClockSummary>, pick: fn(SlaTimeTarget) -> Option<u32>| {
        summary.map(|summary| PageSlaClock {
            targets: PageSlaTargets {
                major_outage: pick(terms.times.major_outage),
                partial_outage: pick(terms.times.partial_outage),
                degraded: pick(terms.times.degraded),
            },
            outcome: summary.outcome,
            counted: summary.counted,
            met: summary.met,
            within_target: (summary.counted > 0)
                .then(|| f64::from(summary.met) / f64::from(summary.counted)),
            average_ms: summary.average_ms,
        })
    };
    let availability = period
        .availability
        .zip(terms.availability)
        .map(|(standing, target)| PageSlaAvailability {
            target,
            outcome: standing.outcome,
            actual: standing.actual,
            budget_used: standing.budget_used,
        });
    let latency = period
        .latency
        .zip(terms.latency_ms)
        .map(|(outcome, target_ms)| PageSlaLatency {
            target_ms,
            outcome,
            actual_ms: period
                .monitors
                .iter()
                .filter_map(|m| m.latency_ms)
                .max_by(f64::total_cmp),
        });
    let response = clock(period.response, |t| t.response_minutes);
    let resolution = clock(period.resolution, |t| t.resolution_minutes);
    let outcome = period::combined(
        [
            availability.map(|a| a.outcome),
            latency.map(|l| l.outcome),
            response.map(|c| c.outcome),
            resolution.map(|c| c.outcome),
        ]
        .into_iter()
        .flatten(),
        closed,
    );
    let monitors = if internal {
        period
            .monitors
            .iter()
            .filter_map(|m| {
                Some(PageSlaMonitor {
                    component: *shown.get(&m.monitor.get())?,
                    availability: m.availability,
                    availability_outcome: m.availability_outcome,
                    latency_ms: m.latency_ms,
                    latency_outcome: m.latency_outcome,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    PageSlaPeriod {
        start: period.start.clone(),
        starts_at: period.starts_at,
        ends_at: period.ends_at,
        outcome,
        availability,
        latency,
        response,
        resolution,
        monitors,
    }
}
