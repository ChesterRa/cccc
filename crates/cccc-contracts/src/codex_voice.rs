use crate::ActorRuntime;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexVoiceSettings {
    #[serde(default, skip_serializing_if = "AgentRuntimeSettings::is_default")]
    pub analyst: AgentRuntimeSettings,
}

impl CodexVoiceSettings {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// User-owned runtime configuration shared by agent hosts.
///
/// Group Actors currently keep these fields flat in their versioned ledger
/// shape. Platform services such as Voice Analyst use this compact form and
/// apply their own host policy (working directory, transport, permissions and
/// CCCC bindings) after resolving the custom command or Runtime Profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentRuntimeSettings {
    #[serde(default, skip_serializing_if = "is_default_runtime")]
    pub runtime: ActorRuntime,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile_id: String,
    #[serde(default = "global_scope", skip_serializing_if = "is_global_scope")]
    pub profile_scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile_owner: String,
}

impl Default for AgentRuntimeSettings {
    fn default() -> Self {
        Self {
            runtime: ActorRuntime::Codex,
            command: Vec::new(),
            profile_id: String::new(),
            profile_scope: global_scope(),
            profile_owner: String::new(),
        }
    }
}

impl AgentRuntimeSettings {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn uses_profile(&self) -> bool {
        !self.profile_id.trim().is_empty()
    }
}

/// Compatibility name for the first Codex-only settings implementation.
pub type CodexVoiceAnalystSettings = AgentRuntimeSettings;

#[derive(Debug, Clone, Default, Deserialize)]
struct AgentRuntimeSettingsWire {
    #[serde(default)]
    runtime: ActorRuntime,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    profile_id: String,
    #[serde(default = "global_scope")]
    profile_scope: String,
    #[serde(default)]
    profile_owner: String,

    // Compatibility with the first local Voice Analyst settings prototype.
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    config_overrides: Vec<LegacyCodexConfigOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct LegacyCodexConfigOverride {
    key: String,
    value: String,
}

impl<'de> Deserialize<'de> for AgentRuntimeSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AgentRuntimeSettingsWire::deserialize(deserializer)?;
        let has_legacy = wire.executable.is_some()
            || wire.profile.is_some()
            || !wire.config_overrides.is_empty();
        let command = if wire.command.is_empty() && has_legacy {
            let executable = wire
                .executable
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "codex".into());
            let mut command = vec![executable];
            if let Some(profile) = wire
                .profile
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
            {
                command.extend(["--profile".into(), profile]);
            }
            for entry in wire.config_overrides {
                let key = entry.key.trim();
                if !key.is_empty() {
                    command.extend(["-c".into(), format!("{}={}", key, entry.value)]);
                }
            }
            command
        } else {
            wire.command
        };
        Ok(Self {
            runtime: wire.runtime,
            command,
            profile_id: wire.profile_id,
            profile_scope: wire.profile_scope,
            profile_owner: wire.profile_owner,
        })
    }
}

fn global_scope() -> String {
    "global".into()
}

fn is_global_scope(value: &String) -> bool {
    value == "global"
}

fn is_default_runtime(value: &ActorRuntime) -> bool {
    *value == ActorRuntime::Codex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_settings_do_not_emit_empty_sections() {
        assert_eq!(
            serde_json::to_value(CodexVoiceSettings::default()).expect("settings"),
            serde_json::json!({})
        );
    }

    #[test]
    fn settings_round_trip_runtime_profile_reference() {
        let settings = CodexVoiceSettings {
            analyst: AgentRuntimeSettings {
                runtime: ActorRuntime::Codex,
                profile_id: "rp_voice".into(),
                profile_scope: "user".into(),
                profile_owner: "owner-1".into(),
                ..Default::default()
            },
        };
        let encoded = serde_json::to_value(&settings).expect("encode");
        assert_eq!(
            serde_json::from_value::<CodexVoiceSettings>(encoded).expect("decode"),
            settings
        );
    }

    #[test]
    fn legacy_codex_fields_migrate_to_the_shared_command() {
        let settings = serde_json::from_value::<AgentRuntimeSettings>(serde_json::json!({
            "executable":"/opt/codex",
            "profile":"voice",
            "config_overrides":[{"key":"model_reasoning_effort","value":"\"high\""}]
        }))
        .expect("legacy settings");
        assert_eq!(
            settings.command,
            [
                "/opt/codex",
                "--profile",
                "voice",
                "-c",
                "model_reasoning_effort=\"high\""
            ]
        );
    }

    #[test]
    fn blank_legacy_fields_migrate_to_the_codex_default() {
        let settings = serde_json::from_value::<AgentRuntimeSettings>(serde_json::json!({
            "executable":"  ",
            "profile":"",
            "config_overrides":[{"key":" ","value":""}]
        }))
        .expect("legacy settings");
        assert_eq!(settings.command, ["codex"]);
    }
}
