//! Scoped API keys of the caller. A key reaches one node and its subtree with a set of
//! capabilities, and acts as its owner with the owner's current role there, never more. Keys
//! send `Authorization: Bearer <secret>` and never manage keys, members or accounts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Capability;
use crate::envelope::Empty;
use crate::ids::{ApiKeyId, NodeId};

crate::domain! {
    pub trait ApiKeysHandlers;
    commands {
        /// Creates a key and discloses its secret once. Needs reauthentication. Every
        /// capability must be key-grantable and held by the caller on the node, else
        /// `grant_exceeds_principal`. A node the caller cannot see is `not_found`. At most 50
        /// active keys per person.
        "apiKeys.create" => api_keys_create (Person, reauth, secret): ApiKeysCreateArgs => ApiKeyCreated;
        /// Revokes one of the caller's keys at once. Requests with it answer like an unknown
        /// key. Revoking a revoked key answers the key unchanged.
        "apiKeys.revoke" => api_keys_revoke (Person): ApiKeyArgs => ApiKeyResult;
    }
    queries {
        /// The caller's keys, newest first, revoked ones included. Never the secret.
        "apiKeys.list" => api_keys_list (Person): Empty => ApiKeyList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKey {
    pub id: ApiKeyId,
    pub name: String,
    /// The first characters of the secret, to tell keys apart.
    pub prefix: String,
    /// The key reaches this node and its subtree.
    pub node: NodeId,
    pub capabilities: Vec<Capability>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_used_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeysCreateArgs {
    /// 1 to 100 characters after trimming.
    pub name: String,
    pub node: NodeId,
    /// 1 or more key-grantable capabilities.
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyCreated {
    pub key: ApiKey,
    /// The full secret for `Authorization: Bearer <secret>`. `null` in a replayed answer.
    pub secret: Option<String>,
    /// True when this answer is a replay and the secret was withheld.
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyArgs {
    pub key: ApiKeyId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyResult {
    pub key: ApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiKeyList {
    pub keys: Vec<ApiKey>,
}
