//! Standalone, administrator-approved Group pairs; no account authority.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectEndpoint {
    pub instance_id: String,
    pub public_key: String,
    /// Peer software version recorded when this pairing was created.
    pub client_version: String,
    pub name: String,
    pub group_id: String,
    pub generation: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectState {
    Invited,
    Pending,
    Active,
    /// The receiver confirmed that the deadline passed without approval.
    Expired,
    Revoked,
}

/// Secrets are carried only by explicit invitation creation/import and the TLS hello.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectInvitation {
    pub v: u32,
    pub id: String,
    pub address: String,
    pub host: DirectEndpoint,
    pub expires_at: String,
    pub secret: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectListener {
    pub bind: String,
    pub address: String,
}

/// Read-only suggestions from this instance's interfaces; not proof of peer reachability.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectAddress {
    pub interface: String,
    pub bind: String,
    pub address: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectRelation {
    pub id: String,
    pub local: DirectEndpoint,
    pub remote: Option<DirectEndpoint>,
    pub state: DirectState,
    pub created_at: String,
    pub expires_at: String,
    /// Present only on the initiating daemon; never included in status output.
    pub invitation: Option<DirectInvitation>,
    /// Receiver keeps a digest, not the reusable invitation text.
    pub secret_sha256: String,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DirectStore {
    /// Removed grants stay non-reusable without retaining names or invitation secrets.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub retired: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub display_name: String,
    pub listener: Option<DirectListener>,
    pub relations: Vec<DirectRelation>,
}

pub fn is_direct(id: &str) -> bool {
    id.strip_prefix("direct-")
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}
