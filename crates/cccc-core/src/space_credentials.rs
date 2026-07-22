use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;

use crate::HomeLayout;
use crate::fs::{read_json, with_exclusive_lock, write_secret_json};

const NOTEBOOKLM_ENV: &str = "CCCC_NOTEBOOKLM_AUTH_JSON";

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialDoc {
    #[serde(default)]
    providers: BTreeMap<String, StoredCredential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCredential {
    auth_json: String,
    updated_at: String,
}

pub fn status(home: &HomeLayout, provider: &str) -> io::Result<Value> {
    validate_provider(provider)?;
    let env_configured = provider == "notebooklm"
        && std::env::var(NOTEBOOKLM_ENV).is_ok_and(|value| !value.trim().is_empty());
    let stored = load(home)?.providers.get(provider).cloned();
    let store_configured = stored
        .as_ref()
        .is_some_and(|value| !value.auth_json.trim().is_empty());
    let source = if env_configured {
        "env"
    } else if store_configured {
        "store"
    } else {
        "none"
    };
    Ok(json!({
        "provider": provider,
        "key": format!("{provider}_auth_json"),
        "configured": env_configured || store_configured,
        "source": source,
        "env_configured": env_configured,
        "store_configured": store_configured,
        "updated_at": stored.as_ref().map(|value| value.updated_at.as_str()),
        "masked_value": if env_configured || store_configured { Value::String("********".into()) } else { Value::Null },
    }))
}

pub fn update(home: &HomeLayout, provider: &str, auth_json: &str) -> io::Result<Value> {
    validate_provider(provider)?;
    let parsed: Value = serde_json::from_str(auth_json).map_err(io::Error::other)?;
    if !parsed.is_object() {
        return Err(io::Error::other("auth_json must be a JSON object"));
    }
    mutate(home, |doc| {
        doc.providers.insert(
            provider.to_owned(),
            StoredCredential {
                auth_json: serde_json::to_string(&parsed).map_err(io::Error::other)?,
                updated_at: utc_now(),
            },
        );
        Ok(())
    })?;
    status(home, provider)
}

pub fn clear(home: &HomeLayout, provider: &str) -> io::Result<Value> {
    validate_provider(provider)?;
    mutate(home, |doc| {
        doc.providers.remove(provider);
        Ok(())
    })?;
    status(home, provider)
}

/// Resolves the credential without exposing it through an API response.
pub fn resolve(home: &HomeLayout, provider: &str) -> io::Result<Option<String>> {
    validate_provider(provider)?;
    if provider == "notebooklm"
        && let Ok(value) = std::env::var(NOTEBOOKLM_ENV)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    Ok(load(home)?
        .providers
        .get(provider)
        .map(|credential| credential.auth_json.clone())
        .filter(|value| !value.trim().is_empty()))
}

fn load(home: &HomeLayout) -> io::Result<CredentialDoc> {
    let path = path(home);
    if path.exists() {
        read_json(&path)
    } else {
        Ok(CredentialDoc::default())
    }
}

fn mutate<T>(
    home: &HomeLayout,
    change: impl FnOnce(&mut CredentialDoc) -> io::Result<T>,
) -> io::Result<T> {
    with_exclusive_lock(&home.root().join("space-credentials.json.lock"), || {
        let mut doc = load(home)?;
        let result = change(&mut doc)?;
        write_secret_json(&path(home), &doc)?;
        Ok(result)
    })
}

fn path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join("space-credentials.json")
}

fn validate_provider(provider: &str) -> io::Result<()> {
    matches!(provider, "notebooklm")
        .then_some(())
        .ok_or_else(|| io::Error::other("unsupported space provider"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_masked_credentials_without_exposing_values() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let result = update(&home, "notebooklm", r#"{"cookie":"secret"}"#).expect("update");
        assert_eq!(result["masked_value"], "********");
        assert!(!result.to_string().contains("secret"));
        assert!(
            std::fs::read_to_string(path(&home))
                .expect("stored")
                .contains("secret")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path(&home))
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert_eq!(
            clear(&home, "notebooklm").expect("clear")["configured"],
            false
        );
    }
}
