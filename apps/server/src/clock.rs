//! The application clock. Domain code reads wall time and waits for it only through this, so a
//! browser journey can run at a pinned instant (`serve --frozen-clock`, `dev-clock` feature)
//! and tests can move time by hand.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct Clock(Source);

#[derive(Debug, Clone)]
enum Source {
    System,
    Frozen(i64),
    /// Moves only through [`Clock::advance`].
    Manual(Arc<watch::Sender<i64>>),
}

impl Clock {
    pub fn system() -> Self {
        Self(Source::System)
    }

    /// A clock that stays at `at_ms`. Waits take their real duration, so scheduled work keeps
    /// running while the reported time stands still.
    pub fn frozen(at_ms: i64) -> Self {
        Self(Source::Frozen(at_ms))
    }

    /// A clock starting at `at_ms` that only [`Clock::advance`] moves, for tests.
    pub fn manual(at_ms: i64) -> Self {
        Self(Source::Manual(Arc::new(watch::Sender::new(at_ms))))
    }

    /// Unix milliseconds.
    pub fn now_ms(&self) -> i64 {
        match &self.0 {
            Source::System => crate::db::now_ms(),
            Source::Frozen(at) => *at,
            Source::Manual(now) => *now.borrow(),
        }
    }

    pub fn is_test(&self) -> bool {
        !matches!(self.0, Source::System)
    }

    /// Moves a manual clock forward and wakes everything waiting on it.
    ///
    /// # Panics
    ///
    /// On any other clock.
    pub fn advance(&self, by: Duration) {
        let Source::Manual(now) = &self.0 else {
            panic!("only a manual clock advances");
        };
        now.send_modify(|now| *now += by.as_millis() as i64);
    }

    /// Returns once the clock reads `at_ms` or later.
    pub async fn sleep_until(&self, at_ms: i64) {
        match &self.0 {
            Source::Manual(now) => {
                let mut now = now.subscribe();
                // The sender lives as long as this clock, so waiting cannot fail.
                let _ = now.wait_for(|now| *now >= at_ms).await;
            }
            _ => {
                let wait = (at_ms - self.now_ms()).max(0) as u64;
                tokio::time::sleep(Duration::from_millis(wait)).await;
            }
        }
    }
}
