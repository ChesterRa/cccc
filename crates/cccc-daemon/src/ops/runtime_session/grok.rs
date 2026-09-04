use super::{
    command_fingerprint, model_from_command, read, resume_enabled, string, valid_session_id,
    workspace_path, write,
};
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

const MANAGED_RECORD_VERSION: u64 = 2;
const MANAGED_TRANSPORT: &str = "grok_acp_leader";

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
        || string(&document, "runtime") != "grok"
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
    if !valid_session_id(&session_id) {
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
    if !valid_session_id(session_id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid Grok ACP session id",
        ));
    }
    let now = utc_now();
    let document = Map::from_iter([
        ("v".into(), json!(MANAGED_RECORD_VERSION)),
        ("kind".into(), json!("runtime_session")),
        ("transport".into(), json!(MANAGED_TRANSPORT)),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("grok")),
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
                "grok_acp_session_load"
            } else {
                "grok_acp_session_new"
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
        runtime: cccc_contracts::ActorRuntime::Grok,
        command: command.to_vec(),
        environment: environment.clone(),
    }
    .identity_fingerprint()
}
