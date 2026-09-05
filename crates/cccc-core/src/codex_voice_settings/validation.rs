use cccc_contracts::CodexVoiceAnalystSettings;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

const MAX_COMMAND_ARGS: usize = 256;
const MAX_COMMAND_BYTES: usize = 64 * 1024;
const MAX_ENV_KEYS: usize = 256;
const MAX_ENV_VALUE_CHARS: usize = 200_000;

pub(super) fn normalize(value: &mut CodexVoiceAnalystSettings) -> io::Result<()> {
    if value.command.len() > MAX_COMMAND_ARGS {
        return invalid("runtime command has too many arguments");
    }
    let mut total = 0usize;
    for argument in &mut value.command {
        *argument = argument.trim().to_owned();
        if argument.is_empty() || argument.contains('\0') || argument.len() > 8192 {
            return invalid("runtime command contains an invalid argument");
        }
        total = total.saturating_add(argument.len());
    }
    if total > MAX_COMMAND_BYTES {
        return invalid("runtime command is too large");
    }

    value.profile_id = value.profile_id.trim().to_owned();
    value.profile_scope = value.profile_scope.trim().to_ascii_lowercase();
    value.profile_owner = value.profile_owner.trim().to_owned();
    if value.profile_id.is_empty() {
        value.profile_scope = "global".into();
        value.profile_owner.clear();
    } else {
        validate_profile_ref(
            &value.profile_id,
            &value.profile_scope,
            &value.profile_owner,
        )?;
    }
    Ok(())
}

pub(super) fn validate_private_environment(values: &BTreeMap<String, String>) -> io::Result<()> {
    if values.len() > MAX_ENV_KEYS {
        return invalid("too many private environment keys");
    }
    for (key, value) in values {
        validate_env_key(key)?;
        validate_environment_value(key, value)?;
        if matches!(
            key.as_str(),
            "CLAUDE_CONFIG_DIR"
                | "CODEX_HOME"
                | "GROK_HOME"
                | "HOME"
                | "USERPROFILE"
                | "XDG_DATA_HOME"
                | "XDG_CONFIG_HOME"
                | "OPENCODE_CONFIG"
                | "OPENCODE_CONFIG_DIR"
                | "KILO_CONFIG"
                | "KILO_CONFIG_DIR"
                | "KILO_DB"
        ) {
            explicit_path(value, key)?;
        }
    }
    Ok(())
}

pub(super) fn validate_env_key(value: &str) -> io::Result<()> {
    let mut chars = value.chars();
    let valid_start = chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    if !valid_start || !chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        return invalid(format!("invalid environment key: {value}"));
    }
    if value.to_ascii_uppercase().starts_with("CCCC_") {
        return invalid(format!("CCCC owns environment key: {value}"));
    }
    Ok(())
}

pub(super) fn normalized_environment_value(key: &str, value: String) -> io::Result<String> {
    if matches!(
        key,
        "CLAUDE_CONFIG_DIR"
            | "CODEX_HOME"
            | "GROK_HOME"
            | "HOME"
            | "USERPROFILE"
            | "XDG_DATA_HOME"
            | "XDG_CONFIG_HOME"
            | "OPENCODE_CONFIG"
            | "OPENCODE_CONFIG_DIR"
            | "KILO_CONFIG"
            | "KILO_CONFIG_DIR"
            | "KILO_DB"
    ) {
        return explicit_path(&value, key).map(|path| path.to_string_lossy().into_owned());
    }
    Ok(value)
}

pub(super) fn validate_environment_value(key: &str, value: &str) -> io::Result<()> {
    if value.chars().count() > MAX_ENV_VALUE_CHARS || value.contains('\0') {
        return invalid(format!("environment value is invalid or too large: {key}"));
    }
    Ok(())
}

fn validate_profile_ref(profile_id: &str, scope: &str, owner_id: &str) -> io::Result<()> {
    if profile_id.len() > 64
        || !profile_id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return invalid("invalid Runtime Profile id");
    }
    if !matches!(scope, "global" | "user") {
        return invalid("invalid Runtime Profile scope");
    }
    if scope == "global" && !owner_id.is_empty() {
        return invalid("global Runtime Profile owner must be empty");
    }
    if scope == "user" && owner_id.is_empty() {
        return invalid("user Runtime Profile requires an owner");
    }
    Ok(())
}

fn explicit_path(value: &str, name: &str) -> io::Result<PathBuf> {
    let value = value.trim();
    if value.is_empty()
        || (!Path::new(value).is_absolute()
            && value != "~"
            && !value.starts_with("~/")
            && !value.starts_with("~\\"))
    {
        return invalid(format!("{name} must be an absolute path or start with ~"));
    }
    crate::path_input::expand_user_path(value)
}

fn invalid<T>(message: impl Into<String>) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}
