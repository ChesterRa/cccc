//! Administrator projections; never native provider payloads or authority tokens.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceControl {
    pub supported: bool,
    pub management_revision: String,
    pub access_blocked: bool,
    pub pause_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
}

impl Default for SourceControl {
    fn default() -> Self {
        Self {
            supported: true,
            management_revision: "0".into(),
            access_blocked: false,
            pause_complete: true,
            reason_code: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveState {
    Ready,
    SharedBackendMissing,
    ThreadNotLoaded,
    ReceiveCheckFailed,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceDiagnostic {
    pub state: ReceiveState,
    pub checked_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl Default for SourceDiagnostic {
    fn default() -> Self {
        Self {
            state: ReceiveState::Unknown,
            checked_at: None,
            origin: None,
        }
    }
}
