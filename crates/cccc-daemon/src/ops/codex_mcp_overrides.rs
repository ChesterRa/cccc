use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

const HOOK_TIMEOUT_SECONDS: u64 = 3;
// Keep this list aligned with Codex's documented hook contract. Failed tool
// commands are reported through PostToolUse; Codex does not currently expose
// separate PostToolUseFailure or StopFailure hook events.
const HOOK_EVENTS: [(&str, &str); 9] = [
    ("SessionStart", "session_start"),
    ("UserPromptSubmit", "user_prompt_submit"),
    ("PreToolUse", "pre_tool_use"),
    ("PermissionRequest", "permission_request"),
    ("PostToolUse", "post_tool_use"),
    ("SubagentStart", "subagent_start"),
    ("SubagentStop", "subagent_stop"),
    ("Stop", "stop"),
    ("SessionEnd", "session_end"),
];

#[cfg(test)]
pub(super) fn append_overrides(
    command: &mut Vec<String>,
    home: &Path,
    executable: &Path,
    group_id: &str,
    actor_id: &str,
) {
    append_mcp_overrides(command, home, executable, group_id, actor_id);
    append_hook_overrides(command, executable);
}

pub(super) fn append_mcp_overrides(
    command: &mut Vec<String>,
    home: &Path,
    executable: &Path,
    group_id: &str,
    actor_id: &str,
) {
    let executable_toml = toml_string(executable);
    let home = toml_string(home);
    let group_id = serde_json::to_string(group_id).unwrap_or_else(|_| "\"\"".into());
    let actor_id = serde_json::to_string(actor_id).unwrap_or_else(|_| "\"\"".into());
    insert_before_prompt_tail(
        command,
        [
            "-c".into(),
            format!("mcp_servers.cccc.command={executable_toml}"),
            "-c".into(),
            "mcp_servers.cccc.args=[\"mcp\"]".into(),
            "-c".into(),
            format!("mcp_servers.cccc.env.CCCC_HOME={home}"),
            "-c".into(),
            format!("mcp_servers.cccc.env.CCCC_GROUP_ID={group_id}"),
            "-c".into(),
            format!("mcp_servers.cccc.env.CCCC_ACTOR_ID={actor_id}"),
        ],
    );
}

pub(super) fn append_hook_overrides(command: &mut Vec<String>, executable: &Path) {
    insert_before_prompt_tail(command, hook_arguments(executable));
}

pub(super) fn hook_arguments(executable: &Path) -> Vec<String> {
    let hook_command = super::hook_command_for(executable, "codex-state");
    let hook_command_toml = serde_json::to_string(&hook_command).unwrap_or_else(|_| "\"\"".into());
    let mut overrides = Vec::new();
    for (event_name, _) in HOOK_EVENTS {
        overrides.extend([
            "-c".into(),
            format!(
                "hooks.{event_name}=[{{hooks=[{{type=\"command\",command={hook_command_toml},timeout={HOOK_TIMEOUT_SECONDS}}}]}}]"
            ),
        ]);
    }
    let trusted = HOOK_EVENTS
        .iter()
        .map(|(_, event_key)| {
            let key = format!("/<session-flags>/config.toml:{event_key}:0:0");
            let key = serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".into());
            let hash = hook_hash(event_key, &hook_command);
            format!("{key}={{trusted_hash=\"{hash}\"}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    overrides.extend(["-c".into(), format!("hooks.state={{{trusted}}}")]);
    overrides
}

pub(super) fn hook_hash(event_key: &str, command: &str) -> String {
    let mut identity = json!({
        "event_name": event_key,
        "hooks": [{
            "async": false,
            "command": command,
            "timeout": HOOK_TIMEOUT_SECONDS,
            "type": "command"
        }]
    });
    canonicalize(&mut identity);
    let digest = Sha256::digest(serde_json::to_vec(&identity).unwrap_or_default());
    format!("sha256:{digest:x}")
}

fn canonicalize(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for value in object.values_mut() {
                canonicalize(value);
            }
            let mut sorted = std::mem::take(object).into_iter().collect::<Vec<_>>();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            object.extend(sorted);
        }
        Value::Array(items) => items.iter_mut().for_each(canonicalize),
        _ => {}
    }
}

fn insert_before_prompt_tail(
    command: &mut Vec<String>,
    arguments: impl IntoIterator<Item = String>,
) {
    let index = command
        .iter()
        .position(|argument| argument == "--")
        .unwrap_or(command.len());
    command.splice(index..index, arguments);
}

fn toml_string(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).unwrap_or_else(|_| "\"\"".into())
}
