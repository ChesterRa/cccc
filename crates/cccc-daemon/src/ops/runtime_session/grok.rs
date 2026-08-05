use super::{
    PreparedCommand, command_fingerprint, model_from_command, read, resume_enabled, string,
    valid_session_id, workspace_path, write,
};
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::path::Path;

const SUBCOMMANDS: [&str; 22] = [
    "agent",
    "completions",
    "dashboard",
    "export",
    "help",
    "inspect",
    "leader",
    "login",
    "logout",
    "mcp",
    "memory",
    "models",
    "plugin",
    "sessions",
    "setup",
    "trace",
    "update",
    "version",
    "worktree",
    "wrap",
    "--help",
    "--version",
];

pub fn prepare(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
) -> PreparedCommand {
    prepare_inner(home, group_id, actor_id, cwd, base_command, false)
}

pub fn prepare_fresh(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
) -> PreparedCommand {
    prepare_inner(home, group_id, actor_id, cwd, base_command, true)
}

fn prepare_inner(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
    force_fresh: bool,
) -> PreparedCommand {
    let unmanaged = || PreparedCommand {
        command: base_command.to_vec(),
        resumed_session_id: None,
    };
    if !resume_enabled() || !supports_managed_session(base_command) {
        return unmanaged();
    }
    if !force_fresh
        && let Ok(mut document) = read(home, group_id, actor_id)
        && string(&document, "runtime") == "grok"
        && string(&document, "status") == "usable"
        && document
            .get("resume_eligible")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        && string(&document, "workspace_path") == workspace_path(cwd)
        && string(&document, "command_fingerprint") == command_fingerprint(base_command)
        && string(&document, "model") == model_from_command(base_command)
    {
        let session_id = string(&document, "provider_session_id");
        if valid_session_id(&session_id) {
            document.insert("last_resume_attempt_at".into(), json!(utc_now()));
            document.insert("updated_at".into(), json!(utc_now()));
            let _ = write(home, group_id, actor_id, &document);
            let mut command = vec![
                base_command[0].clone(),
                "--resume".into(),
                session_id.clone(),
            ];
            command.extend_from_slice(&base_command[1..]);
            return PreparedCommand {
                command,
                resumed_session_id: Some(session_id),
            };
        }
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    if record(home, group_id, actor_id, cwd, base_command, &session_id).is_err() {
        return unmanaged();
    }
    let mut command = vec![base_command[0].clone(), "--session-id".into(), session_id];
    command.extend_from_slice(&base_command[1..]);
    PreparedCommand {
        command,
        resumed_session_id: None,
    }
}

fn record(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
    session_id: &str,
) -> std::io::Result<()> {
    let now = utc_now();
    let document = Map::from_iter([
        ("v".into(), json!(1)),
        ("kind".into(), json!("runtime_session")),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("grok")),
        ("runner".into(), json!("pty")),
        ("workspace_path".into(), json!(workspace_path(cwd))),
        (
            "command_fingerprint".into(),
            json!(command_fingerprint(base_command)),
        ),
        ("model".into(), json!(model_from_command(base_command))),
        ("provider_session_id".into(), json!(session_id)),
        ("provider_thread_id".into(), json!("")),
        (
            "resume_command_hint".into(),
            json!(format!("grok --resume {session_id}")),
        ),
        ("captured_from".into(), json!("grok_generated_session_id")),
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

fn supports_managed_session(command: &[String]) -> bool {
    let grok = command
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|program| program.to_str())
        == Some(if cfg!(windows) { "grok.exe" } else { "grok" });
    grok && !command.iter().skip(1).any(|item| {
        SUBCOMMANDS.contains(&item.as_str())
            || matches!(
                item.as_str(),
                "--resume" | "-r" | "--continue" | "-c" | "--session-id" | "-s" | "--fork-session"
            )
            || item.starts_with("--resume=")
            || item.starts_with("--session-id=")
            || (item.len() > 2 && (item.starts_with("-r") || item.starts_with("-s")))
    })
}
