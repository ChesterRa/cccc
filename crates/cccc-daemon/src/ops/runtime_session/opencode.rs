use super::{
    command_fingerprint, model_from_command, read, resume_enabled, string, workspace_path, write,
};
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

const MANAGED_RECORD_VERSION: u64 = 2;
const MANAGED_TRANSPORT: &str = "opencode_acp_attach";

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
        || string(&document, "runtime") != "opencode"
        || string(&document, "status") != "usable"
        || !document
            .get("resume_eligible")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || string(&document, "workspace_path") != workspace_path(cwd)
        || string(&document, "command_fingerprint") != command_fingerprint(base_command)
        || string(&document, "model") != model_from_command(base_command)
        || string(&document, "identity_fingerprint")
            != identity_fingerprint(base_command, environment)
        || !string(&document, "provider_thread_id").is_empty()
    {
        return Ok(None);
    }
    let session_id = string(&document, "provider_session_id");
    if !valid_opencode_session_id(&session_id) {
        return Ok(None);
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
) -> std::io::Result<()> {
    if !resume_enabled() {
        return Ok(());
    }
    if !valid_opencode_session_id(session_id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid OpenCode ACP session id",
        ));
    }
    let now = utc_now();
    let document = Map::from_iter([
        ("v".into(), json!(MANAGED_RECORD_VERSION)),
        ("kind".into(), json!("runtime_session")),
        ("transport".into(), json!(MANAGED_TRANSPORT)),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("opencode")),
        ("workspace_path".into(), json!(workspace_path(cwd))),
        (
            "command_fingerprint".into(),
            json!(command_fingerprint(base_command)),
        ),
        ("model".into(), json!(model_from_command(base_command))),
        (
            "identity_fingerprint".into(),
            json!(identity_fingerprint(base_command, environment)),
        ),
        ("provider_session_id".into(), json!(session_id)),
        ("provider_thread_id".into(), json!("")),
        ("resume_command_hint".into(), json!("")),
        (
            "captured_from".into(),
            json!(if resumed {
                "opencode_acp_session_load"
            } else {
                "opencode_acp_session_new"
            }),
        ),
        ("status".into(), json!("usable")),
        ("resume_eligible".into(), json!(true)),
        ("last_seen_at".into(), json!(now)),
        ("last_resume_attempt_at".into(), json!("")),
        ("last_resume_error".into(), json!("")),
        ("failure_count".into(), json!(0)),
        ("updated_at".into(), json!(utc_now())),
    ]);
    write(home, group_id, actor_id, &document)
}

fn identity_fingerprint(command: &[String], environment: &BTreeMap<String, String>) -> String {
    cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Opencode,
        command: command.to_vec(),
        environment: environment.clone(),
    }
    .identity_fingerprint()
}

fn valid_opencode_session_id(value: &str) -> bool {
    value.len() <= 128
        && value.starts_with("ses")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;

    const SESSION_ID: &str = "ses_f9cf78ed2ffeQvcX7bSyTWKpW6";

    #[test]
    fn managed_receipt_round_trips_an_opencode_session_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("OpenCode receipt", "").expect("group");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let command = vec!["opencode".into(), "--model".into(), "test/model".into()];
        let environment = BTreeMap::from([("XDG_DATA_HOME".into(), "/tmp/opencode-home".into())]);

        record_managed(
            &home,
            &group.group_id,
            "opencode-1",
            &workspace,
            &command,
            &environment,
            SESSION_ID,
            false,
        )
        .expect("record OpenCode session");
        let stored = read(&home, &group.group_id, "opencode-1").expect("receipt");
        assert!(stored.get("runner").is_none());
        assert_eq!(
            prepare_managed(
                &home,
                &group.group_id,
                "opencode-1",
                &workspace,
                &command,
                &environment,
            )
            .expect("prepare OpenCode resume")
            .as_deref(),
            Some(SESSION_ID)
        );
    }

    #[test]
    fn rejects_unsafe_or_unrelated_session_ids() {
        assert!(valid_opencode_session_id(SESSION_ID));
        for value in ["", "not-a-session", "ses/../../other", "ses_\nother"] {
            assert!(!valid_opencode_session_id(value), "accepted {value:?}");
        }
        assert!(!valid_opencode_session_id(&format!(
            "ses_{}",
            "a".repeat(129)
        )));
    }
}
