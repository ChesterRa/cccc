//! Stable references for facts observed by the global assistant.
use serde::{Deserialize, Serialize};

/// Read-only host provenance on a Chet-authored Group event. Clients must not
/// supply it when sending; the host derives it from the current Group.
pub const GROUP_AUTHOR_GENERATION_FIELD: &str = "_cccc_chet_group_generation";

/// A Group message delivered to the assistant.
///
/// The Group ledger owns the message text and attachments. These fields
/// identify their source and the registrations observed at admission; they
/// do not themselves grant access or prove that either registration is live.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupInput {
    pub group_id: String,
    pub group_generation: String,
    pub event_id: String,
    pub sender: GroupInputSender,
    #[serde(default)]
    pub reply_to: Option<String>,
}

/// The host verifies the sender against its source before registration. A
/// stored `User` label alone does not identify an authenticated administrator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GroupInputSender {
    User,
    Actor {
        actor_id: String,
        actor_generation: String,
    },
}
