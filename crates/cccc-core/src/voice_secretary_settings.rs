use crate::{HomeLayout, codex_voice_settings, fs, settings};
use cccc_contracts::voice_secretary::VoiceSecretarySettings;
use serde_json::{Value, json};
use std::{collections::BTreeMap, io};

pub fn normalize(mut value: VoiceSecretarySettings) -> io::Result<VoiceSecretarySettings> {
    if value.runtime.is_none() && value.profile_id.trim().is_empty() && !value.command.is_empty() {
        return Err(invalid("Choose a Runtime before setting a command"));
    }
    // Runtime validation is shared with the Analyst; host lifecycle stays separate.
    let mut runtime = value.runtime_settings().unwrap_or_default();
    runtime = codex_voice_settings::normalize(runtime)?;
    value.runtime_mode = runtime.runtime_mode;
    value.command = runtime.command;
    value.profile_id = runtime.profile_id;
    value.profile_scope = runtime.profile_scope;
    value.profile_owner = runtime.profile_owner;
    let config = &mut value.config;
    config.recognition_backend = config.recognition_backend.trim().to_owned();
    if !matches!(
        config.recognition_backend.as_str(),
        "browser_asr" | "assistant_service_local_asr" | "external_provider_asr"
    ) {
        return Err(invalid("Unsupported recognition backend"));
    }
    config.recognition_language = config.recognition_language.trim().to_owned();
    if config.recognition_language.is_empty()
        || config.recognition_language.len() > 64
        || config.recognition_language.contains('\0')
    {
        return Err(invalid("Invalid recognition language"));
    }
    if !matches!(
        config.external_asr_provider.as_str(),
        "bailian" | "volcengine"
    ) {
        return Err(invalid("Unsupported external ASR provider"));
    }
    if config
        .auto_document_max_window_seconds
        .is_some_and(|seconds| !(10..=300).contains(&seconds))
    {
        return Err(invalid(
            "Document update interval must be between 10 and 300 seconds",
        ));
    }
    for id in [
        &config.service_model_id,
        &config.service_diarization_model_id,
    ] {
        if id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return Err(invalid("Invalid local ASR model id"));
        }
    }
    if config.guidance.len() > crate::group_prompts::MAX_PROMPT_BYTES
        || config.guidance.contains('\0')
    {
        return Err(invalid(
            "Secretary work rules must be at most 512 KiB of text",
        ));
    }
    config.guidance = config.guidance.trim().to_owned();
    Ok(value)
}

pub fn resolve(
    home: &HomeLayout,
    value: &VoiceSecretarySettings,
    environment: &BTreeMap<String, String>,
) -> io::Result<codex_voice_settings::ResolvedAgentRuntime> {
    let runtime = value.runtime_settings().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Global Voice Secretary is not configured",
        )
    })?;
    codex_voice_settings::resolve(home, &runtime, environment)
}

/// Nonsecret global preferences, projected into the existing Group workflow view.
/// Group-owned documents and tasks do not own a second configuration.
pub fn assistant_view(home: &HomeLayout) -> io::Result<Value> {
    let config = settings::load(home)?.voice_secretary.config;
    let mut config = serde_json::to_value(config).expect("Secretary preferences serialize");
    config["auto_document_enabled"] = json!(true);
    config["document_default_dir"] = json!("docs/voice-secretary");
    config["auto_document_quiet_ms"] = json!(5000);
    config["auto_document_min_chars"] = json!(700);
    config["retention_ttl_seconds"] = json!(900);
    config["tts_enabled"] = json!(false);
    Ok(
        json!({"assistant_id":"voice_secretary","kind":"voice_secretary","enabled":true,"principal":"assistant:voice_secretary","config":config}),
    )
}

pub fn guidance(home: &HomeLayout) -> io::Result<String> {
    let text = settings::load(home)?.voice_secretary.config.guidance;
    Ok(if text.trim().is_empty() {
        include_str!("../../../resources/voice-secretary-guidance.md").to_owned()
    } else {
        text
    })
}

fn secret_path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join("state/secrets/voice-secretary.json")
}

pub fn private_environment(home: &HomeLayout) -> io::Result<BTreeMap<String, String>> {
    match fs::read_json(&secret_path(home)) {
        Ok(values) => Ok(values),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error),
    }
}

pub fn replace_private_environment(
    home: &HomeLayout,
    values: &BTreeMap<String, String>,
) -> io::Result<()> {
    codex_voice_settings::validate_private_environment(values)?;
    let path = secret_path(home);
    fs::with_exclusive_lock(&path.with_extension("json.lock"), || {
        if values.is_empty() {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            };
        }
        std::fs::create_dir_all(path.parent().expect("secret parent"))?;
        fs::write_secret_json(&path, values)
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
