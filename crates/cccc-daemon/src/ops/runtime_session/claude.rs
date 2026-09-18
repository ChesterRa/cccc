use super::{
    command_fingerprint, model_from_command, read, resume_enabled, string, valid_session_id,
    workspace_path, write,
};
use crate::ops::codex_voice_analyst::{WorkerIdentity, reap_unreachable_worker};
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

const MANAGED_RECORD_VERSION: u64 = 2;
const MANAGED_TRANSPORT: &str = "claude_agent_view";

pub fn prepare_managed(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
    environment: &BTreeMap<String, String>,
) -> std::io::Result<Option<String>> {
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
        || string(&document, "status") != "usable"
        || !document
            .get("resume_eligible")
            .and_then(Value::as_bool)
            .unwrap_or(false)
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
    // A worker tree whose daemon is gone cannot be resumed; the conversation can.
    // Reap it now so the fresh worker is not a second copy.
    if let Some(worker) = recorded_worker(&document) {
        match reap_unreachable_worker(environment, &worker) {
            Ok(None) => {}
            Ok(Some(_)) => {
                document.remove("provider_worker_pid");
                document.remove("provider_worker_started");
            }
            Err(error) => {
                tracing::warn!(%error, %group_id, %actor_id, "could not check the recorded Claude worker");
            }
        }
    }
    document.insert("last_resume_attempt_at".into(), json!(utc_now()));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)?;
    Ok(Some(session_id))
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
    worker: Option<&WorkerIdentity>,
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
        ("last_resume_error".into(), json!("")),
        ("failure_count".into(), json!(0)),
        ("updated_at".into(), json!(utc_now())),
        ("provider_worker_pid".into(), json!(worker.map(|w| w.pid))),
        (
            "provider_worker_started".into(),
            json!(worker.map(|w| w.started.as_str())),
        ),
    ]);
    write(home, group_id, actor_id, &document)
}

fn recorded_worker(document: &Map<String, Value>) -> Option<WorkerIdentity> {
    let pid = document
        .get("provider_worker_pid")
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())?;
    let started = string(document, "provider_worker_started");
    (!started.is_empty()).then_some(WorkerIdentity { pid, started })
}

fn identity_fingerprint(
    command: &[String],
    environment: &BTreeMap<String, String>,
    cwd: &Path,
) -> std::io::Result<String> {
    cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Claude,
        command: command.to_vec(),
        environment: environment.clone(),
    }
    .identity_fingerprint_at(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;

    #[cfg(unix)]
    #[test]
    fn resume_reaps_a_worker_whose_daemon_is_gone_and_keeps_the_conversation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("Claude orphan", "")
            .expect("group");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let config_dir = temp.path().join("claude-config");
        std::fs::create_dir(&config_dir).expect("config dir");
        let command = vec!["claude".into()];
        let environment = BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".into(),
            config_dir.to_string_lossy().into_owned(),
        )]);
        let (mut host, _worker) = crate::ops::codex_voice_analyst::spawn_fake_worker_host();
        let started =
            crate::ops::codex_voice_analyst::identify_worker(host.id()).expect("identity");
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
            Some(&started),
        )
        .expect("record");
        assert_eq!(
            read(&home, &group.group_id, "claude-1").expect("receipt")["provider_worker_pid"],
            json!(host.id())
        );

        let resumed = prepare_managed(
            &home,
            &group.group_id,
            "claude-1",
            &workspace,
            &command,
            &environment,
        )
        .expect("prepare");
        assert_eq!(
            resumed.as_deref(),
            Some(session_id),
            "the conversation stays resumable"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            host.try_wait().expect("poll").is_some(),
            "orphaned worker host must be reaped"
        );
        let receipt = read(&home, &group.group_id, "claude-1").expect("receipt");
        assert!(receipt.get("provider_worker_pid").is_none());
        assert!(receipt.get("provider_worker_started").is_none());
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
            None,
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
            .as_deref(),
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
            None,
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
            .as_deref(),
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
