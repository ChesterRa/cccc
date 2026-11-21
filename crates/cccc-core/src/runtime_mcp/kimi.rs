//! Kimi Code has no `mcp add` command. CLI setup and Actor startup share this
//! config-file path; neither probes old Python Kimi directories nor edits a
//! project's higher-priority MCP configuration.
//! Integration regression report: https://github.com/ChesterRa/cccc/pull/97.
use serde_json::{Value, json};
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
    let effective: BTreeMap<String, String> = cccc_runtime::unicode_environment()
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
    super::json_entry::ensure(
        "Kimi Code",
        path,
        &cwd.join(".kimi-code/mcp.json"),
        expected,
        env_matches,
    )
}

/// An entry may leave `env` out; a CCCC variable it does set must be this
/// instance's home.
fn env_matches(env: Option<&Value>, expected: &Value) -> bool {
    env.is_none_or(|env| {
        env.as_object().is_some_and(|env| {
            env.iter().all(|(key, value)| {
                !key.starts_with("CCCC_") || (key == "CCCC_HOME" && value == &expected["CCCC_HOME"])
            })
        })
    })
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

#[cfg(test)]
mod tests;
