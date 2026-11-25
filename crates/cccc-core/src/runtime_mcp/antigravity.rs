//! Native `agy mcp add` owns MCP registration. CLI setup and Actor launch also
//! disable the native feedback survey, which can consume automated terminal input.
use serde_json::Value;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

pub fn ensure(
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    add: impl FnOnce() -> io::Result<()>,
) -> io::Result<PathBuf> {
    let home = cccc_runtime::resolve_antigravity_home(cwd, environment, false)
        .ok_or_else(|| io::Error::other("Antigravity MCP setup requires a user home directory"))?;
    ensure_in_home(cwd, &home, add)
}

fn ensure_in_home(
    cwd: &Path,
    home: &Path,
    add: impl FnOnce() -> io::Result<()>,
) -> io::Result<PathBuf> {
    let path = home.join(".gemini/config/mcp_config.json");
    let configured = ensure_entry(cwd, &path, add)?;
    disable_feedback_survey(&home.join(".gemini/antigravity-cli/settings.json"))?;
    ensure_hooks(&home.join(".gemini/config/hooks.json"))?;
    Ok(configured)
}

fn ensure_entry(
    cwd: &Path,
    path: &Path,
    add: impl FnOnce() -> io::Result<()>,
) -> io::Result<PathBuf> {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let global = read_entry(&resolved)?;
    let project = cwd.join(".agents/mcp_config.json");
    if project != path
        && std::fs::canonicalize(&project).ok().as_ref() != Some(&resolved)
        && let Some(entry) = read_entry(&project)?
    {
        if matches(&entry) {
            return Ok(project);
        }
        return Err(io::Error::other(format!(
            "Antigravity project MCP entry `cccc` in {} overrides the global configuration; remove or correct it before starting the Actor",
            project.display()
        )));
    }
    // Already configured launches are read-only. Take the shared write lock
    // only for repair, then reread in case another instance repaired it first.
    if global.as_ref().is_some_and(matches) {
        return Ok(path.to_owned());
    }
    crate::fs::with_exclusive_lock(&resolved.with_extension("cccc.lock"), || {
        if read_entry(&resolved)?.as_ref().is_some_and(matches) {
            return Ok(());
        }
        add()?;
        if !read_entry(&resolved)?.as_ref().is_some_and(matches) {
            return Err(io::Error::other(format!(
                "Antigravity MCP setup did not produce an enabled `cccc mcp` entry in {} with inherited Actor environment",
                path.display()
            )));
        }
        Ok(())
    })?;
    Ok(path.to_owned())
}

fn disable_feedback_survey(path: &Path) -> io::Result<()> {
    let resolved = match std::fs::canonicalize(path) {
        Ok(resolved) => resolved,
        Err(error) if path.is_symlink() => return Err(error),
        Err(_) => path.to_owned(),
    };
    if read_settings(&resolved)?.get("showFeedbackSurvey") == Some(&Value::Bool(false)) {
        return Ok(());
    }
    // This is an upstream user preference, not a process-local override. Preserve
    // every other preference and serialize concurrent CCCC launches before writing.
    crate::fs::with_exclusive_lock(&resolved.with_extension("cccc.lock"), || {
        let mut settings = read_settings(&resolved)?;
        if settings.get("showFeedbackSurvey") != Some(&Value::Bool(false)) {
            settings.insert("showFeedbackSurvey".into(), Value::Bool(false));
            crate::fs::write_json(&resolved, &settings)?;
        }
        Ok(())
    })
}

fn read_settings(path: &Path) -> io::Result<serde_json::Map<String, Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error),
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!(
            "invalid Antigravity settings at {}; correct the JSON before starting the Actor",
            path.display()
        )))
}

fn ensure_hooks(path: &Path) -> io::Result<()> {
    let resolved = match std::fs::canonicalize(path) {
        Ok(resolved) => resolved,
        Err(error) if path.is_symlink() => return Err(error),
        Err(_) => path.to_owned(),
    };
    let handler = crate::antigravity_hook::hook_handler();
    if hook_entry_ready(
        read_hooks(&resolved)?.get(crate::antigravity_hook::HOOK_ENTRY_NAME),
        &handler,
        &resolved,
    )? {
        return Ok(());
    }
    // Preserve all user-defined hooks (including extra events or handlers under
    // the same group name) and serialize concurrent CCCC launches before writing.
    crate::fs::with_exclusive_lock(&resolved.with_extension("cccc.lock"), || {
        let mut hooks = read_hooks(&resolved)?;
        if hook_entry_ready(
            hooks.get(crate::antigravity_hook::HOOK_ENTRY_NAME),
            &handler,
            &resolved,
        )? {
            return Ok(());
        }
        let merged = merge_hook_entry(
            hooks.get(crate::antigravity_hook::HOOK_ENTRY_NAME),
            &handler,
            &resolved,
        )?;
        hooks.insert(crate::antigravity_hook::HOOK_ENTRY_NAME.into(), merged);
        crate::fs::write_json(&resolved, &hooks)
    })
}

const TRACKED_HOOK_EVENTS: [&str; 3] = ["SessionStart", "PreInvocation", "Stop"];

fn hook_entry_ready(existing: Option<&Value>, handler: &Value, path: &Path) -> io::Result<bool> {
    let Some(entry) = existing else {
        return Ok(false);
    };
    let object = validate_hook_group(entry, path)?;
    Ok(TRACKED_HOOK_EVENTS.iter().all(|event| {
        object
            .get(*event)
            .and_then(Value::as_array)
            .is_some_and(|handlers| handlers.iter().any(|item| item == handler))
    }))
}

fn merge_hook_entry(existing: Option<&Value>, handler: &Value, path: &Path) -> io::Result<Value> {
    let mut object = match existing {
        None => serde_json::Map::new(),
        Some(entry) => validate_hook_group(entry, path)?.clone(),
    };
    for event in TRACKED_HOOK_EVENTS {
        match object.get_mut(event) {
            None => {
                object.insert(event.into(), Value::Array(vec![handler.clone()]));
            }
            Some(Value::Array(handlers)) => {
                if !handlers.iter().any(|item| item == handler) {
                    handlers.push(handler.clone());
                }
            }
            Some(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "invalid Antigravity hook event `{event}` under `{}` in {}; expected an array",
                        crate::antigravity_hook::HOOK_ENTRY_NAME,
                        path.display()
                    ),
                ));
            }
        }
    }
    Ok(Value::Object(object))
}

fn validate_hook_group<'a>(
    entry: &'a Value,
    path: &Path,
) -> io::Result<&'a serde_json::Map<String, Value>> {
    let object = entry.as_object().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid Antigravity hook group `{}` in {}; expected a JSON object",
                crate::antigravity_hook::HOOK_ENTRY_NAME,
                path.display()
            ),
        )
    })?;
    if object.get("enabled") == Some(&Value::Bool(false)) {
        return Err(io::Error::other(format!(
            "Antigravity hook group `{}` in {} is disabled (`enabled: false`); re-enable or remove it before starting the Actor",
            crate::antigravity_hook::HOOK_ENTRY_NAME,
            path.display()
        )));
    }
    for event in TRACKED_HOOK_EVENTS {
        if let Some(value) = object.get(event)
            && !value.is_array()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "invalid Antigravity hook event `{event}` under `{}` in {}; expected an array",
                    crate::antigravity_hook::HOOK_ENTRY_NAME,
                    path.display()
                ),
            ));
        }
    }
    Ok(object)
}

fn read_hooks(path: &Path) -> io::Result<serde_json::Map<String, Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error),
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!(
            "invalid Antigravity hooks configuration at {}; correct the JSON before starting the Actor",
            path.display()
        )))
}

fn read_entry(path: &Path) -> io::Result<Option<Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // Antigravity's first run leaves an empty file, which it reads as having no
    // servers; `agy mcp add` then fills it in.
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid Antigravity MCP configuration at {}",
                path.display()
            ),
        )
    };
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let document = document.as_object().ok_or_else(invalid)?;
    document
        .get("mcpServers")
        .map(|servers| {
            servers
                .as_object()
                .map(|servers| servers.get("cccc").cloned())
                .ok_or_else(invalid)
        })
        .transpose()
        .map(Option::flatten)
}

fn matches(entry: &Value) -> bool {
    // One global entry can serve several CCCC installs. Actor launch supplies
    // its owning CLI on PATH; neither the executable nor identity is pinned to
    // whichever instance last repaired this shared registration.
    entry["command"] == "cccc"
        && entry["args"] == serde_json::json!(["mcp"])
        && entry["disabled"] != true
        && entry["enabled"] != false
        && entry.get("serverUrl").is_none()
        && entry.get("cwd").is_none()
        && entry
            .get("disabledTools")
            .is_none_or(|value| value == &serde_json::json!([]))
        && entry.get("env").is_none_or(|value| {
            value.as_object().is_some_and(|env| {
                env.keys()
                    .all(|key| !key.starts_with("CCCC_") && key != "PATH")
            })
        })
}

#[cfg(test)]
mod tests;
