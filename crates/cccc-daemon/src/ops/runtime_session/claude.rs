use super::{
    command_fingerprint, model_from_command, read, resume_enabled, string, valid_session_id,
    workspace_path, write,
};
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

const MANAGED_RECORD_VERSION: u64 = 2;
const MANAGED_TRANSPORT: &str = "claude_agent_view";

#[cfg(test)]
#[path = "claude_failure_tests.rs"]
mod failure_tests;

pub struct ResumeAttempt {
    pub session_id: String,
    pub attempt_id: String,
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
        || string(&document, "identity_fingerprint")
            != identity_fingerprint(base_command, environment, cwd)?
        || !string(&document, "provider_thread_id").is_empty()
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
    }))
}

/// Preserve the original conversation and fence late failures from newer attempts/successes.
pub fn record_resume_failure(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    attempt: &ResumeAttempt,
    diagnostic: &str,
    blocked: bool,
) -> io::Result<()> {
    let mut document = read(home, group_id, actor_id)?;
    if string(&document, "provider_session_id") != attempt.session_id
        || string(&document, "last_resume_attempt_id") != attempt.attempt_id
    {
        return Ok(());
    }
    document.insert("last_resume_error".into(), json!(diagnostic));
    let failures = document
        .get("failure_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    document.insert("failure_count".into(), json!(failures.saturating_add(1)));
    if blocked {
        document.insert("status".into(), json!("resume_failed"));
        document.insert("resume_eligible".into(), json!(false));
    }
    // A terminal attempt must not record the same failure twice.
    document.insert("last_resume_attempt_id".into(), json!(""));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)
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
        (
            "identity_fingerprint".into(),
            json!(identity_fingerprint(base_command, environment, cwd)?),
        ),
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
    write(home, group_id, actor_id, &document)
}

fn identity_fingerprint(
    command: &[String],
    environment: &BTreeMap<String, String>,
    cwd: &Path,
) -> std::io::Result<String> {
    cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Claude,
        runtime_mode: cccc_contracts::RuntimeMode::default(),
        command: command.to_vec(),
        environment: environment.clone(),
    }
    .identity_fingerprint_at(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;

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
