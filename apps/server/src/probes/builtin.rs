//! The built-in probe: runs the checks assigned to it and the server's own rounds for push,
//! group and manual monitors and for monitors under maintenance, one task per monitor on the runtime's workers. The tasks are
//! rebuilt whenever the assignment version moves.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast::error::RecvError, watch};
use tokio::task::JoinHandle;
use v1per_protocol::ids::{MonitorId, ProbeId};

use super::schedule::{Checker, Grid, Next, Start};
use super::wire;
use crate::monitors::{self, Active, Assignment, Round};
use crate::state::AppState;

/// Assignments are re-read at least this often, besides after every committed change.
const RESYNC: Duration = Duration::from_secs(60);

/// Runs the built-in probe until `stop`. Checks in flight are abandoned; their monitors run
/// again at the next start.
pub async fn run(state: AppState, mut stop: watch::Receiver<bool>) {
    let probe = match builtin_id(&state).await {
        Ok(probe) => probe,
        Err(error) => {
            tracing::error!(%error, "the built-in probe is missing; no checks run");
            return;
        }
    };
    let checker = Arc::new(Checker::new(state.builtin_lag.clone()));
    let (mut wake, _) = state.hub.subscribe();
    let mut tasks: HashMap<MonitorId, (Active, JoinHandle<()>)> = HashMap::new();
    let mut version = None;
    let started = state.clock.now_ms();
    while !*stop.borrow() {
        if let Err(error) = sync(&state, &checker, probe, &mut tasks, &mut version, started).await {
            tracing::error!(%error, "reading monitor assignments failed; retrying");
        }
        tokio::select! {
            woken = wake.recv() => {
                if matches!(woken, Err(RecvError::Closed)) {
                    break;
                }
            }
            _ = tokio::time::sleep(RESYNC) => {}
            _ = stop.changed() => {}
        }
    }
    for (_, task) in tasks.into_values() {
        task.abort();
    }
}

async fn builtin_id(state: &AppState) -> sqlx::Result<ProbeId> {
    super::builtin(&mut *state.db.reader.acquire().await?).await
}

/// Brings the tasks in line with the active monitors when the assignment version moved.
async fn sync(
    state: &AppState,
    checker: &Arc<Checker>,
    probe: ProbeId,
    tasks: &mut HashMap<MonitorId, (Active, JoinHandle<()>)>,
    version: &mut Option<i64>,
    started: i64,
) -> anyhow::Result<()> {
    let mut conn = state.db.reader.acquire().await?;
    let current = monitors::assignment_version(&mut conn).await?;
    if *version == Some(current) {
        return Ok(());
    }
    let active: HashMap<MonitorId, Active> = monitors::active(&mut conn, probe, true)
        .await?
        .into_iter()
        .map(|active| (active.monitor, active))
        .collect();
    tasks.retain(|monitor, (running, task)| {
        let keep = active.get(monitor) == Some(running) && !task.is_finished();
        if !keep {
            task.abort();
        }
        keep
    });
    for (monitor, run) in active {
        if tasks.contains_key(&monitor) {
            continue;
        }
        let task = match monitors::assignment(&mut conn, &state.secrets, monitor).await? {
            Some(Assignment::Check(assignment)) => {
                let retries: Option<i64> = sqlx::query_scalar(
                    "SELECT retries FROM probe_state WHERE monitor_id = ? AND probe_id = ?",
                )
                .bind(monitor.get())
                .bind(probe.get())
                .fetch_optional(&mut *conn)
                .await?;
                tokio::spawn(check(
                    state.clone(),
                    checker.clone(),
                    probe,
                    *assignment,
                    retries.unwrap_or(0) as u32,
                    started,
                ))
            }
            Some(Assignment::Round(round)) => tokio::spawn(rounds(state.clone(), round, started)),
            None => continue,
        };
        tasks.insert(monitor, (run, task));
    }
    *version = Some(current);
    Ok(())
}

/// One monitor's checks, forever. `retries` continues the probe's count from its state.
async fn check(
    state: AppState,
    checker: Arc<Checker>,
    probe: ProbeId,
    assignment: wire::Assignment,
    mut retries: u32,
    started: i64,
) {
    let clock = &state.clock;
    let mut grid = Grid::new(
        assignment.monitor,
        assignment.interval_ms,
        assignment.retry_interval_ms,
        Start::for_change(assignment.updated_at, started),
        clock.now_ms(),
    );
    loop {
        clock.sleep_until(grid.due()).await;
        let result = checker
            .check(&assignment, &mut retries, grid.due(), clock)
            .await;
        let next = result.next();
        state.evaluator.result(probe, result).await;
        grid.advance(next, clock.now_ms());
    }
}

/// One server-evaluated monitor's rounds, forever.
async fn rounds(state: AppState, round: Round, started: i64) {
    let clock = &state.clock;
    let start = if round.maintenance {
        Start::Now
    } else if round.push {
        Start::AfterInterval
    } else {
        Start::for_change(round.updated_at, started)
    };
    let mut grid = Grid::new(
        round.monitor,
        round.interval_ms,
        round.retry_interval_ms,
        start,
        clock.now_ms(),
    );
    loop {
        clock.sleep_until(grid.due()).await;
        let next: Next = state
            .evaluator
            .round(round.monitor, round.revision, clock.now_ms())
            .await;
        grid.advance(next, clock.now_ms());
    }
}
