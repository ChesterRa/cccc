use cccc_contracts::utc_now;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

mod claude;
mod grok;
#[cfg(test)]
mod grok_tests;
mod opencode;
pub use claude::{
    prepare_managed as prepare_claude_managed_session,
    record_managed as record_claude_managed_session,
};
pub use grok::{
    prepare_managed as prepare_grok_managed_session, record_managed as record_grok_managed_session,
};
pub use opencode::{
    prepare_managed as prepare_opencode_managed_session,
    record_managed as record_opencode_managed_session,
};

const NO_RESUME_VALUES: [&str; 4] = ["0", "false", "no", "off"];
const MANAGED_RECORD_VERSION: u64 = 2;
const CODEX_MANAGED_TRANSPORT: &str = "codex_app_server";

pub fn prepare_codex_app_thread(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &[String],
    environment: &std::collections::BTreeMap<String, String>,
    model: &str,
) -> std::io::Result<Option<String>> {
    if !resume_enabled() {
        return Ok(None);
    }
    let Ok(mut document) = read(home, group_id, actor_id) else {
        return Ok(None);
    };
    if document.get("v").and_then(Value::as_u64) != Some(MANAGED_RECORD_VERSION)
        || string(&document, "kind") != "runtime_session"
        || string(&document, "transport") != CODEX_MANAGED_TRANSPORT
        || string(&document, "runtime") != "codex"
        || string(&document, "status") != "usable"
        || !document
            .get("resume_eligible")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || string(&document, "workspace_path") != workspace_path(cwd)
        || string(&document, "command_fingerprint") != app_thread_command_fingerprint(command)
        || string(&document, "model") != model.trim()
        || string(&document, "identity_fingerprint")
            != codex_identity_fingerprint(command, environment)
        || !string(&document, "provider_session_id").is_empty()
    {
        return Ok(None);
    }
    let thread_id = string(&document, "provider_thread_id");
    if thread_id.is_empty() {
        return Ok(None);
    }
    let now = utc_now();
    document.insert("last_resume_attempt_at".into(), json!(now));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)?;
    Ok(Some(thread_id))
}

pub struct CodexAppThread<'a> {
    pub id: &'a str,
    pub resumed: bool,
}

pub fn record_codex_app_thread(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &[String],
    environment: &std::collections::BTreeMap<String, String>,
    thread: CodexAppThread<'_>,
) -> std::io::Result<()> {
    let thread_id = thread.id;
    if !resume_enabled() {
        return Ok(());
    }
    if thread_id.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty Codex app-server thread id",
        ));
    }
    let now = utc_now();
    let document = Map::from_iter([
        ("v".into(), json!(MANAGED_RECORD_VERSION)),
        ("kind".into(), json!("runtime_session")),
        ("transport".into(), json!(CODEX_MANAGED_TRANSPORT)),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("codex")),
        ("workspace_path".into(), json!(workspace_path(cwd))),
        (
            "command_fingerprint".into(),
            json!(app_thread_command_fingerprint(command)),
        ),
        ("model".into(), json!(model_from_command(command))),
        (
            "identity_fingerprint".into(),
            json!(codex_identity_fingerprint(command, environment)),
        ),
        ("provider_session_id".into(), json!("")),
        ("provider_thread_id".into(), json!(thread_id.trim())),
        ("resume_command_hint".into(), json!("")),
        (
            "captured_from".into(),
            json!(if thread.resumed {
                "app_server_thread_resume"
            } else {
                "app_server_thread_start"
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

pub fn remove(home: &HomeLayout, group_id: &str, actor_id: &str) -> std::io::Result<()> {
    let path = path(home, group_id, actor_id)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn snapshot(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> std::io::Result<Option<Map<String, Value>>> {
    let session_path = path(home, group_id, actor_id)?;
    if !session_path.exists() {
        return Ok(None);
    }
    read(home, group_id, actor_id).map(Some)
}

pub fn restore_snapshot(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    snapshot: Option<&Map<String, Value>>,
) -> std::io::Result<()> {
    if let Some(document) = snapshot {
        write(home, group_id, actor_id, document)
    } else {
        remove(home, group_id, actor_id)
    }
}

pub fn actor_fields(home: &HomeLayout, group_id: &str, actor_id: &str) -> Map<String, Value> {
    let document = read(home, group_id, actor_id).unwrap_or_default();
    Map::from_iter([
        (
            "runtime_session_status".into(),
            nullable_string(&document, "status"),
        ),
        (
            "runtime_session_resume_eligible".into(),
            document
                .get("resume_eligible")
                .cloned()
                .filter(Value::is_boolean)
                .unwrap_or(Value::Null),
        ),
        (
            "runtime_session_last_resume_error".into(),
            nullable_string(&document, "last_resume_error"),
        ),
    ])
}

fn read(home: &HomeLayout, group_id: &str, actor_id: &str) -> std::io::Result<Map<String, Value>> {
    let value: Value = cccc_core::fs::read_json(&path(home, group_id, actor_id)?)?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| std::io::Error::other("runtime session document is not an object"))
}

fn write(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    document: &Map<String, Value>,
) -> std::io::Result<()> {
    cccc_core::fs::write_json(&path(home, group_id, actor_id)?, document)
}

fn path(home: &HomeLayout, group_id: &str, actor_id: &str) -> std::io::Result<PathBuf> {
    let safe_actor_id = actor_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || (!actor_id.is_empty()
            && !actor_id.contains(['/', '\\'])
            && actor_id != "."
            && actor_id != "..");
    if !safe_actor_id {
        return Err(std::io::Error::other("invalid actor id"));
    }
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("runtime_sessions")
        .join(format!("{actor_id}.json")))
}

fn command_fingerprint(command: &[String]) -> String {
    let raw = serde_json::to_vec(&json!({"argv":command})).unwrap_or_default();
    format!("{:x}", Sha256::digest(raw))
}

fn app_thread_command_fingerprint(command: &[String]) -> String {
    let stable = stable_app_thread_command(command);
    let raw = serde_json::to_vec(&json!({"argv":stable})).unwrap_or_default();
    format!("{:x}", Sha256::digest(raw))
}

fn codex_identity_fingerprint(
    command: &[String],
    environment: &std::collections::BTreeMap<String, String>,
) -> String {
    cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Codex,
        command: command.to_vec(),
        environment: environment.clone(),
    }
    .identity_fingerprint()
}

fn stable_app_thread_command(command: &[String]) -> Vec<String> {
    if command.len() < 2
        || Path::new(&command[0])
            .file_name()
            .and_then(|value| value.to_str())
            != Some("codex")
        || command[1] != "app-server"
    {
        return command.to_vec();
    }
    let mut stable = Vec::with_capacity(command.len());
    let mut skip_next = false;
    for item in command {
        if skip_next {
            skip_next = false;
        } else if item == "--listen" {
            stable.push(item.clone());
            skip_next = true;
        } else if item.starts_with("--listen=") {
            stable.push("--listen".into());
        } else {
            stable.push(item.clone());
        }
    }
    stable
}

fn model_from_command(command: &[String]) -> String {
    for (index, item) in command.iter().enumerate() {
        if matches!(item.as_str(), "-m" | "--model") {
            return command.get(index + 1).cloned().unwrap_or_default();
        }
        if let Some(model) = item.strip_prefix("--model=") {
            return model.trim().to_owned();
        }
    }
    String::new()
}

fn valid_session_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

fn workspace_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn resume_enabled() -> bool {
    std::env::var("CCCC_RUNTIME_RESUME")
        .ok()
        .map(|value| !NO_RESUME_VALUES.contains(&value.trim().to_ascii_lowercase().as_str()))
        .unwrap_or(true)
}

fn string(document: &Map<String, Value>, key: &str) -> String {
    document
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn nullable_string(document: &Map<String, Value>, key: &str) -> Value {
    let value = string(document, key);
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, HomeLayout, String, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("runtime session", "")
            .expect("group");
        let cwd = temp.path().join("repo");
        std::fs::create_dir(&cwd).expect("cwd");
        (temp, home, group.group_id, cwd)
    }

    fn app_thread_command() -> Vec<String> {
        vec![
            "codex".into(),
            "app-server".into(),
            "--listen".into(),
            "stdio://".into(),
        ]
    }

    #[test]
    fn records_managed_codex_app_thread_metadata() {
        let (_temp, home, group_id, cwd) = fixture();
        let command = app_thread_command();
        let environment =
            std::collections::BTreeMap::from([("CODEX_HOME".into(), "/tmp/codex-a".into())]);

        record_codex_app_thread(
            &home,
            &group_id,
            "peer1",
            &cwd,
            &command,
            &environment,
            CodexAppThread {
                id: "thread-1",
                resumed: false,
            },
        )
        .expect("record app thread");

        let stored = read(&home, &group_id, "peer1").expect("stored metadata");
        assert_eq!(stored["v"], MANAGED_RECORD_VERSION);
        assert_eq!(stored["transport"], CODEX_MANAGED_TRANSPORT);
        assert!(stored.get("runner").is_none());
        assert_eq!(stored["provider_session_id"], "");
        assert_eq!(stored["provider_thread_id"], "thread-1");
        assert_eq!(stored["captured_from"], "app_server_thread_start");
        assert_eq!(stored["status"], "usable");
        assert_eq!(stored["resume_eligible"], true);
        assert_eq!(
            stored["command_fingerprint"],
            "e21da22b1aea2a44604536594c24efbfc4eabe61a03b833c6cb64b09f13ecad4"
        );

        record_codex_app_thread(
            &home,
            &group_id,
            "peer2",
            &cwd,
            &command,
            &environment,
            CodexAppThread {
                id: "thread-2",
                resumed: true,
            },
        )
        .expect("record resumed app thread");
        let resumed = read(&home, &group_id, "peer2").expect("stored resumed metadata");
        assert_eq!(resumed["provider_thread_id"], "thread-2");
        assert_eq!(resumed["captured_from"], "app_server_thread_resume");
    }

    #[test]
    fn prepares_codex_app_thread_only_when_contract_and_identity_match() {
        let (_temp, home, group_id, cwd) = fixture();
        let command = app_thread_command();
        let environment =
            std::collections::BTreeMap::from([("CODEX_HOME".into(), "/tmp/codex-a".into())]);
        record_codex_app_thread(
            &home,
            &group_id,
            "peer1",
            &cwd,
            &command,
            &environment,
            CodexAppThread {
                id: "thread-1",
                resumed: false,
            },
        )
        .expect("record");

        let prepared =
            prepare_codex_app_thread(&home, &group_id, "peer1", &cwd, &command, &environment, "")
                .expect("prepare app thread");

        assert_eq!(prepared.as_deref(), Some("thread-1"));
        let stored = read(&home, &group_id, "peer1").expect("stored metadata");
        assert!(
            stored["last_resume_attempt_at"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );

        let changed_environment =
            std::collections::BTreeMap::from([("CODEX_HOME".into(), "/tmp/codex-b".into())]);
        assert!(
            prepare_codex_app_thread(
                &home,
                &group_id,
                "peer1",
                &cwd,
                &command,
                &changed_environment,
                "",
            )
            .expect("changed identity")
            .is_none()
        );

        let mut legacy = read(&home, &group_id, "peer1").expect("receipt");
        legacy.insert("v".into(), json!(1));
        legacy.remove("transport");
        write(&home, &group_id, "peer1", &legacy).expect("legacy receipt");
        assert!(
            prepare_codex_app_thread(&home, &group_id, "peer1", &cwd, &command, &environment, "",)
                .expect("legacy receipt")
                .is_none()
        );
    }

    #[test]
    fn app_thread_fingerprint_normalizes_only_the_listen_target() {
        let stdio = app_thread_command();
        let websocket = vec![
            "codex".into(),
            "app-server".into(),
            "--listen=ws://127.0.0.1:12345".into(),
        ];
        assert_eq!(
            app_thread_command_fingerprint(&stdio),
            app_thread_command_fingerprint(&websocket)
        );
    }
}
