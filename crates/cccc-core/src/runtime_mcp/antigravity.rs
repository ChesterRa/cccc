//! Native `agy mcp add` owns MCP registration. Session-only feedback survey
//! suppression is passed via CLI `--settings` on native PTY launch rather than
//! mutating the user's `~/.gemini/antigravity-cli/settings.json`.
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
    ensure_entry(cwd, &path, add)
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
