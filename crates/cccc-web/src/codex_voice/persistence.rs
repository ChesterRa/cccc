use super::*;
use anyhow::{Context, Result, anyhow, bail};
use cccc_core::{GroupStore, group_scope};
use serde::{Deserialize, Serialize};

const MAX_CLIENT_SESSION_ID_BYTES: usize = 128;
const ANALYST_STATE_FILE: &str = "codex_voice_analyst.json";

pub(super) fn resolve_scope(
    home: &HomeLayout,
    group_id: &str,
) -> Result<(String, String, PathBuf)> {
    let store = GroupStore::new(home.clone()).context("open Group store")?;
    let group = store
        .load(group_id.trim())
        .with_context(|| format!("load Codex Voice Group {group_id}"))?;
    let scope = group_scope::resolve_attached_scope(&group, &group.active_scope_key)
        .ok_or_else(|| anyhow!("Codex Voice requires one active attached repository root"))?;
    let root = PathBuf::from(&scope.url)
        .canonicalize()
        .with_context(|| format!("resolve active repository root {}", scope.url))?;
    if !root.is_dir() {
        bail!("Codex Voice active repository root is not a directory");
    }
    Ok((group.group_id, group.title, root))
}

pub(super) async fn launch_analyst(
    home: &HomeLayout,
    group_id: &str,
    group_title: &str,
    root: &Path,
    fresh_warning: String,
) -> Result<AnalystRuntime> {
    let resume_thread_id = resumable_thread_id(home, group_id, root);
    if let Some(thread_id) = resume_thread_id {
        let mut config = LaunchConfig::new(group_id, root);
        config.resume_thread_id = Some(thread_id);
        match CodexVoiceAnalyst::launch(home, config).await {
            Ok(analyst) => {
                return Ok(AnalystRuntime::new(
                    group_id.to_owned(),
                    group_title.to_owned(),
                    root.to_owned(),
                    analyst,
                    "ready",
                    String::new(),
                ));
            }
            Err(error) => {
                tracing::warn!(%error, %group_id, "Voice Analyst resume failed; starting fresh");
                return launch_fresh_analyst(
                    home,
                    group_id,
                    group_title,
                    root,
                    "analyst_resume_replaced".into(),
                )
                .await;
            }
        }
    }
    launch_fresh_analyst(home, group_id, group_title, root, fresh_warning).await
}

pub(super) fn resolve_resumable_scope(
    home: &HomeLayout,
) -> Result<Option<(String, String, PathBuf)>> {
    let Some(persisted) = load_persisted_analyst(home)? else {
        return Ok(None);
    };
    if !persisted.materialized || persisted.thread_id.trim().is_empty() {
        return Ok(None);
    }
    let store =
        GroupStore::new(home.clone()).context("open Group store for Voice Analyst resume")?;
    let group = store
        .load(&persisted.group_id)
        .with_context(|| format!("load persisted Voice Analyst Group {}", persisted.group_id))?;
    let scope = group_scope::resolve_attached_scope(&group, &persisted.root)
        .context("persisted Voice Analyst repository is no longer attached")?;
    let root = PathBuf::from(&scope.url)
        .canonicalize()
        .context("resolve attached Voice Analyst repository")?;
    let persisted_root = PathBuf::from(&persisted.root)
        .canonicalize()
        .context("resolve persisted Voice Analyst repository")?;
    if root != persisted_root || !root.is_dir() {
        bail!("persisted Voice Analyst repository binding no longer matches");
    }
    Ok(Some((group.group_id, group.title, root)))
}

pub(super) async fn launch_fresh_analyst(
    home: &HomeLayout,
    group_id: &str,
    group_title: &str,
    root: &Path,
    warning: String,
) -> Result<AnalystRuntime> {
    let analyst = CodexVoiceAnalyst::launch(home, LaunchConfig::new(group_id, root)).await?;
    Ok(AnalystRuntime::new(
        group_id.to_owned(),
        group_title.to_owned(),
        root.to_owned(),
        analyst,
        "waiting",
        warning,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedAnalyst {
    pub(super) group_id: String,
    pub(super) root: String,
    pub(super) thread_id: String,
    pub(super) materialized: bool,
    pub(super) updated_at: String,
}

fn analyst_state_path(home: &HomeLayout) -> PathBuf {
    home.daemon_dir().join(ANALYST_STATE_FILE)
}

fn load_persisted_analyst(home: &HomeLayout) -> Result<Option<PersistedAnalyst>> {
    let path = analyst_state_path(home);
    match cccc_core::fs::read_json::<PersistedAnalyst>(&path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(latest_legacy_persisted_analyst(home))
        }
        Err(error) => {
            if let Some(legacy) = latest_legacy_persisted_analyst(home) {
                tracing::warn!(%error, path = %path.display(), "using legacy Voice Analyst resume state because the global state is invalid");
                return Ok(Some(legacy));
            }
            Err(error).with_context(|| {
                format!("read persisted Voice Analyst binding at {}", path.display())
            })
        }
    }
}

fn latest_legacy_persisted_analyst(home: &HomeLayout) -> Option<PersistedAnalyst> {
    // Early experimental builds stored one resume receipt per Group. Read the newest valid one
    // only as a migration source; the next successful launch writes the canonical global receipt.
    let store = GroupStore::new(home.clone()).ok()?;
    store
        .list()
        .ok()?
        .into_iter()
        .filter_map(|group| {
            let path = store
                .state_dir(&group.group_id)
                .ok()?
                .join(ANALYST_STATE_FILE);
            cccc_core::fs::read_json::<PersistedAnalyst>(&path).ok()
        })
        .filter(|persisted| persisted.materialized && !persisted.thread_id.trim().is_empty())
        .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
}

pub(super) fn resumable_thread_id(
    home: &HomeLayout,
    group_id: &str,
    root: &Path,
) -> Option<String> {
    let persisted = match load_persisted_analyst(home) {
        Ok(Some(persisted)) => persisted,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(%error, "ignored invalid Voice Analyst resume state");
            return None;
        }
    };
    (persisted.materialized
        && persisted.group_id == group_id
        && Path::new(&persisted.root) == root
        && !persisted.thread_id.trim().is_empty())
    .then_some(persisted.thread_id)
}

pub(super) fn persist_analyst(
    home: &HomeLayout,
    analyst: &AnalystRuntime,
    materialized: bool,
) -> Result<()> {
    let path = analyst_state_path(home);
    cccc_core::fs::write_json(
        &path,
        &PersistedAnalyst {
            group_id: analyst.group_id.clone(),
            root: analyst.root.to_string_lossy().into_owned(),
            thread_id: analyst.analyst.thread_id().to_owned(),
            materialized,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .with_context(|| format!("persist Voice Analyst binding at {}", path.display()))
}

pub(super) fn validate_client_session_id(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_CLIENT_SESSION_ID_BYTES {
        bail!("Codex Voice client session id is empty or oversized");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("Codex Voice client session id contains unsupported characters");
    }
    Ok(value.to_owned())
}

#[cfg(test)]
pub(super) const TEST_ANALYST_STATE_FILE: &str = ANALYST_STATE_FILE;
