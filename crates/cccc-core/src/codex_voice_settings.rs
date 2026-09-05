use crate::HomeLayout;
use crate::fs::{read_json, with_exclusive_lock, write_secret_json};
use crate::profiles::ProfileStore;
use cccc_contracts::{ActorRuntime, CodexVoiceAnalystSettings};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};

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
        if self.runtime == ActorRuntime::Claude {
            // Claude Agent View persists launch flags and provider environment
            // in the background job. Resuming the exact session cannot apply
            // changed launch configuration, so any resolved setting change
            // intentionally selects a fresh session.
            let encoded = serde_json::to_vec(self).expect("resolved Claude runtime serializes");
            return format!("{:x}", Sha256::digest(encoded));
        }
        identity_fingerprint(self.runtime, &self.environment)
    }

    pub fn identity_fingerprint_at(&self, cwd: &Path) -> io::Result<String> {
        if self.runtime != ActorRuntime::Claude {
            return Ok(self.identity_fingerprint());
        }
        let files = claude_launch_file_fingerprints(&self.command, cwd)?;
        let encoded =
            serde_json::to_vec(&(self, files)).expect("resolved Claude runtime serializes");
        Ok(format!("{:x}", Sha256::digest(encoded)))
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
    if !matches!(
        resolved.runtime,
        ActorRuntime::Claude
            | ActorRuntime::Codex
            | ActorRuntime::Grok
            | ActorRuntime::Opencode
            | ActorRuntime::Kilo
    ) {
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

pub fn runtime_identity_changed(
    before: &ResolvedAgentRuntime,
    after: &ResolvedAgentRuntime,
) -> bool {
    before.identity_fingerprint() != after.identity_fingerprint()
}

fn identity_fingerprint(runtime: ActorRuntime, environment: &BTreeMap<String, String>) -> String {
    if runtime == ActorRuntime::Codex {
        // A Codex thread is resumable only under the same effective credential
        // and session-store roots. Launch arguments are fenced separately by
        // the runtime-session command fingerprint.
        let effective = ["CODEX_HOME", "HOME", "USERPROFILE"].map(|key| {
            let value = environment.get(key).cloned().or_else(|| {
                std::env::var_os(key).map(|value| value.to_string_lossy().into_owned())
            });
            (key, value)
        });
        let encoded =
            serde_json::to_vec(&effective).expect("Codex identity environment serializes");
        return format!("{:x}", Sha256::digest(encoded));
    }
    let provider_keys: &[&str] = match runtime {
        ActorRuntime::Claude => unreachable!("Claude fingerprints the full resolved launch"),
        ActorRuntime::Grok => &["GROK_HOME", "HOME", "USERPROFILE"],
        ActorRuntime::Kilo => &[
            "HOME",
            "USERPROFILE",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "KILO_CONFIG",
            "KILO_CONFIG_DIR",
            "KILO_DB",
        ],
        ActorRuntime::Opencode => &[
            "HOME",
            "USERPROFILE",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_DB",
        ],
        _ => &["HOME", "USERPROFILE"],
    };
    let effective = provider_keys
        .iter()
        .map(|key| {
            let value = environment.get(*key).cloned().or_else(|| {
                std::env::var_os(key).map(|value| value.to_string_lossy().into_owned())
            });
            (*key, value)
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_vec(&(runtime, effective))
        .expect("managed Runtime identity environment serializes");
    format!("{:x}", Sha256::digest(encoded))
}

fn claude_launch_file_fingerprints(
    command: &[String],
    cwd: &Path,
) -> io::Result<Vec<(String, String, String)>> {
    const SINGLE_FILE_FLAGS: &[&str] = &[
        "--settings",
        "--system-prompt-file",
        "--append-system-prompt-file",
    ];
    let mut files = Vec::new();
    let mut index = 1usize;
    while index < command.len() {
        let argument = &command[index];
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(flag, value)| {
                (flag, Some(value))
            });
        if !SINGLE_FILE_FLAGS.contains(&flag) && flag != "--mcp-config" {
            index += 1;
            continue;
        }
        let (values, consumed) = if let Some(value) = inline {
            (vec![value], 1)
        } else if flag == "--mcp-config" {
            let values = command[index + 1..]
                .iter()
                .take_while(|value| !value.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>();
            let consumed = values.len().saturating_add(1);
            (values, consumed)
        } else {
            (
                command
                    .get(index + 1)
                    .map(String::as_str)
                    .into_iter()
                    .collect(),
                2,
            )
        };
        for value in values
            .into_iter()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if matches!(flag, "--settings" | "--mcp-config")
                && matches!(value.as_bytes().first().copied(), Some(b'{') | Some(b'['))
            {
                continue;
            }
            files.push(claude_launch_file_fingerprint(flag, value, cwd)?);
        }
        index += consumed;
    }
    Ok(files)
}

fn claude_launch_file_fingerprint(
    flag: &str,
    value: &str,
    cwd: &Path,
) -> io::Result<(String, String, String)> {
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let canonical = path.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to resolve Claude launch input {}: {error}",
                path.display()
            ),
        )
    })?;
    let metadata = std::fs::metadata(&canonical)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Claude launch input is not a file: {}", canonical.display()),
        ));
    }
    let mut file = File::open(&canonical)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok((
        flag.to_owned(),
        canonical.to_string_lossy().into_owned(),
        format!("{:x}", digest.finalize()),
    ))
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
