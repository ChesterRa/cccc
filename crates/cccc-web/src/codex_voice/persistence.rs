use super::*;
use anyhow::{Context, Result, bail};
use cccc_core::codex_voice_settings::ResolvedAgentRuntime;
use serde::{Deserialize, Serialize};

const MAX_CLIENT_SESSION_ID_BYTES: usize = 128;
const ANALYST_STATE_FILE: &str = "codex_voice_analyst.json";
const ANALYST_WORKSPACE_VERSION: u32 = 1;

pub(super) async fn launch_analyst(home: &HomeLayout) -> Result<AnalystRuntime> {
    let workdir = cccc_core::codex_voice_settings::workdir(home)
        .context("prepare neutral Voice Analyst working directory")?;
    let settings = cccc_core::codex_voice_settings::load(home)
        .context("load Voice Analyst launch settings")?;
    let custom_environment = cccc_core::codex_voice_settings::private_environment(home)
        .context("load Voice Analyst private environment")?;
    let runtime = cccc_core::codex_voice_settings::resolve(home, &settings, &custom_environment)
        .context("resolve Voice Analyst runtime configuration")?;
    let identity_fingerprint = runtime
        .identity_fingerprint_at(&workdir)
        .context("fingerprint Voice Analyst launch inputs")?;
    let (resume_thread_id, warning) = resumable_thread(home, &workdir, &identity_fingerprint)?;
    if let Some(thread_id) = resume_thread_id {
        match launch_exact(
            home,
            workdir.clone(),
            runtime.clone(),
            Some(thread_id),
            "ready",
            String::new(),
        )
        .await
        {
            Ok(analyst) => return Ok(analyst),
            Err(error) => {
                tracing::warn!(%error, "Voice Analyst resume failed; starting fresh");
                return launch_exact(
                    home,
                    workdir,
                    runtime,
                    None,
                    "waiting",
                    "analyst_resume_replaced".into(),
                )
                .await;
            }
        }
    }
    launch_exact(home, workdir, runtime, None, "waiting", warning).await
}

pub(super) async fn launch_fresh_analyst(
    home: &HomeLayout,
    warning: String,
) -> Result<AnalystRuntime> {
    let settings = cccc_core::codex_voice_settings::load(home)?;
    let custom_environment = cccc_core::codex_voice_settings::private_environment(home)?;
    let runtime = cccc_core::codex_voice_settings::resolve(home, &settings, &custom_environment)?;
    launch_exact(
        home,
        cccc_core::codex_voice_settings::workdir(home)?,
        runtime,
        None,
        "waiting",
        warning,
    )
    .await
}

pub(super) async fn launch_exact(
    home: &HomeLayout,
    workdir: PathBuf,
    runtime: ResolvedAgentRuntime,
    resume_thread_id: Option<String>,
    phase: &str,
    warning: String,
) -> Result<AnalystRuntime> {
    let mut config = LaunchConfig::new(&workdir);
    config.runtime = runtime.runtime;
    config.command = runtime.command.clone();
    config.environment = runtime.environment.clone();
    config.resume_thread_id = resume_thread_id;
    let analyst = CodexVoiceAnalyst::launch(home, config).await?;
    Ok(AnalystRuntime::new(
        workdir, analyst, runtime, phase, warning,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedAnalyst {
    #[serde(default)]
    pub(super) workspace_version: u32,
    #[serde(default)]
    pub(super) group_id: String,
    pub(super) root: String,
    pub(super) thread_id: String,
    #[serde(default)]
    pub(super) identity_fingerprint: String,
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
        Err(error) => Err(error)
            .with_context(|| format!("read Voice Analyst resume state at {}", path.display())),
    }
}

fn latest_legacy_persisted_analyst(home: &HomeLayout) -> Option<PersistedAnalyst> {
    let store = cccc_core::GroupStore::new(home.clone()).ok()?;
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

pub(super) fn resumable_thread(
    home: &HomeLayout,
    workdir: &Path,
    identity_fingerprint: &str,
) -> Result<(Option<String>, String)> {
    let Some(persisted) = load_persisted_analyst(home)? else {
        return Ok((None, String::new()));
    };
    if !persisted.materialized || persisted.thread_id.trim().is_empty() {
        return Ok((None, String::new()));
    }
    if persisted.workspace_version != ANALYST_WORKSPACE_VERSION {
        return Ok((None, "analyst_workspace_migrated".into()));
    }
    let Ok(persisted_root) = PathBuf::from(&persisted.root).canonicalize() else {
        return Ok((None, "analyst_resume_replaced".into()));
    };
    if persisted_root != workdir {
        return Ok((None, "analyst_resume_replaced".into()));
    }
    if persisted.identity_fingerprint != identity_fingerprint {
        return Ok((None, "analyst_configuration_started_new_session".into()));
    }
    Ok((Some(persisted.thread_id), String::new()))
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
            workspace_version: ANALYST_WORKSPACE_VERSION,
            group_id: String::new(),
            root: analyst.workdir.to_string_lossy().into_owned(),
            thread_id: analyst.analyst.thread_id().to_owned(),
            identity_fingerprint: analyst
                .launch_runtime()
                .identity_fingerprint_at(&analyst.workdir)
                .context("fingerprint Voice Analyst launch inputs")?,
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

#[cfg(test)]
pub(super) const TEST_ANALYST_WORKSPACE_VERSION: u32 = ANALYST_WORKSPACE_VERSION;
