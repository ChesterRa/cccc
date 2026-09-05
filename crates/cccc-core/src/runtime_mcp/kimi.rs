//! Kimi Code has no `mcp add` command. CLI setup and Actor startup share this
//! config-file path; neither probes old Python Kimi directories nor edits a
//! project's higher-priority MCP configuration.
//! Integration regression report: https://github.com/ChesterRa/cccc/pull/97.
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

pub fn ensure(
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    executable: &Path,
) -> io::Result<PathBuf> {
    // Actor launch carries only overrides; native child processes also
    // inherit the daemon environment. Resolve exactly that effective home.
    let effective: BTreeMap<String, String> = std::env::vars()
        .chain(
            environment
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )
        .collect();
    let environment = &effective;
    let home = environment
        .get("CCCC_HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("CCCC_HOME is required for Kimi Code MCP setup"))?;
    let expected = json!({"command":executable,"args":["mcp"],"env":{"CCCC_HOME":home}});
    let path = user_config_path(cwd, environment)?;
    // Resolve existing aliases before comparing, locking, or replacing a file.
    // Keep the configured path for reporting and preserve user-owned symlinks.
    let resolved_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let project = cwd.join(".kimi-code/mcp.json");
    let project_document = read_document(&project)?;
    if project != path
        && std::fs::canonicalize(&project).ok().as_ref() != Some(&resolved_path)
        && let Some(entry) =
            servers(&project_document, &project)?.and_then(|servers| servers.get("cccc"))
    {
        if matches(entry, &expected) {
            return Ok(project);
        }
        return Err(io::Error::other(format!(
            "Kimi Code project MCP entry `cccc` in {} overrides the user configuration; remove or correct it before starting the actor",
            project.display(),
        )));
    }

    crate::fs::with_exclusive_lock(&resolved_path.with_extension("cccc.lock"), || {
        let mut document = read_document(&resolved_path)?;
        if servers(&document, &resolved_path)?
            .and_then(|servers| servers.get("cccc"))
            .is_some_and(|entry| matches(entry, &expected))
        {
            return Ok(());
        }
        let servers = document.entry("mcpServers").or_insert_with(|| json!({}));
        // read_document/servers already validated any existing object. Never
        // replace malformed JSON or unrelated provider/server configuration.
        servers
            .as_object_mut()
            .expect("validated MCP object")
            .insert("cccc".into(), expected);
        crate::fs::write_json(&resolved_path, &document)
    })?;
    Ok(path)
}

fn user_config_path(cwd: &Path, environment: &BTreeMap<String, String>) -> io::Result<PathBuf> {
    let configured = environment
        .get("KIMI_CODE_HOME")
        .filter(|value| !value.is_empty());
    let root = if let Some(path) = configured {
        PathBuf::from(path)
    } else {
        let keys = if cfg!(windows) {
            ["USERPROFILE", "HOME"]
        } else {
            ["HOME", "USERPROFILE"]
        };
        let home = keys
            .iter()
            .find_map(|key| environment.get(*key).filter(|value| !value.is_empty()))
            .ok_or_else(|| {
                io::Error::other(
                    "Kimi Code MCP setup requires KIMI_CODE_HOME or a user home directory",
                )
            })?;
        Path::new(home).join(".kimi-code")
    };
    Ok(if root.is_absolute() {
        root
    } else {
        cwd.join(root)
    }
    .join("mcp.json"))
}

fn read_document(path: &Path) -> io::Result<Map<String, Value>> {
    let document = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid Kimi Code MCP JSON at {}: {error}", path.display()),
            )
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error),
    };
    document.as_object().cloned().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Kimi Code MCP configuration at {} must be an object",
                path.display()
            ),
        )
    })
}

fn servers<'a>(
    document: &'a Map<String, Value>,
    path: &Path,
) -> io::Result<Option<&'a Map<String, Value>>> {
    document
        .get("mcpServers")
        .map(|value| {
            value.as_object().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("mcpServers at {} must be an object", path.display()),
                )
            })
        })
        .transpose()
}

fn matches(entry: &Value, expected: &Value) -> bool {
    let same_command = entry["command"] == expected["command"]
        || (entry["command"]
            .as_str()
            .and_then(|path| std::fs::canonicalize(path).ok())
            .zip(
                expected["command"]
                    .as_str()
                    .and_then(|path| std::fs::canonicalize(path).ok()),
            )
            .is_some_and(|(actual, expected)| actual == expected));
    same_command
        && entry["args"] == expected["args"]
        && entry["enabled"] != false
        && entry["disabled"] != true
        && entry.get("url").is_none()
        && entry.get("cwd").is_none()
        && entry.get("transport").is_none_or(|value| value == "stdio")
        && entry.get("env").is_none_or(|env| {
            env.as_object().is_some_and(|env| {
                env.iter().all(|(key, value)| {
                    !key.starts_with("CCCC_")
                        || (key == "CCCC_HOME" && value == &expected["env"]["CCCC_HOME"])
                })
            })
        })
}

#[cfg(test)]
mod tests;
