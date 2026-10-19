//! Deployment settings the web app reads at boot from `/config.json`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Served uncached, so a changed server setting reaches the browser without rebuilding the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RuntimeConfig {
    /// Path prefix of the admin API, without a trailing slash.
    pub api_base: String,
    /// Server version, shown in the sidebar footer.
    pub version: String,
    /// The operator's name for this instance, shown in the shell and the document title.
    pub instance_name: String,
}
