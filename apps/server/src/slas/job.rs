//! The SLA background work: tallies closed days of `any_down` SLAs, freezes closed periods
//! from each SLA's start date on, and queues the warnings and breaches of the running and the
//! newly frozen periods. Without SLAs a tick is one query. Every write is keyed and inserted
//! once, so a tick interrupted by a restart is repeated at the next start and sends nothing
//! twice.

use std::collections::HashSet;

use chrono::NaiveDate;
use serde_json::json;
use sqlx::SqliteConnection;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use v1per_notify::{Language, SlaAlert, SlaAlertKind, SlaIncident};
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{MonitorId, PageId, SlaId};
use v1per_protocol::operations::incidents::Impact;
use v1per_protocol::operations::slas::{Sla, SlaCombine, SlaPeriod, SlaTimeTarget};
use v1per_sla::{self as rules, PeriodCalendar};

use super::period::{self, FREEZE_DELAY_MS, Signal};
use super::{DATE, load, present};
use crate::error::ApiError;
use crate::incidents::record_tree;
use crate::notifications::queue_sla;
use crate::state::AppState;

/// Between ticks with nothing left to do.
const IDLE_MS: i64 = 5 * 60_000;
/// Between ticks while a backfill has work left.
const BUSY_MS: i64 = 1_000;
/// After a failed tick.
const RETRY_MS: i64 = 60_000;
/// Days tallied per tick, each one minute rollup read.
const TALLIES_PER_TICK: usize = 31;
/// How long after its freeze a period is still checked for clocks that ran out late.
const LATE_CLOCK_DAYS: i64 = 90;
/// Periods frozen per tick, each a full period computation.
const FREEZES_PER_TICK: usize = 3;

fn failed(error: ApiError) -> anyhow::Error {
    anyhow::anyhow!("{error:?}")
}

/// Ticks until `stop`. A change to an SLA wakes it early.
pub async fn run(state: AppState, mut stop: watch::Receiver<bool>) {
    let (mut wake, _) = state.hub.subscribe();
    while !*stop.borrow() {
        let now = state.clock.now_ms();
        let until = match catch_up(&state, now).await {
            Ok(true) => now + BUSY_MS,
            Ok(false) => now + IDLE_MS,
            Err(error) => {
                tracing::error!(%error, "SLA tallies and freezing failed; retrying");
                now + RETRY_MS
            }
        };
        loop {
            tokio::select! {
                woken = wake.recv() => match woken {
                    Ok(topic) if topic.starts_with("slas:") => break,
                    Ok(_) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                },
                _ = state.clock.sleep_until(until) => break,
                _ = stop.changed() => break,
            }
        }
    }
}

/// One bounded tick at `now`: tallies the closed days and then freezes the closed periods that
/// have none yet, oldest first. A period of an SLA with days left to tally waits for them.
/// Once nothing is left, checks the running period of every SLA for alerts. Returns whether
/// work is left.
pub async fn catch_up(state: &AppState, now: i64) -> anyhow::Result<bool> {
    let slas: Vec<i64> = sqlx::query_scalar("SELECT id FROM slas ORDER BY id")
        .fetch_all(&state.db.reader)
        .await?;
    let mut tallies = TALLIES_PER_TICK;
    let mut freezes = FREEZES_PER_TICK;
    let mut more = false;
    let mut running = Vec::with_capacity(slas.len());
    for id in slas {
        let sla = {
            let mut conn = state.db.reader.acquire().await?;
            match load(&mut conn, SlaId::new(id)).await {
                Ok(row) => present(&mut conn, row).await.map_err(failed)?,
                // Deleted since the list was read.
                Err(error) if error == ApiError::not_found() => continue,
                Err(error) => return Err(failed(error)),
            }
        };
        let calendar = period::calendar(&sla.terms).map_err(failed)?;
        if sla.terms.combine == SlaCombine::AnyDown
            && !tally(state, &sla, &calendar, now, &mut tallies).await?
        {
            more = true;
            continue;
        }
        if !freeze(state, &sla, &calendar, now, &mut freezes).await? {
            more = true;
        }
        running.push((sla, calendar));
    }
    // During a backfill the running periods wait, so busy ticks do not recompute them.
    if !more {
        for (sla, calendar) in running {
            check_running(state, &sla, &calendar, now).await?;
            check_late_clocks(state, &sla, &calendar, now).await?;
        }
    }
    Ok(more)
}

/// Queues the alerts of the period running at `now`. Latency is checked at close only.
async fn check_running(
    state: &AppState,
    sla: &Sla,
    calendar: &PeriodCalendar,
    now: i64,
) -> anyhow::Result<()> {
    let due = period::resolve(calendar, None, now).map_err(failed)?;
    if due.starts_at.timestamp_millis() > now {
        return Ok(());
    }
    let result = {
        let mut conn = state.db.reader.acquire().await?;
        period::compute(&mut conn, &state.storage, sla, &due, now)
            .await
            .map_err(failed)?
    };
    let signals: Vec<Signal> = period::signals(&result)
        .into_iter()
        .filter(|signal| signal.kind != "sla_latency_breach")
        .collect();
    if signals.is_empty() {
        return Ok(());
    }
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let topics = alert(&mut tx, sla, &result, &signals, now).await?;
    tx.commit().await?;
    publish(state, &topics);
    Ok(())
}

/// Queues the missed response and resolution clocks of incidents that were still open when
/// their period froze. Periods frozen within [`LATE_CLOCK_DAYS`] are checked, and not those
/// that closed before the SLA existed.
async fn check_late_clocks(
    state: &AppState,
    sla: &Sla,
    calendar: &PeriodCalendar,
    now: i64,
) -> anyhow::Result<()> {
    let frozen: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT starts_at, frozen_at FROM sla_periods
         WHERE sla_id = ? AND frozen_at > ? AND ends_at > ? ORDER BY starts_at",
    )
    .bind(sla.id.get())
    .bind(now - LATE_CLOCK_DAYS * 86_400_000)
    .bind(sla.created_at)
    .fetch_all(&state.db.reader)
    .await?;
    for (starts_at, frozen_at) in frozen {
        let Some(due) = calendar.containing(period::instant(starts_at)) else {
            continue;
        };
        let result = {
            let mut conn = state.db.reader.acquire().await?;
            period::late_clocks(&mut conn, sla, &due, frozen_at, now)
                .await
                .map_err(failed)?
        };
        let signals = period::signals(&result);
        if signals.is_empty() {
            continue;
        }
        let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let topics = alert(&mut tx, sla, &result, &signals, now).await?;
        tx.commit().await?;
        publish(state, &topics);
    }
    Ok(())
}

fn publish(state: &AppState, topics: &[Topic]) {
    for topic in topics {
        state.hub.publish(topic);
    }
    if topics
        .iter()
        .any(|topic| topic.as_str().starts_with("channels:"))
    {
        state.outbox.notify_one();
    }
}

/// Records `signals` of `period` in `sla_alerts` and queues an outbox row per active SLA
/// channel for each one not recorded before. Returns the topics to wake.
async fn alert(
    conn: &mut SqliteConnection,
    sla: &Sla,
    period: &SlaPeriod,
    signals: &[Signal],
    now: i64,
) -> anyhow::Result<Vec<Topic>> {
    let mut topics = Vec::new();
    let mut heading: Option<(Language, String, String)> = None;
    for signal in signals {
        let new = sqlx::query(
            "INSERT OR IGNORE INTO sla_alerts (sla_id, start, kind, subject, sent_at)
             SELECT id, ?, ?, ?, ? FROM slas WHERE id = ?",
        )
        .bind(&period.start)
        .bind(signal.kind)
        .bind(signal.subject)
        .bind(now)
        .bind(sla.id.get())
        .execute(&mut *conn)
        .await?
        .rows_affected()
            == 1;
        let Some((monitor, kind)) = new.then(|| describe(period, *signal)).flatten() else {
            continue;
        };
        if heading.is_none() {
            heading = Some((
                language(conn, sla.id).await?,
                node_name(conn, sla).await?,
                last_day(period)?,
            ));
        }
        let (language, node, last_day) = heading.clone().expect("set above");
        let alert = SlaAlert {
            language,
            sla: sla.terms.name.clone(),
            node,
            first_day: period.start.clone(),
            last_day,
            kind,
        };
        for topic in queue_sla(conn, sla.id, monitor, alert, now).await? {
            if !topics.contains(&topic) {
                topics.push(topic);
            }
        }
    }
    Ok(topics)
}

/// The language of an SLA's alerts: the default locale of the first page showing it, English
/// when no page does. Channels carry no language of their own.
async fn language(conn: &mut SqliteConnection, sla: SlaId) -> sqlx::Result<Language> {
    let locale: Option<String> = sqlx::query_scalar(
        "SELECT p.default_locale FROM page_slas ps JOIN pages p ON p.id = ps.page_id
         WHERE ps.sla_id = ? ORDER BY p.id LIMIT 1",
    )
    .bind(sla.get())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match locale.as_deref() {
        Some("de") => Language::De,
        _ => Language::En,
    })
}

async fn node_name(conn: &mut SqliteConnection, sla: &Sla) -> sqlx::Result<String> {
    sqlx::query_scalar("SELECT name FROM nodes WHERE id = ?")
        .bind(sla.node.get())
        .fetch_one(&mut *conn)
        .await
}

/// The period's last day in the SLA's timezone.
fn last_day(period: &SlaPeriod) -> anyhow::Result<String> {
    let zone: chrono_tz::Tz = period.terms.timezone.parse().map_err(anyhow::Error::msg)?;
    let last = period::instant(period.ends_at - 1).with_timezone(&zone);
    Ok(last.date_naive().format(DATE).to_string())
}

/// What `signal` says about `period`, and the monitor of a per-monitor breach. `None` when the
/// standing it names is gone, which a signal from the same period never is.
fn describe(period: &SlaPeriod, signal: Signal) -> Option<(Option<MonitorId>, SlaAlertKind)> {
    let terms = &period.terms;
    let monitor = || {
        period
            .monitors
            .iter()
            .find(|m| m.monitor.get() == signal.subject)
    };
    let incident = || {
        period
            .incidents
            .iter()
            .find(|i| i.incident.get() == signal.subject)
    };
    let times = |impact: Impact| match impact {
        Impact::MajorOutage => terms.times.major_outage,
        Impact::PartialOutage => terms.times.partial_outage,
        Impact::Degraded => terms.times.degraded,
        Impact::Operational => SlaTimeTarget::default(),
    };
    let kind = match signal.kind {
        "sla_budget_warning" => {
            let availability = period.availability?;
            SlaAlertKind::BudgetWarning {
                used: availability.budget_used?,
                actual: availability.actual,
                target: terms.availability?,
            }
        }
        "sla_availability_breach" if signal.subject == 0 => SlaAlertKind::AvailabilityBreach {
            monitor: None,
            actual: period.availability?.actual,
            target: terms.availability?,
        },
        "sla_availability_breach" => {
            let standing = monitor()?;
            return Some((
                Some(standing.monitor),
                SlaAlertKind::AvailabilityBreach {
                    monitor: Some(standing.name.clone()),
                    actual: standing.availability,
                    target: terms.availability?,
                },
            ));
        }
        "sla_latency_breach" => {
            let standing = monitor()?;
            return Some((
                Some(standing.monitor),
                SlaAlertKind::LatencyBreach {
                    monitor: standing.name.clone(),
                    actual_ms: standing.latency_ms,
                    target_ms: terms.latency_ms?,
                },
            ));
        }
        "sla_response_missed" => {
            let standing = incident()?;
            SlaAlertKind::ResponseMissed {
                incident: SlaIncident {
                    id: standing.incident.get(),
                    title: standing.title.clone(),
                },
                elapsed_ms: standing.response.elapsed_ms,
                target_minutes: times(standing.impact).response_minutes?,
            }
        }
        "sla_resolution_missed" => {
            let standing = incident()?;
            SlaAlertKind::ResolutionMissed {
                incident: SlaIncident {
                    id: standing.incident.get(),
                    title: standing.title.clone(),
                },
                elapsed_ms: standing.resolution.elapsed_ms,
                target_minutes: times(standing.impact).resolution_minutes?,
            }
        }
        _ => return None,
    };
    Some((None, kind))
}

/// Writes the missing tallies of days that closed at least [`FREEZE_DELAY_MS`] ago, within
/// `budget`. A day the minute rollups do not cover is stored with 0 counted minutes, which the
/// period read takes as no data. Returns whether every closed day is tallied.
async fn tally(
    state: &AppState,
    sla: &Sla,
    calendar: &PeriodCalendar,
    now: i64,
    budget: &mut usize,
) -> anyhow::Result<bool> {
    let done: HashSet<String> = sqlx::query_scalar("SELECT day FROM sla_tallies WHERE sla_id = ?")
        .bind(sla.id.get())
        .fetch_all(&state.db.reader)
        .await?
        .into_iter()
        .collect();
    let monitors = period::storage_ids(&sla.terms.monitors);
    let mut day: NaiveDate = calendar.start;
    loop {
        let from = rules::day_start(calendar.zone, day).timestamp_millis();
        let next = day.succ_opt().expect("date in range");
        let to = rules::day_start(calendar.zone, next).timestamp_millis();
        if to + FREEZE_DELAY_MS > now {
            return Ok(true);
        }
        let key = day.format(DATE).to_string();
        if !done.contains(&key) {
            if *budget == 0 {
                return Ok(false);
            }
            *budget -= 1;
            let minutes = state
                .storage
                .down_minutes(&monitors, from, to)
                .await
                .map_err(|error| failed(period::read_failed(error)))?
                .value;
            let (counted, down) = if minutes.covered {
                (minutes.counted, minutes.down)
            } else {
                (0, 0)
            };
            sqlx::query(
                "INSERT OR IGNORE INTO sla_tallies (sla_id, day, counted_minutes, down_minutes,
                     written_at)
                 SELECT id, ?, ?, ?, ? FROM slas WHERE id = ?",
            )
            .bind(&key)
            .bind(counted as i64)
            .bind(down as i64)
            .bind(now)
            .bind(sla.id.get())
            .execute(&state.db.writer)
            .await?;
        }
        day = next;
    }
}

/// Freezes the periods that ended at least [`FREEZE_DELAY_MS`] ago and have no stored result,
/// within `budget`, and announces each with `sla.period_frozen` and to the pages showing the
/// SLA. A period that closed after the SLA was created also sends its alerts; older ones are
/// history, not news. Returns whether none is left.
async fn freeze(
    state: &AppState,
    sla: &Sla,
    calendar: &PeriodCalendar,
    now: i64,
    budget: &mut usize,
) -> anyhow::Result<bool> {
    let frozen: HashSet<String> =
        sqlx::query_scalar("SELECT start FROM sla_periods WHERE sla_id = ?")
            .bind(sla.id.get())
            .fetch_all(&state.db.reader)
            .await?
            .into_iter()
            .collect();
    let due = calendar
        .through(period::instant(now))
        .into_iter()
        .filter(|p| p.ends_at.timestamp_millis() + FREEZE_DELAY_MS <= now)
        .filter(|p| !frozen.contains(&p.first_day.format(DATE).to_string()));
    for due in due {
        if *budget == 0 {
            return Ok(false);
        }
        *budget -= 1;
        let mut result = {
            let mut conn = state.db.reader.acquire().await?;
            period::compute(&mut conn, &state.storage, sla, &due, now)
                .await
                .map_err(failed)?
        };
        result.frozen_at = Some(now);
        let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let stored = sqlx::query(
            "INSERT OR IGNORE INTO sla_periods (sla_id, start, starts_at, ends_at, result,
                 frozen_at)
             SELECT id, ?, ?, ?, ?, ? FROM slas WHERE id = ?",
        )
        .bind(&result.start)
        .bind(result.starts_at)
        .bind(result.ends_at)
        .bind(serde_json::to_string(&result)?)
        .bind(now)
        .bind(sla.id.get())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let mut topics = Vec::new();
        if stored == 1 {
            topics = record_tree(
                &mut tx,
                Topic::slas,
                sla.node,
                "sla.period_frozen",
                &json!({ "sla": sla.id, "start": result.start }),
                now,
            )
            .await?;
            let pages: Vec<i64> =
                sqlx::query_scalar("SELECT page_id FROM page_slas WHERE sla_id = ?")
                    .bind(sla.id.get())
                    .fetch_all(&mut *tx)
                    .await?;
            for page in pages {
                let topic = Topic::page(PageId::new(page));
                crate::events::record(&mut tx, &topic, "page.status_changed", &json!({}), now)
                    .await?;
                topics.push(topic);
            }
            if result.ends_at > sla.created_at {
                let signals = period::signals(&result);
                topics.extend(alert(&mut tx, sla, &result, &signals, now).await?);
            }
        }
        tx.commit().await?;
        publish(state, &topics);
    }
    Ok(true)
}
