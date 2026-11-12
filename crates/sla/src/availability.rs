//! Availability, the downtime budget and average latency.
//!
//! Availability and its targets are ratios from 0 to 1, so 99.9 % is `0.999`.

use chrono::TimeDelta;

/// The share of the budget at which the warning fires.
pub const WARNING_SHARE: f64 = 0.75;

/// `each` mode availability of one monitor: `up / (up + down)`, pending counted as down.
/// `None` without beats.
pub fn availability(up: u64, down: u64) -> Option<f64> {
    let total = up + down;
    (total > 0).then(|| up as f64 / total as f64)
}

/// `any_down` mode availability: `(counted - down) / counted` over minutes. `None` without
/// counted minutes.
pub fn any_down_availability(counted: u64, down: u64) -> Option<f64> {
    (counted > 0).then(|| counted.saturating_sub(down) as f64 / counted as f64)
}

/// Whether a closed period missed its target. No data is not a miss.
pub fn below_target(availability: Option<f64>, target: f64) -> bool {
    availability.is_some_and(|value| value < target)
}

/// What the expected count of a budget is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetBasis {
    /// `each` mode: beats of one monitor, the period length divided by its interval.
    Beats { interval: TimeDelta },
    /// `any_down` mode: minutes in the period.
    Minutes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetState {
    Within,
    /// Down reached [`WARNING_SHARE`] of the budget.
    Warning,
    /// Down passed the budget.
    Exceeded,
}

/// How many beats or minutes a period may be down: `(1 - target)` times the expected count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DowntimeBudget {
    pub allowed: f64,
}

impl DowntimeBudget {
    pub fn new(target: f64, period_length: TimeDelta, basis: BudgetBasis) -> Self {
        let unit = match basis {
            BudgetBasis::Beats { interval } => interval,
            BudgetBasis::Minutes => TimeDelta::minutes(1),
        };
        let expected = period_length.num_milliseconds() as f64 / unit.num_milliseconds() as f64;
        Self {
            allowed: (1.0 - target).max(0.0) * expected,
        }
    }

    /// The share of the budget `down` uses. `None` when nothing is allowed down.
    pub fn share_used(&self, down: u64) -> Option<f64> {
        (self.allowed > 0.0).then(|| down as f64 / self.allowed)
    }

    pub fn state(&self, down: u64) -> BudgetState {
        let down = down as f64;
        if down > self.allowed {
            BudgetState::Exceeded
        } else if down > 0.0 && down >= WARNING_SHARE * self.allowed {
            BudgetState::Warning
        } else {
            BudgetState::Within
        }
    }
}

/// Average latency in milliseconds over up beats. `None` without measured beats.
pub fn average_latency(latency_sum: f64, latency_count: u64) -> Option<f64> {
    (latency_count > 0).then(|| latency_sum / latency_count as f64)
}

/// Whether an average is above the target. No data is not a breach.
pub fn latency_breached(average: Option<f64>, target_ms: f64) -> bool {
    average.is_some_and(|value| value > target_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_needs_data() {
        assert_eq!(availability(0, 0), None);
        assert_eq!(availability(999, 1), Some(0.999));
        assert_eq!(any_down_availability(0, 0), None);
        assert_eq!(any_down_availability(1000, 10), Some(0.99));
    }

    #[test]
    fn closed_period_below_target_misses() {
        assert!(below_target(Some(0.998), 0.999));
        assert!(!below_target(Some(0.999), 0.999));
        assert!(!below_target(None, 0.999));
    }

    #[test]
    fn each_mode_budget_counts_expected_beats() {
        // 30 days at a 60 s interval is 43 200 beats; 99.9 % allows 43.2 down.
        let budget = DowntimeBudget::new(
            0.999,
            TimeDelta::days(30),
            BudgetBasis::Beats {
                interval: TimeDelta::seconds(60),
            },
        );
        assert!((budget.allowed - 43.2).abs() < 1e-9);
        assert_eq!(budget.state(32), BudgetState::Within);
        assert_eq!(budget.state(33), BudgetState::Warning);
        assert_eq!(budget.state(43), BudgetState::Warning);
        assert_eq!(budget.state(44), BudgetState::Exceeded);
    }

    #[test]
    fn any_down_budget_counts_minutes() {
        // A 31 day month has 44 640 minutes; 99 % allows 446.4 down.
        let budget = DowntimeBudget::new(0.99, TimeDelta::days(31), BudgetBasis::Minutes);
        assert!((budget.allowed - 446.4).abs() < 1e-9);
        let share = budget.share_used(223).expect("budget above zero");
        assert!((share - 0.5).abs() < 1e-3);
    }

    #[test]
    fn full_target_has_no_budget() {
        let budget = DowntimeBudget::new(1.0, TimeDelta::days(30), BudgetBasis::Minutes);
        assert_eq!(budget.share_used(0), None);
        assert_eq!(budget.state(0), BudgetState::Within);
        assert_eq!(budget.state(1), BudgetState::Exceeded);
    }

    #[test]
    fn latency_average_against_target() {
        assert_eq!(average_latency(0.0, 0), None);
        assert_eq!(average_latency(4500.0, 30), Some(150.0));
        assert!(latency_breached(Some(150.0), 120.0));
        assert!(!latency_breached(Some(120.0), 120.0));
        assert!(!latency_breached(None, 120.0));
    }
}
