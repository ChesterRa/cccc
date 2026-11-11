use super::{
    command_fingerprint, model_from_command, read, resume_enabled, string, valid_session_id,
    workspace_path, write, write_recorded,
};
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use cccc_core::codex_voice_settings::LaunchParts;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

const MANAGED_RECORD_VERSION: u64 = 2;
const MANAGED_TRANSPORT: &str = "claude_agent_view";
/// Consecutive failed resumes of one saved conversation after which automatic
/// recovery pauses even for failures that are individually retryable. A
/// successful resume resets the count; an explicit Start/Restart gets one more
/// attempt per pause.
const AUTOMATIC_RESUME_FAILURE_LIMIT: u64 = 10;

#[cfg(test)]
#[path = "claude_failure_tests.rs"]
mod failure_tests;

pub struct ResumeAttempt {
    pub session_id: String,
    pub attempt_id: String,
    /// A PATH-only launch change may resume a stopped session, but must not
    /// adopt a live worker that still has its old launch environment.
    pub cold_only: bool,
}

#[derive(Debug)]
pub struct ResumeBlocked(pub String);

impl std::fmt::Display for ResumeBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} Automatic resume is paused; retry with Start/Restart or choose New Session.",
            self.0
        )
    }
}

impl std::error::Error for ResumeBlocked {}

pub fn is_resume_blocked(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<ResumeBlocked>())
}

pub fn prepare_managed(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
    environment: &BTreeMap<String, String>,
) -> std::io::Result<Option<ResumeAttempt>> {
    if !resume_enabled() {
        return Ok(None);
    }
    let Ok(mut document) = read(home, group_id, actor_id) else {
        return Ok(None);
    };
    if document.get("v").and_then(Value::as_u64) != Some(MANAGED_RECORD_VERSION)
        || string(&document, "kind") != "runtime_session"
        || string(&document, "transport") != MANAGED_TRANSPORT
        || string(&document, "runtime") != "claude"
        || string(&document, "workspace_path") != workspace_path(cwd)
        || string(&document, "command_fingerprint") != command_fingerprint(base_command)
        || string(&document, "model") != model_from_command(base_command)
        || !string(&document, "provider_thread_id").is_empty()
    {
        return Ok(None);
    }
    let (identity, parts) = launch_identity(base_command, environment, cwd)?;
    let saved_identity = string(&document, "identity_fingerprint");
    let cold_only = saved_identity != identity;
    if cold_only
        && (saved_identity.is_empty()
            || !parts.is_some_and(|parts| {
                super::replaced::only_launch_path_changed(&Value::Object(document.clone()), &parts)
            }))
    {
        return Ok(None);
    }
    let session_id = string(&document, "provider_session_id");
    if !valid_session_id(&session_id) {
        return Ok(None);
    }
    if string(&document, "status") == "resume_failed" {
        return Err(io::Error::other(ResumeBlocked(string(
            &document,
            "last_resume_error",
        ))));
    }
    if string(&document, "status") != "usable"
        || document.get("resume_eligible").and_then(Value::as_bool) != Some(true)
    {
        return Ok(None);
    }
    let attempt_id = uuid::Uuid::new_v4().simple().to_string();
    document.insert("last_resume_attempt_at".into(), json!(utc_now()));
    document.insert("last_resume_attempt_id".into(), json!(attempt_id));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)?;
    Ok(Some(ResumeAttempt {
        session_id,
        attempt_id,
        cold_only,
    }))
}

/// Preserve the original conversation and fence late failures from newer attempts/successes.
///
/// Returns the recorded diagnostic when this failure was retryable on its own
/// but reached `AUTOMATIC_RESUME_FAILURE_LIMIT` and so paused automatic
/// recovery. The conversation stays resume-eligible: only the retry loop stops.
pub fn record_resume_failure(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    attempt: &ResumeAttempt,
    diagnostic: &str,
    blocked: bool,
) -> io::Result<Option<String>> {
    let mut document = read(home, group_id, actor_id)?;
    if string(&document, "provider_session_id") != attempt.session_id
        || string(&document, "last_resume_attempt_id") != attempt.attempt_id
    {
        return Ok(None);
    }
    let failures = document
        .get("failure_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(1);
    let limited = !blocked && failures >= AUTOMATIC_RESUME_FAILURE_LIMIT;
    let diagnostic = if limited {
        format!("{diagnostic} CCCC stopped after {failures} consecutive failed resumes.")
    } else {
        diagnostic.to_owned()
    };
    document.insert("last_resume_error".into(), json!(diagnostic));
    document.insert("failure_count".into(), json!(failures));
    if blocked || limited {
        document.insert("status".into(), json!("resume_failed"));
    }
    if blocked {
        document.insert("resume_eligible".into(), json!(false));
    }
    // A terminal attempt must not record the same failure twice.
    document.insert("last_resume_attempt_id".into(), json!(""));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)?;
    Ok(limited.then_some(diagnostic))
}

/// Called only by an authorized explicit Actor lifecycle action, never auto-wake or polling.
pub fn retry_failed_resume(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<bool> {
    let mut document = match read(home, group_id, actor_id) {
        Ok(document) => document,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if string(&document, "transport") != MANAGED_TRANSPORT
        || string(&document, "runtime") != "claude"
        || string(&document, "status") != "resume_failed"
    {
        return Ok(false);
    }
    document.insert("status".into(), json!("usable"));
    document.insert("resume_eligible".into(), json!(true));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub fn record_managed(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
    environment: &BTreeMap<String, String>,
    session_id: &str,
    resumed: bool,
) -> std::io::Result<()> {
    if !resume_enabled() {
        return Ok(());
    }
    if !valid_session_id(session_id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid Claude Agent View session id",
        ));
    }
    let now = utc_now();
    let (identity_fingerprint, launch_parts) = launch_identity(base_command, environment, cwd)?;
    let document = Map::from_iter([
        ("v".into(), json!(MANAGED_RECORD_VERSION)),
        ("kind".into(), json!("runtime_session")),
        ("transport".into(), json!(MANAGED_TRANSPORT)),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("claude")),
        ("workspace_path".into(), json!(workspace_path(cwd))),
        (
            "command_fingerprint".into(),
            json!(command_fingerprint(base_command)),
        ),
        ("model".into(), json!(model_from_command(base_command))),
        ("identity_fingerprint".into(), json!(identity_fingerprint)),
        // Beside the fingerprint, never part of what a resume is matched by.
        ("launch_parts".into(), json!(launch_parts)),
        ("provider_session_id".into(), json!(session_id)),
        ("provider_thread_id".into(), json!("")),
        ("resume_command_hint".into(), json!("")),
        (
            "captured_from".into(),
            json!(if resumed {
                "claude_agent_view_resume"
            } else {
                "claude_agent_view_start"
            }),
        ),
        ("status".into(), json!("usable")),
        ("resume_eligible".into(), json!(true)),
        ("last_seen_at".into(), json!(now)),
        ("last_resume_attempt_at".into(), json!("")),
        ("last_resume_attempt_id".into(), json!("")),
        ("last_resume_error".into(), json!("")),
        ("failure_count".into(), json!(0)),
        ("updated_at".into(), json!(utc_now())),
    ]);
    write_recorded(home, group_id, actor_id, document)
}

fn launch_identity(
    command: &[String],
    environment: &BTreeMap<String, String>,
    cwd: &Path,
) -> std::io::Result<(String, Option<LaunchParts>)> {
    cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Claude,
        runtime_mode: cccc_contracts::RuntimeMode::default(),
        command: command.to_vec(),
        environment: environment.clone(),
    }
    .launch_identity_at(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;

    #[test]
    fn a_receipt_saved_before_fingerprints_were_split_still_resumes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("Claude receipt", "")
            .expect("group")
            .group_id;
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        // Encoded outside this code, from the format in use before the split.
        const SAVED: &str = "a2a3b3725b5684ed419c7d060dd8e9202f70da928537f06e210991b79b3fca40";
        const SAVED_COMMAND: &str =
            "1a670870531a6a414b1a98742b0f4348ba8f93fd8040cfe3d2c8d079eca525ed";
        // Written as it was then, not through `record_managed`.
        let saved = json!({"v":2,"kind":"runtime_session","transport":"claude_agent_view","group_id":group,"actor_id":"claude-1","runtime":"claude","workspace_path":workspace_path(&workspace),"command_fingerprint":SAVED_COMMAND,"model":"sonnet","identity_fingerprint":SAVED,"provider_session_id":session_id,"provider_thread_id":"","resume_command_hint":"","captured_from":"claude_agent_view_start","status":"usable","resume_eligible":true,"last_seen_at":"2026-01-01T00:00:00Z","last_resume_attempt_at":"","last_resume_attempt_id":"","last_resume_error":"","failure_count":0,"updated_at":"2026-01-01T00:00:00Z"});
        write(
            &home,
            &group,
            "claude-1",
            saved.as_object().expect("receipt"),
        )
        .expect("saved receipt");
        let command = vec!["claude".to_owned(), "--model".into(), "sonnet".into()];
        let environment = [
            ("CCCC_CLI", "/probe/cccc"),
            ("HOME", "/probe/home"),
            ("PATH", "/probe/bin:/usr/bin"),
            ("PI_CODING_AGENT_DIR", "/probe/pi"),
            ("PROBE_SETTING", "same"),
            ("USERPROFILE", "/probe/profile"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        let resumes = || {
            prepare_managed(
                &home,
                &group,
                "claude-1",
                &workspace,
                &command,
                &environment,
            )
            .expect("prepare")
            .map(|attempt| attempt.session_id)
        };
        assert_eq!(resumes().as_deref(), Some(session_id));

        // The parts go beside the fingerprint, which keeps its value and its version.
        record_managed(
            &home,
            &group,
            "claude-1",
            &workspace,
            &command,
            &environment,
            session_id,
            true,
        )
        .expect("record");
        let stored = read(&home, &group, "claude-1").expect("stored receipt");
        assert_eq!(stored["v"], 2);
        assert_eq!(stored["identity_fingerprint"], SAVED);
        assert_eq!(stored["launch_parts"]["v"], 1);
        for part in ["path", "cccc_cli", "rest"] {
            assert!(stored["launch_parts"][part].is_string(), "{part}");
        }
        assert_eq!(resumes().as_deref(), Some(session_id));
    }

    #[test]
    fn managed_receipt_rejects_legacy_and_identity_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("Claude receipt", "")
            .expect("group");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let command = vec!["claude".into(), "--model".into(), "sonnet".into()];
        let environment = BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".into(),
            temp.path().join("claude-a").to_string_lossy().into_owned(),
        )]);
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";

        record_managed(
            &home,
            &group.group_id,
            "claude-1",
            &workspace,
            &command,
            &environment,
            session_id,
            false,
        )
        .expect("record Claude session");
        let stored = read(&home, &group.group_id, "claude-1").expect("receipt");
        assert!(stored.get("runner").is_none());
        assert_eq!(
            prepare_managed(
                &home,
                &group.group_id,
                "claude-1",
                &workspace,
                &command,
                &environment,
            )
            .expect("prepare Claude resume")
            .as_ref()
            .map(|attempt| attempt.session_id.as_str()),
            Some(session_id)
        );

        let mut changed = environment;
        changed.insert(
            "CLAUDE_CONFIG_DIR".into(),
            temp.path().join("claude-b").to_string_lossy().into_owned(),
        );
        assert!(
            prepare_managed(
                &home,
                &group.group_id,
                "claude-1",
                &workspace,
                &command,
                &changed,
            )
            .expect("changed identity")
            .is_none()
        );

        let mut changed_secret = BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".into(),
            temp.path().join("claude-a").to_string_lossy().into_owned(),
        )]);
        changed_secret.insert("ANTHROPIC_API_KEY".into(), "replacement-key".into());
        assert!(
            prepare_managed(
                &home,
                &group.group_id,
                "claude-1",
                &workspace,
                &command,
                &changed_secret,
            )
            .expect("changed provider configuration")
            .is_none()
        );
    }

    #[test]
    fn managed_receipt_rejects_changed_file_backed_launch_input() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("Claude file identity", "")
            .expect("group");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::write(workspace.join("prompt.txt"), "first prompt").expect("prompt");
        let command = vec![
            "claude".into(),
            "--system-prompt-file".into(),
            "prompt.txt".into(),
        ];
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        record_managed(
            &home,
            &group.group_id,
            "claude-1",
            &workspace,
            &command,
            &BTreeMap::new(),
            session_id,
            false,
        )
        .expect("record session");
        assert_eq!(
            prepare_managed(
                &home,
                &group.group_id,
                "claude-1",
                &workspace,
                &command,
                &BTreeMap::new(),
            )
            .expect("same input")
            .as_ref()
            .map(|attempt| attempt.session_id.as_str()),
            Some(session_id)
        );

        std::fs::write(workspace.join("prompt.txt"), "changed prompt").expect("change prompt");
        assert!(
            prepare_managed(
                &home,
                &group.group_id,
                "claude-1",
                &workspace,
                &command,
                &BTreeMap::new(),
            )
            .expect("changed input")
            .is_none()
        );
    }
}
