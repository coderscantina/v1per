//! The SLA background work: tallies closed days of `any_down` SLAs and freezes closed periods,
//! from each SLA's start date on. Without SLAs a tick is one query. Every write is keyed and
//! inserted once, so a tick interrupted by a restart is repeated at the next start.

use std::collections::HashSet;

use chrono::NaiveDate;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::SlaId;
use v1per_protocol::operations::slas::{Sla, SlaCombine};
use v1per_sla::{self as rules, PeriodCalendar};

use super::period::{self, FREEZE_DELAY_MS};
use super::{DATE, load, present};
use crate::error::ApiError;
use crate::incidents::record_tree;
use crate::state::AppState;

/// Between ticks with nothing left to do.
const IDLE_MS: i64 = 5 * 60_000;
/// Between ticks while a backfill has work left.
const BUSY_MS: i64 = 1_000;
/// After a failed tick.
const RETRY_MS: i64 = 60_000;
/// Days tallied per tick, each one minute rollup read.
const TALLIES_PER_TICK: usize = 31;
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
/// Returns whether work is left.
pub async fn catch_up(state: &AppState, now: i64) -> anyhow::Result<bool> {
    let slas: Vec<i64> = sqlx::query_scalar("SELECT id FROM slas ORDER BY id")
        .fetch_all(&state.db.reader)
        .await?;
    let mut tallies = TALLIES_PER_TICK;
    let mut freezes = FREEZES_PER_TICK;
    let mut more = false;
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
    }
    Ok(more)
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
/// within `budget`, and announces each with `sla.period_frozen`. Returns whether none is left.
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
        let topics = if stored == 1 {
            record_tree(
                &mut tx,
                Topic::slas,
                sla.node,
                "sla.period_frozen",
                &json!({ "sla": sla.id, "start": result.start }),
                now,
            )
            .await?
        } else {
            Vec::new()
        };
        tx.commit().await?;
        for topic in &topics {
            state.hub.publish(topic);
        }
    }
    Ok(true)
}
