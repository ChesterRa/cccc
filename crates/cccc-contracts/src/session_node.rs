//! Versioned session-node protocol; endpoint identity is not Group authority.
use crate::local_session::LocalSessionSource;
use serde::{Deserialize, Serialize};

pub const SESSION_NODE_PROTOCOL: u32 = 1;
pub const SESSION_NODE_RELAY_TICKET: u32 = 2;
pub const SESSION_NODE_ALPN: &[u8] = b"cccc/session-node/1";
pub const REMOTE_SOURCE_PREFIX: &str = "remote:";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionNodeTicket {
    pub v: u32,
    pub endpoint_id: String,
    pub addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_token: Option<String>,
    pub invite_id: String,
    pub secret: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionNodeSource {
    pub source: LocalSessionSource,
    pub session_revision: String,
    pub alive: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteSessionSource {
    pub source: LocalSessionSource,
    pub native_source_id: String,
    pub session_revision: String,
    pub node_id: String,
    pub node_name: String,
    pub grant_id: String,
    pub alive: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionNodeMembership {
    pub hub_id: String,
    pub source_id: String,
    pub source_revision: String,
    pub group_id: String,
    pub group_title: String,
    pub actor_id: String,
    pub actor_generation: String,
    pub role: String,
    pub scope_key: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteSessionDelivery {
    pub handoff_id: String,
    pub source_id: String,
    pub source_revision: String,
    pub membership: SessionNodeMembership,
    pub event_ids: Vec<String>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SessionNodeDeliveryOutcome {
    Accepted,
    Rejected { reason: String },
    Ambiguous { reason: String },
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionNodeEndpoint {
    pub endpoint_id: String,
    pub addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionNodePeer {
    pub node_id: String,
    pub name: String,
    pub grant_id: String,
    pub enabled: bool,
    pub connected: bool,
    pub granted_at: String,
    pub last_seen_at: String,
}
