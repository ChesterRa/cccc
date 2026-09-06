//! Instance-wide trusted-user Voice preferences and ledger references.
//! These are not public chat events and do not modify Actor Mail/read state.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationScope {
    #[default]
    Off,
    ToUser,
    AllChat,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceVerbosity {
    Concise,
    #[default]
    Standard,
    Detailed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStyle {
    #[default]
    Natural,
    Direct,
    Patient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VoicePreferences {
    pub revision: u64,
    pub groups: BTreeMap<String, NotificationScope>,
    pub suppress_viewed: bool,
    pub verbosity: VoiceVerbosity,
    pub style: VoiceStyle,
}

impl Default for VoicePreferences {
    fn default() -> Self {
        Self {
            revision: 0,
            groups: BTreeMap::new(),
            suppress_viewed: true,
            verbosity: VoiceVerbosity::Standard,
            style: VoiceStyle::Natural,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceMessageRef {
    pub group_id: String,
    pub event_id: String,
}

impl VoiceMessageRef {
    pub fn key(&self) -> String {
        format!("{}:{}", self.group_id, self.event_id)
    }
    pub fn correlation_id(&self) -> String {
        format!("voice-result:{}", self.key())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceMessageKind {
    RequestReply,
    Background,
}

/// Observed handoff, not a task state or proof of model execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceMessageHandoff {
    pub analyst_generation: String,
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceNotification {
    pub sequence: u64,
    pub source: VoiceMessageRef,
    pub kind: VoiceMessageKind,
    pub by: String,
    pub to_user: bool,
    #[serde(default)]
    pub handoff: Option<VoiceMessageHandoff>,
    #[serde(default)]
    pub processed: bool,
    #[serde(default)]
    pub attempted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceNotificationResult {
    pub id: String,
    /// Durable completion order, allocated once; Runtime turn IDs are opaque.
    pub sequence: u64,
    pub analyst_generation: String,
    pub sources: Vec<VoiceMessageRef>,
    pub text: String,
    pub user_answer: bool,
    pub output_call: Option<String>,
    pub output_submitted: bool,
    pub output_suppressed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression_reason: Option<VoiceSuppressionReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSuppressionReason {
    Viewed,
    Policy,
    SourceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceOutputStatus {
    Processing,
    Ready,
    Unconfirmed,
    Submitted,
    Suppressed,
}

#[derive(Debug, Serialize)]
pub struct VoiceNotificationView {
    #[serde(flatten)]
    pub notification: VoiceNotification,
    pub output_status: VoiceOutputStatus,
    pub suppression_reason: Option<VoiceSuppressionReason>,
}
