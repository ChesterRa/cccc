use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io;
use std::time::SystemTime;

use crate::HomeLayout;
use crate::fs::{read_json, with_exclusive_lock, write_secret_json};

const NOTEBOOKLM_ENV: &str = "CCCC_NOTEBOOKLM_AUTH_JSON";
const NOTEBOOKLM_KEY: &str = "NOTEBOOKLM_AUTH_JSON";

pub fn status(home: &HomeLayout, provider: &str) -> io::Result<Value> {
    validate_provider(provider)?;
    let env_configured = std::env::var(NOTEBOOKLM_ENV).is_ok_and(|value| !value.trim().is_empty());
    let stored = load(home, provider)?;
    let store_configured = stored
        .get(NOTEBOOKLM_KEY)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let stored_value = stored
        .get(NOTEBOOKLM_KEY)
        .and_then(Value::as_str)
        .unwrap_or_default();
    let updated_at = path(home, provider)
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(system_time_iso);
    Ok(json!({
        "provider":provider,
        "key":NOTEBOOKLM_KEY,
        "configured":env_configured || store_configured,
        "source":if env_configured{"env"}else if store_configured{"store"}else{"none"},
        "env_configured":env_configured,
        "store_configured":store_configured,
        "updated_at":updated_at,
        "masked_value":if env_configured {
            Value::String("EN******ON".into())
        } else if store_configured {
            Value::String(mask_secret(stored_value))
        } else {
            Value::Null
        },
    }))
}

pub fn update(home: &HomeLayout, provider: &str, auth_json: &str) -> io::Result<Value> {
    validate_provider(provider)?;
    let parsed: Value = serde_json::from_str(auth_json).map_err(io::Error::other)?;
    if !parsed.is_object() {
        return Err(io::Error::other("auth_json must be a JSON object"));
    }
    mutate(home, provider, |doc| {
        doc.insert(
            NOTEBOOKLM_KEY.into(),
            Value::String(serde_json::to_string(&parsed).map_err(io::Error::other)?),
        );
        Ok(())
    })?;
    status(home, provider)
}

pub fn clear(home: &HomeLayout, provider: &str) -> io::Result<Value> {
    validate_provider(provider)?;
    mutate(home, provider, |doc| {
        doc.remove(NOTEBOOKLM_KEY);
        Ok(())
    })?;
    status(home, provider)
}

pub fn resolve(home: &HomeLayout, provider: &str) -> io::Result<Option<String>> {
    validate_provider(provider)?;
    if let Ok(value) = std::env::var(NOTEBOOKLM_ENV)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    Ok(load(home, provider)?
        .get(NOTEBOOKLM_KEY)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty()))
}

fn load(home: &HomeLayout, provider: &str) -> io::Result<Map<String, Value>> {
    migrate_legacy(home, provider)?;
    let path = path(home, provider);
    if path.exists() {
        read_json(&path)
    } else {
        Ok(Map::new())
    }
}

fn mutate<T>(
    home: &HomeLayout,
    provider: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> io::Result<T> {
    let target = path(home, provider);
    with_exclusive_lock(&target.with_extension("json.lock"), || {
        let mut doc = load(home, provider)?;
        let result = change(&mut doc)?;
        if doc.is_empty() {
            let _ = std::fs::remove_file(&target);
        } else {
            write_secret_json(&target, &doc)?;
        }
        Ok(result)
    })
}

fn path(home: &HomeLayout, provider: &str) -> std::path::PathBuf {
    let digest = format!("{:x}", Sha256::digest(provider.as_bytes()));
    home.root()
        .join("state/secrets/space_providers")
        .join(format!("{provider}.{}.json", &digest[..16]))
}

fn migrate_legacy(home: &HomeLayout, provider: &str) -> io::Result<()> {
    let target = path(home, provider);
    let legacy = home.root().join("space-credentials.json");
    let marker = home
        .root()
        .join("state/secrets/space_providers/.rust-credentials-migrated-v1");
    if marker.exists() || !legacy.exists() {
        return Ok(());
    }
    let raw: Value = read_json(&legacy)?;
    if let Some(auth_json) = raw
        .get("providers")
        .and_then(|providers| providers.get(provider))
        .and_then(|item| item.get("auth_json"))
        .and_then(Value::as_str)
        && !target.exists()
    {
        write_secret_json(&target, &json!({NOTEBOOKLM_KEY:auth_json}))?;
    }
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(marker, b"migrated from space-credentials.json\n")
}

fn validate_provider(provider: &str) -> io::Result<()> {
    (provider == "notebooklm")
        .then_some(())
        .ok_or_else(|| io::Error::other("unsupported space provider"))
}

fn mask_secret(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 6 {
        "******".into()
    } else {
        format!(
            "{}{}******{}{}",
            chars[0],
            chars[1],
            chars[chars.len() - 2],
            chars[chars.len() - 1]
        )
    }
}

fn system_time_iso(value: SystemTime) -> Option<String> {
    let date: chrono::DateTime<chrono::Utc> = value.into();
    Some(date.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_python_compatible_masked_credentials() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let result = update(&home, "notebooklm", r#"{"cookie":"secret"}"#).expect("update");
        assert_eq!(result["masked_value"], "{\"******\"}");
        assert!(!result.to_string().contains("secret"));
        assert!(
            std::fs::read_to_string(path(&home, "notebooklm"))
                .expect("stored")
                .contains(NOTEBOOKLM_KEY)
        );
        assert_eq!(
            clear(&home, "notebooklm").expect("clear")["configured"],
            false
        );
    }
}
