use super::*;
use cccc_contracts::voice_secretary::VoiceSecretarySettings;
use cccc_core::fs;
use std::collections::BTreeMap;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct SourcePolicy {
    held_source_baselines: BTreeMap<String, u64>,
}

fn source_policy(home: &HomeLayout) -> io::Result<SourcePolicy> {
    let path = home.root().join("voice-secretary/source-policy.json");
    if path.exists() {
        fs::read_json(&path)
    } else {
        Ok(SourcePolicy::default())
    }
}

pub(super) fn held_baselines(home: &HomeLayout) -> io::Result<BTreeMap<String, u64>> {
    Ok(source_policy(home)?.held_source_baselines)
}

pub(super) fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    if string_arg(request, "by").is_some_and(|by| by != "user") {
        return Err(OpError::new(
            "permission_denied",
            "Only the user can manage the global secretary",
        ));
    }
    Ok(())
}

pub(super) fn readiness_error(home: &HomeLayout) -> Option<String> {
    resolve_runtime(home)
        .err()
        .map(|error| error.to_string())
        .or_else(|| owner_readiness_error(home))
}

pub(super) fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let settings = settings::load(home).map_err(OpError::io)?.voice_secretary;
    let profiles = ProfileStore::new(home.clone())
        .map_err(OpError::io)?
        .list()
        .map_err(OpError::io)?
        .into_iter()
        .filter(|profile| {
            serde_json::from_value(profile["runtime"].clone()).is_ok_and(|runtime| {
                serde_json::from_value(
                    profile
                        .get("runtime_mode")
                        .cloned()
                        .unwrap_or(json!("default")),
                )
                .is_ok_and(|mode| {
                    cccc_core::codex_voice_settings::supports_structured_runtime(runtime, mode)
                })
            })
        })
        .collect::<Vec<_>>();
    let readiness_error = readiness_error(home);
    let held = held_baselines(home).map_err(OpError::io)?;
    let sources = lookup(home).map_or_else(sources::SourceCounts::default, |manager| {
        manager.source_counts(None, &held)
    });
    object(
        json!({"settings":settings,"configured":readiness_error.is_none(),"readiness_error":readiness_error,
        "profiles":profiles,"default_guidance":include_str!("../../../../../resources/voice-secretary-guidance.md").trim(),"environment_keys":cccc_core::voice_secretary_settings::private_environment(home).map_err(OpError::io)?.keys().collect::<Vec<_>>(),"backlog_sources":sources.deferred + sources.held,
        "held_sources":sources.held,"invalid_sources":sources.invalid}),
    )
}

pub(super) fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    if let Some(patch) = request.args.get("preferences") {
        if ["settings", "environment", "backlog_action"]
            .iter()
            .any(|key| request.args.contains_key(*key))
        {
            return Err(OpError::new(
                "invalid_args",
                "A preferences update cannot include Runtime, environment or backlog changes",
            ));
        }
        let patch = patch
            .as_object()
            .ok_or_else(|| OpError::new("invalid_args", "preferences must be an object"))?;
        settings::update(home, |global| {
            let mut config = serde_json::to_value(&global.voice_secretary.config)?;
            let fields = config.as_object_mut().expect("preferences object");
            for (key, value) in patch {
                if !fields.contains_key(key) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("Unknown Secretary preference: {key}"),
                    ));
                }
                fields.insert(key.clone(), value.clone());
            }
            let next = VoiceSecretarySettings {
                config: serde_json::from_value(config)?,
                ..global.voice_secretary.clone()
            };
            global.voice_secretary.config =
                cccc_core::voice_secretary_settings::normalize(next)?.config;
            Ok(())
        })
        .map_err(OpError::invalid)?;
        // Only preferences change. Do not replace credentials, source policy,
        // or a saved Runtime that may currently be unavailable.
        return get(home, request);
    }
    let previous = settings::load(home).map_err(OpError::io)?.voice_secretary;
    let mut value = request
        .args
        .get("settings")
        .cloned()
        .ok_or_else(|| OpError::new("invalid_args", "settings or preferences is required"))?;
    if let Some(fields) = value.as_object_mut()
        && !fields.contains_key("config")
    {
        fields.insert("config".into(), json!(previous.config));
    }
    let setting = cccc_core::voice_secretary_settings::normalize(
        serde_json::from_value::<VoiceSecretarySettings>(value).map_err(OpError::invalid)?,
    )
    .map_err(OpError::invalid)?;
    let prior_environment =
        cccc_core::voice_secretary_settings::private_environment(home).map_err(OpError::io)?;
    let patch = request
        .args
        .get("environment")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let patch: EnvironmentPatch = serde_json::from_value(patch).map_err(OpError::invalid)?;
    let baseline = if patch.clear {
        BTreeMap::new()
    } else {
        prior_environment.clone()
    };
    let environment = cccc_core::codex_voice_settings::patched_private_environment(
        &baseline,
        patch.set,
        &patch.unset,
    )
    .map_err(OpError::invalid)?;
    let configured = setting.runtime_settings().is_some();
    if configured {
        cccc_core::voice_secretary_settings::resolve(home, &setting, &environment)
            .map_err(OpError::invalid)?;
    }
    let runtime_changed = previous.runtime_settings() != setting.runtime_settings()
        || prior_environment != environment;
    let backlog_action = match request.args.get("backlog_action") {
        None => None,
        Some(Value::String(action)) if matches!(action.as_str(), "process" | "hold") => {
            Some(action.as_str())
        }
        _ => {
            return Err(OpError::new(
                "invalid_args",
                "backlog_action must be process or hold",
            ));
        }
    };
    let manager = lookup(home);
    let _acceptance = manager
        .as_ref()
        .map(|m| m.acceptance.lock().unwrap_or_else(|e| e.into_inner()));
    let mut policy = source_policy(home).map_err(OpError::io)?;
    let source_counts = manager
        .as_ref()
        .map_or_else(sources::SourceCounts::default, |m| {
            m.source_counts(None, &policy.held_source_baselines)
        });
    if configured
        && runtime_changed
        && source_counts.deferred + source_counts.held > 0
        && backlog_action.is_none()
    {
        return Err(OpError::new(
            "secretary_backlog_choice_required",
            "Choose whether to process saved inputs or retain them while processing only new input",
        ));
    }
    if configured {
        match backlog_action {
            Some("process") => policy.held_source_baselines.clear(),
            Some("hold") => {
                if let Some(m) = &manager {
                    m.hold_sources(&mut policy.held_source_baselines);
                }
            }
            _ => {}
        }
    }
    // Admission uses this mutex too. Persist the source boundary before enabling
    // a Profile, and restore it if the settings write fails.
    let path = home.root().join("voice-secretary/source-policy.json");
    let had_policy = path.exists();
    let prior = source_policy(home).map_err(OpError::io)?;
    fs::write_json(&path, &policy).map_err(OpError::io)?;
    if let Err(error) =
        cccc_core::voice_secretary_settings::replace_private_environment(home, &environment)
    {
        restore_policy(home, &path, had_policy, &prior).map_err(OpError::io)?;
        return Err(OpError::io(error));
    }
    if let Err(error) = settings::update(home, |global| {
        global.voice_secretary = setting;
        Ok(())
    }) {
        cccc_core::voice_secretary_settings::replace_private_environment(home, &prior_environment)
            .map_err(OpError::io)?;
        restore_policy(home, &path, had_policy, &prior).map_err(OpError::io)?;
        return Err(OpError::io(error));
    }
    if let Some(m) = &manager {
        m.notify();
    }
    get(home, request)
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EnvironmentPatch {
    set: BTreeMap<String, String>,
    unset: Vec<String>,
    clear: bool,
}

fn restore_policy(
    _home: &HomeLayout,
    path: &std::path::Path,
    existed: bool,
    prior: &SourcePolicy,
) -> io::Result<()> {
    if existed {
        fs::write_json(path, prior)
    } else {
        std::fs::remove_file(path)
    }
}
