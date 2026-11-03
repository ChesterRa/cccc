use serde::{Deserialize, Serialize};

/// A reference to a message kept in a Group's shared Web pin list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessagePin {
    pub event_id: String,
    pub pinned_at: String,
}

/// The desired state makes retries safe: this is not a toggle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessagePinUpdate {
    pub event_id: String,
    pub pinned: bool,
}
