use super::require_interactive_web;
use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::routes::codex_voice::payload;
use axum::Json;
use axum::extract::State;
use cccc_contracts::CodexVoiceAnalystSettings;
use cccc_daemon::experimental_codex_voice::{DEFAULT_REALTIME_VOICE, RealtimeCallConfig};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) async fn codex_voice_readiness(home: &cccc_core::HomeLayout) -> Value {
    let settings = cccc_core::codex_voice_settings::load(home).unwrap_or_default();
    let custom_environment =
        cccc_core::codex_voice_settings::private_environment(home).unwrap_or_default();
    let runtime =
        cccc_core::codex_voice_settings::resolve(home, &settings, &custom_environment).ok();
    let analyst_runtime = runtime
        .as_ref()
        .map(|runtime| runtime_name(runtime.runtime))
        .unwrap_or_else(|| runtime_name(settings.runtime));
    let analyst_runtime_available = runtime.as_ref().is_some_and(|runtime| {
        let executable = analyst_runtime_executable(runtime);
        let explicit = std::path::Path::new(executable).is_absolute()
            || executable == "~"
            || executable.starts_with("~/")
            || executable.starts_with("~\\")
            || executable.contains('/')
            || executable.contains('\\');
        if explicit {
            cccc_core::path_input::expand_user_path(executable).is_ok_and(|path| path.is_file())
        } else {
            cccc_runtime::resolve_executable_in_path(
                executable,
                runtime.environment.get("PATH").map(String::as_str),
            )
            .is_some()
        }
    });
    let realtime_credentials_available =
        match RealtimeCallConfig::from_environment_with_voice(DEFAULT_REALTIME_VOICE) {
            Ok(config) => read_realtime_credentials_available(&config.auth_path).await,
            Err(_) => false,
        };
    json!({
        "analyst_runtime":analyst_runtime,
        "analyst_runtime_available":analyst_runtime_available,
        "realtime_credentials_available":realtime_credentials_available,
    })
}

fn analyst_runtime_executable(
    runtime: &cccc_core::codex_voice_settings::ResolvedAgentRuntime,
) -> &str {
    runtime
        .command
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| match runtime.runtime {
            cccc_contracts::ActorRuntime::Claude => "claude",
            cccc_contracts::ActorRuntime::Grok => "grok",
            cccc_contracts::ActorRuntime::Opencode => "opencode",
            cccc_contracts::ActorRuntime::Kilo => "kilo",
            _ => "codex",
        })
}

fn runtime_name(runtime: cccc_contracts::ActorRuntime) -> String {
    serde_json::to_value(runtime)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

#[derive(Debug, Deserialize)]
pub(in crate::routes::codex_voice) struct AnalystSettingsUpdate {
    pub settings: Value,
    #[serde(default)]
    pub environment_set: BTreeMap<String, String>,
    #[serde(default)]
    pub environment_unset: Vec<String>,
    #[serde(default)]
    pub environment_clear: bool,
    #[serde(default)]
    pub discard_current_work: bool,
}

pub(in crate::routes::codex_voice) async fn analyst_settings(
    State(state): State<AppState>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let settings = cccc_core::codex_voice_settings::load(&state.home).map_err(|error| {
        tracing::warn!(%error, "Voice Analyst settings read failed");
        ApiError::unavailable(
            "codex_voice_settings_unavailable",
            "Voice Analyst settings could not be read.",
        )
    })?;
    let environment =
        cccc_core::codex_voice_settings::private_environment(&state.home).map_err(|error| {
            tracing::warn!(%error, "Voice Analyst private environment read failed");
            ApiError::unavailable(
                "codex_voice_settings_unavailable",
                "Voice Analyst private environment metadata could not be read.",
            )
        })?;
    Ok(success(json!({
        "settings":settings,
        "environment_keys":environment.into_keys().collect::<Vec<_>>(),
    })))
}

pub(in crate::routes::codex_voice) async fn update_analyst_settings(
    State(state): State<AppState>,
    Json(body): Json<AnalystSettingsUpdate>,
) -> ApiResult {
    require_interactive_web(&state)?;
    let settings = parse_analyst_settings(body.settings).map_err(|error| {
        ApiError::bad_code("codex_voice_settings_invalid", error.to_string(), json!({}))
    })?;
    let outcome = state
        .codex_voice
        .apply_analyst_settings(
            &state.home,
            settings,
            body.environment_set,
            body.environment_unset,
            body.environment_clear,
            body.discard_current_work,
        )
        .await
        .map_err(|error| {
            let message = error.to_string();
            tracing::warn!(%error, "Voice Analyst settings update failed");
            if message.contains("Stop the active") {
                ApiError::conflict(
                    "codex_voice_call_active",
                    "Stop the active Codex Voice call before applying Analyst settings.",
                    json!({}),
                )
            } else if message.contains("Wait for or cancel") {
                ApiError::conflict(
                    "codex_voice_settings_busy",
                    "The Voice Analyst still has active or queued work.",
                    json!({}),
                )
            } else {
                ApiError::bad_code("codex_voice_settings_invalid", message, json!({}))
            }
        })?;
    Ok(success(json!({
        "analyst":outcome.analyst.map(payload::analyst_info_value),
        "restarted":outcome.restarted,
        "started_new_session":outcome.started_new_session,
        "discarded_work":outcome.discarded_work,
    })))
}

fn parse_analyst_settings(mut value: Value) -> anyhow::Result<CodexVoiceAnalystSettings> {
    if let Some(command) = value
        .as_object_mut()
        .and_then(|settings| settings.get_mut("command"))
        && let Some(command_line) = command.as_str()
    {
        *command = serde_json::to_value(
            shell_words::split(command_line)
                .map_err(|error| anyhow::anyhow!("invalid runtime command: {error}"))?,
        )?;
    }
    Ok(serde_json::from_value(value)?)
}

async fn read_realtime_credentials_available(path: &std::path::Path) -> bool {
    const MAX_AUTH_BYTES: u64 = 1024 * 1024;
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if !metadata.is_file() || metadata.len() > MAX_AUTH_BYTES {
        return false;
    }
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    realtime_credentials_available(&bytes)
}

fn realtime_credentials_available(bytes: &[u8]) -> bool {
    let Ok(auth) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    ["access_token", "account_id"].into_iter().all(|key| {
        auth.get("tokens")
            .and_then(|tokens| tokens.get(key))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::{
        analyst_runtime_executable, parse_analyst_settings, realtime_credentials_available,
    };
    use cccc_contracts::ActorRuntime;
    use cccc_core::codex_voice_settings::ResolvedAgentRuntime;
    use std::collections::BTreeMap;

    #[test]
    fn readiness_requires_both_codex_token_fields() {
        assert!(realtime_credentials_available(
            br#"{"tokens":{"access_token":"access","account_id":"account"}}"#
        ));
        assert!(!realtime_credentials_available(
            br#"{"tokens":{"access_token":"access"}}"#
        ));
        assert!(!realtime_credentials_available(b"not-json"));
    }

    #[test]
    fn readiness_probes_the_selected_runtime_default() {
        for (runtime, executable) in [
            (ActorRuntime::Codex, "codex"),
            (ActorRuntime::Claude, "claude"),
            (ActorRuntime::Grok, "grok"),
            (ActorRuntime::Opencode, "opencode"),
            (ActorRuntime::Kilo, "kilo"),
        ] {
            let resolved = ResolvedAgentRuntime {
                runtime,
                command: Vec::new(),
                environment: BTreeMap::new(),
            };
            assert_eq!(analyst_runtime_executable(&resolved), executable);
        }
    }

    #[test]
    fn runtime_command_uses_the_same_shell_parser_as_actor_editing() {
        let settings = parse_analyst_settings(serde_json::json!({
            "runtime":"codex",
            "command":"codex -c 'model_provider=\"private provider\"' --search"
        }))
        .expect("settings");
        assert_eq!(
            settings.command,
            [
                "codex",
                "-c",
                "model_provider=\"private provider\"",
                "--search"
            ]
        );
    }
}
