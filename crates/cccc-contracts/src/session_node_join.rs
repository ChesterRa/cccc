//! Public enrollment requests carry no administrator, Group or session authority.
use serde::{Deserialize, Serialize};

pub const JOIN_TTL_SECONDS: u64 = 600;
pub const JOIN_POLL_SECONDS: u64 = 2;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub node_id: String,
    pub name: String,
    /// Client-generated 32-byte random value encoded as 64 hexadecimal digits.
    pub device_code: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CreatedRequest {
    pub request_id: String,
    pub user_code: String,
    pub expires_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollRequest {
    pub request_id: String,
    pub device_code: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinStatus {
    Pending,
    Approved,
    Denied,
    Expired,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PollResponse {
    pub status: JoinStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingRequest {
    pub request_id: String,
    pub name: String,
    pub node_id: String,
    pub user_code: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestDetails {
    #[serde(flatten)]
    pub request: PendingRequest,
    pub status: JoinStatus,
}
