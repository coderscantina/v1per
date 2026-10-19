//! Opaque identifiers. They travel as strings and are compared byte for byte.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
        )]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// A person who signs in.
    UserId
);

id_type!(
    /// A scoped API key.
    ApiKeyId
);

id_type!(
    /// An invitation to a node.
    InvitationId
);

macro_rules! row_id {
    ($(#[$meta:meta])* $name:ident, $what:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, TS, JsonSchema)]
        pub struct $name(
            #[ts(type = "string")]
            #[schemars(with = "String")]
            i64,
        );

        impl $name {
            pub const fn new(value: i64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = std::num::ParseIntError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = std::borrow::Cow::<str>::deserialize(deserializer)?;
                text.parse().map_err(|_| {
                    serde::de::Error::custom(concat!("a ", $what, " ID is a decimal string"))
                })
            }
        }
    };
}

row_id!(
    /// A node of the tree. A SQLite row ID that is never reused, like monitor and probe IDs, and
    /// sent as a decimal string like every other ID.
    NodeId,
    "node"
);

row_id!(
    /// A monitor, owned by one node.
    MonitorId,
    "monitor"
);

row_id!(
    /// A probe that runs checks: the built-in probe or a remote agent.
    ProbeId,
    "probe"
);

row_id!(
    /// A notification channel, owned by one node and usable in its subtree.
    ChannelId,
    "channel"
);

row_id!(
    /// One message for one channel in the notification outbox.
    DeliveryId,
    "delivery"
);

row_id!(
    /// An incident, owned by one node.
    IncidentId,
    "incident"
);

row_id!(
    /// One entry of an incident's timeline.
    IncidentUpdateId,
    "incident update"
);

row_id!(
    /// A maintenance with its schedule, owned by one node.
    MaintenanceId,
    "maintenance"
);

row_id!(
    /// A static component: a part of a service without a monitor, owned by one node.
    ComponentId,
    "component"
);

row_id!(
    /// A status page, owned by one node.
    PageId,
    "page"
);

row_id!(
    /// A group of components on one status page.
    PageGroupId,
    "page group"
);

row_id!(
    /// A monitor or static component placed on one status page. Public snapshots name
    /// components by this ID only, never by monitor or component ID.
    PageComponentId,
    "page component"
);

row_id!(
    /// A share token of one status page.
    ShareTokenId,
    "share token"
);
