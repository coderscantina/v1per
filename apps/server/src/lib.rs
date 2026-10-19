//! v1per server: admin API, live subscriptions, monitoring with the built-in probe and remote
//! probe agents, runtime configuration and the built web app in one process. The same binary
//! runs as a probe agent.

pub mod background;
pub mod clock;
pub mod config;
pub mod context;
pub mod db;
pub mod error;
pub mod events;
pub mod http;
pub mod idempotency;
pub mod identity;
pub mod imports;
pub mod incidents;
pub mod init;
pub mod instance;
pub mod lock;
pub mod maintenance;
pub mod monitors;
pub mod nodes;
pub mod notifications;
pub mod pages;
pub mod probes;
pub mod state;

pub use http::router;
pub use state::AppState;
