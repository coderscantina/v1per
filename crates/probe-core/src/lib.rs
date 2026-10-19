//! What every probe shares, the built-in one inside `v1per serve` and the `v1per-probe` agent
//! (decisions 0003 and 0011): the agent protocol's wire types, the scheduler, Kuma's retry
//! rule and the clock. It stays free of storage, so the agent binary stays small.

pub mod clock;
pub mod retry;
pub mod schedule;
pub mod wire;

pub use clock::now_ms;
