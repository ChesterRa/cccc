use crate::dispatch::OpError;
use serde_yaml::Value as YamlValue;
use std::io;
use std::path::{Path, PathBuf};

use super::{PLACEHOLDERS, SERVER};

/// The `cccc` server entry of one Hermes config.
pub(super) enum PersistentEntry {
    Absent,
    /// Exactly what earlier CCCC versions wrote: this CCCC's start command,
    /// `mcp`, the three identity placeholders and nothing else.
    WrittenByCccc,
    /// Anything else named `cccc`: other fields, another path, fixed values.
    Other,
}

/// A config that does not parse is reported, never read as empty: nothing
/// may be removed on the strength of a file CCCC could not read.
pub(super) fn persistent_entry(path: &Path) -> Result<PersistentEntry, OpError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PersistentEntry::Absent);
        }
        Err(error) => {
            return Err(OpError::new(
                "hermes_config_unreadable",
                format!("cannot read Hermes config {}: {error}", path.display()),
            ));
        }
    };
    let invalid = |detail: String| {
        OpError::new(
            "hermes_config_invalid",
            format!(
                "Hermes config {} cannot be parsed ({detail}); CCCC left it unchanged",
                path.display()
            ),
        )
    };
    let config: YamlValue =
        serde_yaml::from_str(&text).map_err(|error| invalid(error.to_string()))?;
    let servers = match &config {
        YamlValue::Null => return Ok(PersistentEntry::Absent),
        YamlValue::Mapping(config) => config.get(key("mcp_servers")),
        _ => return Err(invalid("the document is not a mapping".into())),
    };
    let entry = match servers {
        None | Some(YamlValue::Null) => return Ok(PersistentEntry::Absent),
        Some(YamlValue::Mapping(servers)) => servers.get(key(SERVER)),
        Some(_) => return Err(invalid("mcp_servers is not a mapping".into())),
    };
    Ok(match entry {
        None => PersistentEntry::Absent,
        Some(entry) if written_by_cccc(entry) => PersistentEntry::WrittenByCccc,
        Some(_) => PersistentEntry::Other,
    })
}

fn written_by_cccc(entry: &YamlValue) -> bool {
    let Some(entry) = entry.as_mapping() else {
        return false;
    };
    let field = |name: &str| entry.get(key(name));
    let known_fields = entry
        .keys()
        .all(|name| matches!(name.as_str(), Some("command" | "args" | "env" | "enabled")));
    let command = field("command")
        .and_then(YamlValue::as_str)
        .is_some_and(is_current_cccc);
    let args = field("args")
        .and_then(YamlValue::as_sequence)
        .is_some_and(|args| matches!(args.as_slice(), [arg] if arg.as_str() == Some("mcp")));
    let env = field("env")
        .and_then(YamlValue::as_mapping)
        .is_some_and(|env| {
            env.len() == PLACEHOLDERS.len()
                && PLACEHOLDERS.iter().all(|(name, value)| {
                    env.get(key(name)).and_then(YamlValue::as_str) == Some(*value)
                })
        });
    let enabled = field("enabled").is_none_or(|enabled| enabled.as_bool() == Some(true));
    known_fields && command && args && env && enabled
}

/// The saved command is the CCCC that starts members now (the same file).
fn is_current_cccc(command: &str) -> bool {
    let Some(current) = crate::ops::codex_mcp::resolve_cccc_executable() else {
        return false;
    };
    matches!(
        (std::fs::canonicalize(command), std::fs::canonicalize(current)),
        (Ok(saved), Ok(current)) if saved == current
    )
}

fn key(name: &str) -> YamlValue {
    YamlValue::String(name.into())
}

pub(super) fn hermes_home() -> PathBuf {
    std::env::var_os("HERMES_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".hermes")))
        .unwrap_or_else(|| PathBuf::from(".hermes"))
}

pub(super) fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH");
    cccc_core::runtime_mcp::find_program(name, path.as_deref())
}
