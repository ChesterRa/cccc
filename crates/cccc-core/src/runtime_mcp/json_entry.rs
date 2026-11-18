//! The `cccc` entry of an `mcpServers` JSON file, for runtimes without an
//! `mcp add` command (Kimi Code and Cursor). Each runtime supplies its file
//! locations, its expected entry and the environment a registered entry must
//! have; the files are read, locked and rewritten here the same way for both.
use serde_json::{Map, Value, json};
use std::io;
use std::path::{Path, PathBuf};

/// Whether a registered entry's `env` (if any) fits the expected one.
pub(super) type EnvMatches = fn(Option<&Value>, &Value) -> bool;

/// Installs or replaces `cccc` in the user file unless an effective project
/// file defines it: a matching project entry is used as is, any other one is
/// an error, and the project file is never changed. Malformed JSON and
/// unrelated servers are preserved.
pub(super) fn ensure(
    label: &str,
    user: PathBuf,
    project: &Path,
    expected: Value,
    env_matches: EnvMatches,
) -> io::Result<PathBuf> {
    // Resolve existing aliases before comparing, locking, or replacing a file.
    // Keep the configured path for reporting and preserve user-owned symlinks.
    let resolved = std::fs::canonicalize(&user).unwrap_or_else(|_| user.clone());
    let project_document = read_document(label, project)?;
    if project != user.as_path()
        && std::fs::canonicalize(project).ok().as_ref() != Some(&resolved)
        && let Some(entry) =
            servers(&project_document, project)?.and_then(|servers| servers.get("cccc"))
    {
        if matches(entry, &expected, env_matches) {
            return Ok(project.to_path_buf());
        }
        return Err(io::Error::other(format!(
            "{label} project MCP entry `cccc` in {} overrides the user configuration; remove or correct it before starting the actor",
            project.display(),
        )));
    }

    crate::fs::with_exclusive_lock(&resolved.with_extension("cccc.lock"), || {
        let mut document = read_document(label, &resolved)?;
        if servers(&document, &resolved)?
            .and_then(|servers| servers.get("cccc"))
            .is_some_and(|entry| matches(entry, &expected, env_matches))
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
        crate::fs::write_json(&resolved, &document)
    })?;
    Ok(user)
}

fn read_document(label: &str, path: &Path) -> io::Result<Map<String, Value>> {
    let document = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid {label} MCP JSON at {}: {error}", path.display()),
            )
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error),
    };
    document.as_object().cloned().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{label} MCP configuration at {} must be an object",
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

fn matches(entry: &Value, expected: &Value, env_matches: EnvMatches) -> bool {
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
        && env_matches(entry.get("env"), &expected["env"])
}
