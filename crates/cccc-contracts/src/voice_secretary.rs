//! Global ASR secretary. Realtime Voice Analyst has a separate lifecycle.
use crate::{ActorRuntime, AgentRuntimeSettings, RuntimeMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSecretarySettings {
    /// None with no Profile means unconfigured; recognition remains usable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<ActorRuntime>,
    #[serde(default, skip_serializing_if = "RuntimeMode::is_default")]
    pub runtime_mode: RuntimeMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    #[serde(default)]
    pub profile_id: String,
    #[serde(default = "global_scope")]
    pub profile_scope: String,
    #[serde(default)]
    pub profile_owner: String,
    #[serde(default)]
    pub config: VoiceSecretaryPreferences,
}

impl Default for VoiceSecretarySettings {
    fn default() -> Self {
        Self {
            runtime: None,
            runtime_mode: RuntimeMode::default(),
            command: Vec::new(),
            profile_id: String::new(),
            profile_scope: global_scope(),
            profile_owner: String::new(),
            config: VoiceSecretaryPreferences::default(),
        }
    }
}

impl VoiceSecretarySettings {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn runtime_settings(&self) -> Option<AgentRuntimeSettings> {
        (self.runtime.is_some() || !self.profile_id.trim().is_empty()).then(|| {
            AgentRuntimeSettings {
                runtime: self.runtime.unwrap_or_default(),
                runtime_mode: self.runtime_mode,
                command: self.command.clone(),
                profile_id: self.profile_id.clone(),
                profile_scope: self.profile_scope.clone(),
                profile_owner: self.profile_owner.clone(),
            }
        })
    }
}

/// Why the daemon cannot accept new Secretary model work. Provider login and
/// process observations belong to the separate runtime phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretaryReadinessCode {
    NotConfigured,
    InvalidConfiguration,
    OwnerUnavailable,
}

/// Read-only admission state, shared by settings, tasks, health and runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretaryReadiness {
    /// A Runtime or Profile is saved, even if it cannot currently be resolved.
    pub configured: bool,
    /// The host permits new model work; does not imply a running provider.
    pub ready: bool,
    pub readiness_code: Option<SecretaryReadinessCode>,
    pub readiness_error: Option<String>,
}

fn global_scope() -> String {
    "global".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VoiceSecretaryPreferences {
    pub recognition_backend: String,
    pub recognition_language: String,
    pub external_asr_provider: String,
    pub service_model_id: String,
    pub service_diarization_model_id: String,
    pub auto_document_max_window_seconds: Option<u32>,
    pub guidance: String,
}

impl Default for VoiceSecretaryPreferences {
    fn default() -> Self {
        Self {
            recognition_backend: "browser_asr".into(),
            recognition_language: "auto".into(),
            external_asr_provider: "bailian".into(),
            service_model_id: String::new(),
            service_diarization_model_id: String::new(),
            auto_document_max_window_seconds: Some(300),
            guidance: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretaryTaskKind {
    Document,
    Ask,
    Prompt,
}

/// Live observation of one fixed task; not a resumable session or task grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretaryTaskExecution {
    pub generation: String,
    pub runtime: ActorRuntime,
    pub native_terminal: bool,
    pub progress: String,
    pub activity: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretaryTaskPhase {
    Queued,
    Starting,
    Running,
    Done,
    NeedsUser,
    Conflict,
    Failed,
    Cancelled,
    Unconfirmed,
}

impl SecretaryTaskPhase {
    pub fn executing(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

/// Authority is selected by the host at acceptance, never by an MCP argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretaryTaskTarget {
    pub group_id: String,
    pub scope_key: String,
    pub kind: SecretaryTaskKind,
    #[serde(default)]
    pub document_id: String,
    #[serde(default)]
    pub document_path: String,
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub composer_snapshot_hash: String,
}
