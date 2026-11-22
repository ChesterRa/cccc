//! Cursor CLI has no `mcp add` command and starts MCP servers without the CCCC
//! variables of its own environment. The user entry therefore maps each
//! context variable with Cursor's `${env:NAME}` interpolation instead of a
//! fixed value: a member's server receives that member's values, and a Cursor
//! session outside CCCC passes the placeholders verbatim, which CCCC reads as
//! unset. Actor startup writes the entry before Cursor starts.
//!
//! A project `cccc` entry takes precedence over the user entry, but Cursor
//! reads only the workspace root's `.cursor/mcp.json`: the top of the
//! enclosing Git work tree, or the working directory outside one. Files in
//! nested directories and above that root are not read (Cursor CLI 2026.11.11).
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::CONTEXT_VARIABLES as CONTEXT;

pub fn ensure(
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    executable: &Path,
) -> io::Result<PathBuf> {
    // Actor launch carries only overrides; the provider also inherits the
    // daemon environment. Resolve the home Cursor itself reads.
    let effective: BTreeMap<String, String> = cccc_runtime::unicode_environment()
        .chain(
            environment
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )
        .collect();
    let keys = if cfg!(windows) {
        ["USERPROFILE", "HOME"]
    } else {
        ["HOME", "USERPROFILE"]
    };
    let home = keys
        .iter()
        .find_map(|key| effective.get(*key).filter(|value| !value.is_empty()))
        .ok_or_else(|| io::Error::other("Cursor MCP setup requires a user home directory"))?;
    let root = Path::new(home).join(".cursor");
    let user = if root.is_absolute() {
        root
    } else {
        cwd.join(root)
    }
    .join("mcp.json");
    super::json_entry::ensure(
        "Cursor",
        user,
        &super::workspace_root(cwd).join(".cursor/mcp.json"),
        expected_entry(executable),
        env_matches,
    )
}

fn expected_entry(executable: &Path) -> Value {
    let env: Map<String, Value> = CONTEXT
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                Value::String(format!("${{env:{name}}}")),
            )
        })
        .collect();
    json!({"command":executable,"args":["mcp"],"env":env})
}

/// Every context variable maps to its own placeholder, and the shared user
/// file fixes no other CCCC variable.
fn env_matches(env: Option<&Value>, expected: &Value) -> bool {
    env.and_then(Value::as_object).is_some_and(|env| {
        CONTEXT
            .iter()
            .all(|name| env.get(*name) == expected.get(*name))
            && env
                .keys()
                .all(|key| !key.starts_with("CCCC_") || CONTEXT.contains(&key.as_str()))
    })
}

#[cfg(test)]
mod tests;
