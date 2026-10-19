//! Work beside request handling: the built-in probe, the monitor evaluator, the notification
//! outbox, the maintenance timer, status page auto-publication and operator jobs, which
//! include the hourly retention run. Started once the server is composed, stopped before
//! storage and the control database close.

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::probes;
use crate::state::AppState;

pub struct Background {
    stop: watch::Sender<bool>,
    probe: JoinHandle<()>,
    evaluator: JoinHandle<()>,
    outbox: JoinHandle<()>,
    maintenance: JoinHandle<()>,
    auto_publish: JoinHandle<()>,
    jobs: JoinHandle<()>,
    state: AppState,
}

impl Background {
    /// # Panics
    ///
    /// When background work was already started on this state.
    pub fn start(state: &AppState) -> Self {
        let (stop, stopped) = watch::channel(false);
        let queue = state
            .evaluator_queue
            .lock()
            .expect("evaluator queue")
            .take()
            .expect("background work starts once per state");
        Self {
            evaluator: tokio::spawn(crate::monitors::run_evaluator(
                state.clone(),
                queue,
                stopped.clone(),
            )),
            probe: tokio::spawn(probes::run(state.clone(), stopped.clone())),
            outbox: tokio::spawn(crate::notifications::run(state.clone(), stopped.clone())),
            maintenance: tokio::spawn(crate::incidents::maintenance::run(
                state.clone(),
                stopped.clone(),
            )),
            auto_publish: tokio::spawn(crate::pages::run_auto_publish(
                state.clone(),
                stopped.clone(),
            )),
            jobs: tokio::spawn(crate::maintenance::jobs::follow(state.clone(), stopped)),
            stop,
            state: state.clone(),
        }
    }

    /// How late the built-in probe's checks started: (started, more than one interval late).
    pub fn check_lag(&self) -> (u64, u64) {
        self.state.builtin_lag.read()
    }

    /// Stops checks first, then lets the evaluator record what is queued, the outbox finish
    /// the sends in flight and a running job finish. Interrupted work rolls back and runs again
    /// at the next start; an interrupted job is marked failed.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.probe.await;
        let _ = self.evaluator.await;
        let _ = self.outbox.await;
        let _ = self.maintenance.await;
        let _ = self.auto_publish.await;
        let _ = self.jobs.await;
    }
}
