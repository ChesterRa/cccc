//! Grok's ACP client and native terminal reload the same native MCP registry.
//! Register only our server, keeping vendor imports and Actor identity separate.
use serde_json::Value;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const MCP_COMMAND: &str = "${CCCC_CLI:-cccc}";

pub fn ensure(
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    executable: &Path,
    grok: &str,
) -> io::Result<PathBuf> {
    let mut environment: BTreeMap<String, String> = std::env::vars()
        .chain(
            environment
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )
        .collect();
    environment.insert("CCCC_CLI".into(), executable.to_string_lossy().into_owned());
    let config = config_path(cwd, &environment)?;
    let grok =
        super::resolve_program_in(grok, environment.get("PATH").map(std::ffi::OsStr::new), cwd);
    ensure_entry(&config, executable, |args| {
        let output = cccc_runtime::capture_command_blocking(
            Command::new(&grok)
                .args(args)
                .current_dir(cwd)
                .envs(&environment),
            None,
            Duration::from_secs(10),
            2_000_000,
        )?;
        // Provider output can include private configuration. Keep it out of
        // errors returned through the daemon and CLI.
        if !output.status.success() || output.stdout_truncated || output.stderr_truncated {
            return Err(io::Error::other(
                "Grok MCP setup failed; check `grok inspect` and that this Grok version supports `mcp add`",
            ));
        }
        Ok(output.stdout)
    })
}

fn config_path(cwd: &Path, environment: &BTreeMap<String, String>) -> io::Result<PathBuf> {
    let configured = environment
        .get("GROK_HOME")
        .filter(|value| !value.is_empty());
    let root = if let Some(root) = configured {
        PathBuf::from(root)
    } else {
        let keys = if cfg!(windows) {
            ["USERPROFILE", "HOME"]
        } else {
            ["HOME", "USERPROFILE"]
        };
        let home = keys
            .iter()
            .find_map(|key| environment.get(*key).filter(|value| !value.is_empty()))
            .ok_or_else(|| io::Error::other("Grok MCP setup requires a user home directory"))?;
        Path::new(home).join(".grok")
    };
    Ok(if root.is_absolute() {
        root
    } else {
        cwd.join(root)
    }
    .join("config.toml"))
}

fn ensure_entry(
    config: &Path,
    executable: &Path,
    mut run: impl FnMut(&[&str]) -> io::Result<Vec<u8>>,
) -> io::Result<PathBuf> {
    // Native `mcp add` replaces malformed TOML with a new document. Validate
    // first so automatic repair cannot erase unrelated user configuration.
    let configured = read_entry(config)?.as_ref().is_some_and(matches);
    if let Some(path) = inspect(config, executable, &mut run)? {
        return Ok(path);
    }
    if configured {
        return Err(unavailable());
    }
    let resolved = std::fs::canonicalize(config).unwrap_or_else(|_| config.to_owned());
    crate::fs::with_exclusive_lock(&resolved.with_extension("cccc.lock"), || {
        let configured = read_entry(config)?.as_ref().is_some_and(matches);
        if let Some(path) = inspect(config, executable, &mut run)? {
            return Ok(path);
        }
        if configured {
            return Err(unavailable());
        }
        run(&[
            "mcp",
            "add",
            "--scope",
            "user",
            "cccc",
            "--",
            MCP_COMMAND,
            "mcp",
        ])?;
        inspect(config, executable, &mut run)?.ok_or_else(unavailable)
    })
}

fn unavailable() -> io::Error {
    io::Error::other(
        "Grok did not expose the configured CCCC MCP server; check `grok inspect` for project overrides, disabled servers, or managed policies",
    )
}

fn inspect(
    config: &Path,
    executable: &Path,
    run: &mut impl FnMut(&[&str]) -> io::Result<Vec<u8>>,
) -> io::Result<Option<PathBuf>> {
    let invalid = || io::Error::other("Grok returned an invalid MCP configuration report");
    let report: Value =
        serde_json::from_slice(&run(&["inspect", "--json"])?).map_err(|_| invalid())?;
    let entries = report["mcpServers"].as_array().ok_or_else(invalid)?;
    let Some(entry) = entries.iter().find(|entry| entry["name"] == "cccc") else {
        return Ok(None);
    };
    if !entry["disabledReason"].is_null() {
        return Err(blocked());
    }
    if entry["source"]["type"] != "configToml" {
        // The explicit native entry wins over imported Claude/Cursor/plugin
        // definitions without changing those other configuration sources.
        return Ok(None);
    }
    let path = entry["source"]["path"].as_str().ok_or_else(invalid)?;
    let path = Path::new(path);
    if read_entry(path)?.as_ref().is_some_and(matches)
        && entry["transport"] == "stdio"
        && entry["target"].as_str() == executable.to_str()
    {
        verify_effective(executable, run)?;
        return Ok(Some(path.to_owned()));
    }
    if path != config && !same_file::is_same_file(path, config).unwrap_or(false) {
        return Err(io::Error::other(format!(
            "Grok project MCP entry `cccc` in {} overrides the user configuration; remove or correct it before starting the Actor",
            path.display()
        )));
    }
    Ok(None)
}

fn blocked() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Grok reports the CCCC MCP server as blocked; check `grok inspect` and the native MCP policy before starting the Actor",
    )
}

fn verify_effective(
    executable: &Path,
    run: &mut impl FnMut(&[&str]) -> io::Result<Vec<u8>>,
) -> io::Result<()> {
    // Native list resolves version overrides and environment expansion. The
    // discovery report omits arguments/env; the raw table is not authoritative.
    let invalid =
        || io::Error::other("Grok returned an invalid effective MCP configuration report");
    let report: Value =
        serde_json::from_slice(&run(&["mcp", "list", "--json"])?).map_err(|_| invalid())?;
    let entries = report.as_array().ok_or_else(invalid)?;
    let entry = entries
        .iter()
        .find(|entry| entry["name"] == "cccc")
        .ok_or_else(invalid)?;
    if !entry["blocked_reason"].is_null() {
        return Err(blocked());
    }
    if entry["command"].as_str() == executable.to_str()
        && entry["args"] == serde_json::json!(["mcp"])
        && entry["enabled"] == true
        && entry.get("url").is_none()
        && entry.get("env").is_none_or(|env| {
            env.as_object()
                .is_some_and(|env| env.keys().all(|key| !reserved_environment(key)))
        })
    {
        return Ok(());
    }
    Err(io::Error::other(
        "Grok's effective MCP entry `cccc` overrides its executable, arguments, or process identity; check `grok mcp list --json` and correct conflicting version/project overrides before starting the Actor",
    ))
}

fn reserved_environment(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    key.starts_with("CCCC_") || key == "PATH"
}

fn read_entry(path: &Path) -> io::Result<Option<toml::Value>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !path.is_symlink() => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid Grok MCP configuration at {}; correct the TOML before setup",
                path.display()
            ),
        )
    };
    let document = raw.parse::<toml::Table>().map_err(|_| invalid())?;
    let Some(servers) = document.get("mcp_servers") else {
        return Ok(None);
    };
    let servers = servers.as_table().ok_or_else(invalid)?;
    let entry = servers.get("cccc");
    if entry.is_some_and(|entry| !entry.is_table()) {
        return Err(invalid());
    }
    Ok(entry.cloned())
}

fn matches(entry: &toml::Value) -> bool {
    entry.get("command").and_then(toml::Value::as_str) == Some(MCP_COMMAND)
        && entry
            .get("args")
            .and_then(toml::Value::as_array)
            .is_some_and(|args| args.len() == 1 && args[0].as_str() == Some("mcp"))
        && entry
            .get("enabled")
            .is_none_or(|value| value.as_bool() == Some(true))
        && entry.get("url").is_none()
        && entry.get("env").is_none_or(|value| {
            value
                .as_table()
                .is_some_and(|values| values.keys().all(|key| !reserved_environment(key)))
        })
}

#[cfg(test)]
mod tests;
