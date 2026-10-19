//! Local fakes and fixtures shared by the integration tests. Nothing here touches the network.
#![allow(dead_code)]

pub mod events;
pub mod http;
pub mod smtp;

use v1per_notify::ChannelConfig;

/// Parses a channel the way the importer reads Kuma's `notification.config`.
pub fn channel(config: serde_json::Value) -> ChannelConfig {
    serde_json::from_value(config).expect("valid Kuma notification config")
}
