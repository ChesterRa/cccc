use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::fs::{read_json, with_exclusive_lock, write_secret_json};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::dispatch::{OpError, OpResult, object, required_arg};

pub fn keys(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let keys: Vec<_> = load(home, &group_id, &actor_id)?.into_keys().collect();
    object(json!({"actor_id": actor_id, "keys": keys}))
}

pub fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let mut values = load(home, &group_id, &actor_id)?;
    if request
        .args
        .get("clear")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        values.clear();
    }
    if let Some(set) = request.args.get("set").and_then(Value::as_object) {
        for (key, value) in set {
            validate_env_key(key)?;
            let secret = python_string(value)?;
            if secret.chars().count() > 200_000 {
                return Err(OpError::new("invalid_args", "env value too large"));
            }
            values.insert(key.clone(), secret);
        }
    }
    if let Some(unset) = request.args.get("unset").and_then(Value::as_array) {
        for key in unset.iter().filter_map(Value::as_str) {
            validate_env_key(key)?;
            values.remove(key);
        }
    }
    if values.len() > 256 {
        return Err(OpError::new("invalid_args", "too many env_private keys"));
    }
    let keys: Vec<_> = values.keys().cloned().collect();
    save(home, &group_id, &actor_id, &values)?;
    object(json!({"actor_id": actor_id, "keys": keys, "updated": true}))
}

fn load(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<BTreeMap<String, String>, OpError> {
    migrate_legacy(home, group_id)?;
    let path = path(home, group_id, actor_id)?;
    if path.exists() {
        read_json(&path).map_err(OpError::io)
    } else {
        Ok(BTreeMap::new())
    }
}

pub fn values(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<BTreeMap<String, String>, OpError> {
    load(home, group_id, actor_id)
}

pub fn replace(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    values: BTreeMap<String, String>,
) -> Result<(), OpError> {
    save(home, group_id, actor_id, &values)
}

pub fn remove(home: &HomeLayout, group_id: &str, actor_id: &str) -> Result<(), OpError> {
    replace(home, group_id, actor_id, BTreeMap::new())
}

pub fn copy_group(
    home: &HomeLayout,
    source_group_id: &str,
    target_group_id: &str,
) -> Result<(), OpError> {
    migrate_legacy(home, source_group_id)?;
    let source = home
        .root()
        .join("state/secrets/actors")
        .join(source_group_id);
    if !source.is_dir() {
        return Ok(());
    }
    let target = home
        .root()
        .join("state/secrets/actors")
        .join(target_group_id);
    std::fs::create_dir_all(&target).map_err(OpError::io)?;
    for entry in std::fs::read_dir(source).map_err(OpError::io)? {
        let entry = entry.map_err(OpError::io)?;
        if entry.file_type().map_err(OpError::io)?.is_file()
            && entry.file_name().to_string_lossy().ends_with(".json")
        {
            std::fs::copy(entry.path(), target.join(entry.file_name())).map_err(OpError::io)?;
        }
    }
    Ok(())
}

pub fn remove_group(home: &HomeLayout, group_id: &str) -> Result<(), OpError> {
    let path = home.root().join("state/secrets/actors").join(group_id);
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(OpError::io(error)),
    }
}

fn save(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    values: &BTreeMap<String, String>,
) -> Result<(), OpError> {
    let path = path(home, group_id, actor_id)?;
    if values.is_empty() {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(OpError::io(error)),
        }
    } else {
        write_secret_json(&path, values).map_err(OpError::io)
    }
}

fn path(home: &HomeLayout, group_id: &str, actor_id: &str) -> Result<PathBuf, OpError> {
    validate_component(group_id, "group_id")?;
    if actor_id.trim().is_empty() {
        return Err(OpError::new("invalid_args", "actor_id is required"));
    }
    Ok(home
        .root()
        .join("state/secrets/actors")
        .join(group_id)
        .join(actor_filename(actor_id)))
}

fn actor_filename(actor_id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(actor_id.as_bytes()));
    let slug = actor_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .chars()
        .take(24)
        .collect::<String>();
    format!(
        "{}.{}.json",
        if slug.is_empty() { "actor" } else { &slug },
        &digest[..16]
    )
}

fn validate_component(value: &str, name: &str) -> Result<(), OpError> {
    if value.trim().is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
    {
        Err(OpError::new("invalid_args", format!("invalid {name}")))
    } else {
        Ok(())
    }
}

fn validate_env_key(value: &str) -> Result<(), OpError> {
    let mut chars = value.chars();
    let valid_start = chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    if valid_start && chars.all(|character| character == '_' || character.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(OpError::new(
            "invalid_args",
            format!("invalid env key: {value}"),
        ))
    }
}

fn python_string(value: &Value) -> Result<String, OpError> {
    match value {
        Value::Null => Err(OpError::new("invalid_args", "missing env value")),
        Value::String(value) => Ok(value.clone()),
        Value::Bool(value) => Ok(if *value { "True" } else { "False" }.into()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Array(_) | Value::Object(_) => Ok(python_repr(value)),
    }
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(value) => if *value { "True" } else { "False" }.into(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(key, value)| format!(
                    "'{}': {}",
                    key.replace('\\', "\\\\").replace('\'', "\\'"),
                    python_repr(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn migrate_legacy(home: &HomeLayout, group_id: &str) -> Result<(), OpError> {
    validate_component(group_id, "group_id")?;
    let legacy = home
        .root()
        .join("groups")
        .join(group_id)
        .join("state/actor-secrets.json");
    let marker = home
        .root()
        .join("state/secrets/actors")
        .join(group_id)
        .join(".rust-actor-secrets-migrated-v1");
    if marker.exists() || !legacy.exists() {
        return Ok(());
    }
    let lock = home
        .root()
        .join("state/secrets/actors")
        .join(group_id)
        .join(".migration.lock");
    with_exclusive_lock(&lock, || {
        if marker.exists() {
            return Ok(());
        }
        let legacy: Value = read_json(&legacy)?;
        if let Some(actors) = legacy.get("actors").and_then(Value::as_object) {
            for (actor_id, raw) in actors {
                let target = path(home, group_id, actor_id)
                    .map_err(|error| std::io::Error::other(error.message))?;
                if !target.exists() {
                    write_secret_json(&target, raw)?;
                }
            }
        }
        if let Some(parent) = marker.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(marker, b"migrated from state/actor-secrets.json\n")
    })
    .map_err(OpError::io)
}
