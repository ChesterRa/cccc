use crate::HomeLayout;
use crate::fs::{read_json, with_exclusive_lock, write_secret_json};
use crate::profiles::ProfileStore;
use cccc_contracts::{ActorRuntime, CodexVoiceAnalystSettings};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

mod validation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedAgentRuntime {
    pub runtime: ActorRuntime,
    pub command: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

impl ResolvedAgentRuntime {
    #[must_use]
    pub fn fingerprint(&self) -> [u8; 32] {
        let encoded = serde_json::to_vec(self).expect("resolved runtime serializes");
        Sha256::digest(encoded).into()
    }

    #[must_use]
    pub fn identity_fingerprint(&self) -> String {
        identity_fingerprint(&self.environment)
    }
}

pub fn load(home: &HomeLayout) -> io::Result<CodexVoiceAnalystSettings> {
    Ok(crate::settings::load(home)?.codex_voice.analyst)
}

pub fn save(home: &HomeLayout, value: &CodexVoiceAnalystSettings) -> io::Result<()> {
    let value = normalize(value.clone())?;
    crate::settings::update(home, |settings| {
        settings.codex_voice.analyst = value;
        Ok(())
    })
}

pub fn normalize(mut value: CodexVoiceAnalystSettings) -> io::Result<CodexVoiceAnalystSettings> {
    validation::normalize(&mut value)?;
    Ok(value)
}

pub fn resolve(
    home: &HomeLayout,
    settings: &CodexVoiceAnalystSettings,
    custom_environment: &BTreeMap<String, String>,
) -> io::Result<ResolvedAgentRuntime> {
    let settings = normalize(settings.clone())?;
    let resolved = if settings.uses_profile() {
        let profile = ProfileStore::new(home.clone())?
            .runtime_ref(
                &settings.profile_id,
                &settings.profile_scope,
                &settings.profile_owner,
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Runtime Profile not found: {}", settings.profile_id),
                )
            })?;
        ResolvedAgentRuntime {
            runtime: profile.runtime,
            command: profile.command,
            environment: profile.environment,
        }
    } else {
        ResolvedAgentRuntime {
            runtime: settings.runtime,
            command: settings.command,
            environment: custom_environment.clone(),
        }
    };
    if resolved.runtime != ActorRuntime::Codex {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "Voice Analyst does not yet support the {:?} runtime",
                resolved.runtime
            ),
        ));
    }
    validation::validate_private_environment(&resolved.environment)?;
    Ok(resolved)
}

pub fn private_environment(home: &HomeLayout) -> io::Result<BTreeMap<String, String>> {
    let path = secret_path(home);
    match read_json(&path) {
        Ok(values) => Ok(values),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error),
    }
}

pub fn replace_private_environment(
    home: &HomeLayout,
    values: &BTreeMap<String, String>,
) -> io::Result<()> {
    validation::validate_private_environment(values)?;
    let path = secret_path(home);
    let lock = lock_path(&path);
    with_exclusive_lock(&lock, || {
        if values.is_empty() {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            };
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_secret_json(&path, values)
    })
}

pub fn patched_private_environment(
    current: &BTreeMap<String, String>,
    set: BTreeMap<String, String>,
    unset: &[String],
) -> io::Result<BTreeMap<String, String>> {
    let mut next = current.clone();
    for key in unset {
        validation::validate_env_key(key)?;
        next.remove(key);
    }
    for (key, value) in set {
        validation::validate_env_key(&key)?;
        let value = validation::normalized_environment_value(&key, value)?;
        validation::validate_environment_value(&key, &value)?;
        next.insert(key, value);
    }
    validation::validate_private_environment(&next)?;
    Ok(next)
}

pub fn validate_private_environment(values: &BTreeMap<String, String>) -> io::Result<()> {
    validation::validate_private_environment(values)
}

pub fn identity_environment_changed(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> bool {
    identity_fingerprint(before) != identity_fingerprint(after)
}

fn identity_fingerprint(environment: &BTreeMap<String, String>) -> String {
    let effective = ["CODEX_HOME", "HOME", "USERPROFILE"].map(|key| {
        let value = environment
            .get(key)
            .cloned()
            .or_else(|| std::env::var_os(key).map(|value| value.to_string_lossy().into_owned()));
        (key, value)
    });
    let encoded = serde_json::to_vec(&effective).expect("Codex identity environment serializes");
    format!("{:x}", Sha256::digest(encoded))
}

pub fn workdir(home: &HomeLayout) -> io::Result<PathBuf> {
    let path = home.root().join("state/codex_voice/analyst-workdir");
    std::fs::create_dir_all(&path)?;
    path.canonicalize()
}

fn secret_path(home: &HomeLayout) -> PathBuf {
    home.root().join("state/secrets/codex_voice_analyst.json")
}

fn lock_path(path: &std::path::Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".lock");
    PathBuf::from(value)
}

#[cfg(test)]
mod tests;
