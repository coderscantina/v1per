//! SLA rules from the monitoring contract's SLA section (decision 0012). Pure computation:
//! periods in the SLA timezone, business time clocks, incident response and resolution
//! outcomes, availability with its downtime budget, and average latency. The server maps
//! its records to these plain types.
//!
//! All instants are UTC. Durations are [`chrono::TimeDelta`].

mod availability;
mod business;
mod incident;
mod period;

pub use availability::{
    BudgetBasis, BudgetState, DowntimeBudget, WARNING_SHARE, any_down_availability, availability,
    average_latency, below_target, latency_breached,
};
pub use business::{BusinessHours, Clock, ScheduleError, WeeklyRange};
pub use incident::{
    Impact, ImpactTargets, IncidentTimes, Resolution, Response, resolution, response, worst_impact,
};
pub use period::{Period, PeriodCalendar, PeriodKind, day_start, local_day};
