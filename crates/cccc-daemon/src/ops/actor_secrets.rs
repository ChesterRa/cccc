use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::fs::{read_json, with_exclusive_lock, write_secret_json};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use crate::dispatch::{OpError, OpResult, object, required_arg, store};

pub fn keys(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = public_target(home, request, "access private env metadata")?;
    let keys: Vec<_> = load(home, &group_id, &actor_id)?.into_keys().collect();
    object(json!({"group_id": group_id, "actor_id": actor_id, "keys": keys}))
}

pub fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = public_target(home, request, "update private env")?;
    let clear = request
        .args
        .get("clear")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut set_values = Vec::new();
    if let Some(raw) = request.args.get("set") {
        let set = raw
            .as_object()
            .ok_or_else(|| OpError::new("invalid_args", "set must be an object"))?;
        for (key, value) in set {
            validate_env_key(key)?;
            let secret = python_string(value)?;
            if secret.chars().count() > 200_000 {
                return Err(OpError::new("invalid_args", "env value too large"));
            }
            set_values.push((key.clone(), secret));
        }
    }
    let mut unset_keys = Vec::new();
    if let Some(raw) = request.args.get("unset") {
        let unset = raw
            .as_array()
            .ok_or_else(|| OpError::new("invalid_args", "unset must be an array"))?;
        for raw_key in unset {
            let key = raw_key
                .as_str()
                .ok_or_else(|| OpError::new("invalid_args", "unset keys must be strings"))?;
            validate_env_key(key)?;
            unset_keys.push(key.to_owned());
        }
    }
    let values = mutate(home, &group_id, &actor_id, |values| {
        if clear {
            values.clear();
        }
        for key in unset_keys {
            values.remove(&key);
        }
        for (key, value) in set_values {
            values.insert(key, value);
        }
        if values.len() > 256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "too many env_private keys",
            ));
        }
        Ok(())
    })?;
    let keys: Vec<_> = values.keys().cloned().collect();
    object(json!({"group_id": group_id, "actor_id": actor_id, "keys": keys, "updated": true}))
}

fn public_target(
    home: &HomeLayout,
    request: &DaemonRequest,
    action: &str,
) -> Result<(String, String), OpError> {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let by = match request.args.get("by") {
        None => "user",
        Some(Value::String(value)) => value.trim(),
        Some(_) => {
            return Err(OpError::new(
                "permission_denied",
                format!("only user can {action}"),
            ));
        }
    };
    if !by.is_empty() && by != "user" {
        return Err(OpError::new(
            "permission_denied",
            format!("only user can {action}"),
        ));
    }
    let group = match store(home)?.load(&group_id) {
        Ok(group) => group,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(OpError::new(
                "group_not_found",
                format!("group not found: {group_id}"),
            ));
        }
        Err(error) => return Err(OpError::io(error)),
    };
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", format!("actor not found: {actor_id}")))?;
    if !actor.profile_id.trim().is_empty() {
        return Err(OpError::new(
            "actor_profile_linked_readonly",
            "linked actor private env is profile-controlled (convert to custom first)",
        ));
    }
    Ok((group_id, actor_id))
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
            let target_path = target.join(entry.file_name());
            let lock = lock_path(&target_path);
            with_exclusive_lock(&lock, || {
                let value: Value = read_json(&entry.path())?;
                write_secret_json(&target_path, &value)
            })
            .map_err(OpError::io)?;
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
    migrate_legacy(home, group_id)?;
    let path = path(home, group_id, actor_id)?;
    let lock = lock_path(&path);
    with_exclusive_lock(&lock, || save_unlocked(&path, values)).map_err(OpError::io)
}

fn mutate(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    operation: impl FnOnce(&mut BTreeMap<String, String>) -> io::Result<()>,
) -> Result<BTreeMap<String, String>, OpError> {
    migrate_legacy(home, group_id)?;
    let path = path(home, group_id, actor_id)?;
    let lock = lock_path(&path);
    with_exclusive_lock(&lock, || {
        let mut values = if path.exists() {
            read_json(&path)?
        } else {
            BTreeMap::new()
        };
        operation(&mut values)?;
        save_unlocked(&path, &values)?;
        Ok(values)
    })
    .map_err(|error| {
        if error.kind() == io::ErrorKind::InvalidInput {
            OpError::new("invalid_args", error.to_string())
        } else {
            OpError::io(error)
        }
    })
}

fn save_unlocked(path: &std::path::Path, values: &BTreeMap<String, String>) -> io::Result<()> {
    if values.is_empty() {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    } else {
        write_secret_json(path, values)
    }
}

fn lock_path(path: &std::path::Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".lock");
    PathBuf::from(value)
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

pub(crate) fn validate_env_key(value: &str) -> Result<(), OpError> {
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

pub(crate) fn python_string(value: &Value) -> Result<String, OpError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn replacement_waits_for_the_shared_actor_secret_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let secret_path = path(&home, "g_lock", "peer").expect("secret path");
        let shared_lock = lock_path(&secret_path);
        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder = std::thread::spawn(move || {
            with_exclusive_lock(&shared_lock, || {
                held_tx.send(()).expect("held signal");
                release_rx.recv().expect("release signal");
                Ok(())
            })
            .expect("hold lock");
        });
        held_rx.recv().expect("lock held");

        let writer_home = home.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            replace(
                &writer_home,
                "g_lock",
                "peer",
                BTreeMap::from([("TOKEN".into(), "value".into())]),
            )
            .expect("replace");
            done_tx.send(()).expect("done signal");
        });
        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "writer bypassed the shared lock"
        );
        release_tx.send(()).expect("release lock");
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("writer completion");
        holder.join().expect("holder");
        writer.join().expect("writer");
        assert_eq!(
            values(&home, "g_lock", "peer").expect("values"),
            BTreeMap::from([("TOKEN".into(), "value".into())])
        );
    }
}
