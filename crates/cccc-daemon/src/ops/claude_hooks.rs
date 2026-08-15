use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "claude_hook_version.rs"]
mod version;
use version::supported_version;

const HOOK_TIMEOUT_SECONDS: u64 = 3;
const HOOK_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "SessionEnd",
];
const NOTIFICATION_MATCHER: &str =
    "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input|agent_completed";

pub fn configure(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> std::io::Result<super::runtime_hook_session::HookSetup> {
    let launch_token =
        super::codex_mcp::begin_hook_launch(home, "claude", group_id, actor_id, env)?;
    if !is_direct_claude_command(command) {
        super::codex_mcp::record_launch_issue(
            home,
            "claude",
            group_id,
            actor_id,
            &launch_token,
            "HookUnavailableCommand",
        )?;
        return Ok(setup(launch_token, false));
    }
    if !supported_version(&command[0], cwd, env) {
        super::codex_mcp::record_launch_issue(
            home,
            "claude",
            group_id,
            actor_id,
            &launch_token,
            "HookUnavailableVersion",
        )?;
        return Ok(setup(launch_token, false));
    }
    let Some(executable) = super::codex_mcp::configure_actor_cli(env) else {
        super::codex_mcp::record_launch_issue(
            home,
            "claude",
            group_id,
            actor_id,
            &launch_token,
            "HookUnavailableExecutable",
        )?;
        return Ok(setup(launch_token, false));
    };
    if append_settings(command, cwd, &executable).is_err() {
        super::codex_mcp::record_launch_issue(
            home,
            "claude",
            group_id,
            actor_id,
            &launch_token,
            "HookUnavailableSettings",
        )?;
        return Ok(setup(launch_token, false));
    }
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), group_id.to_owned());
    env.insert("CCCC_ACTOR_ID".into(), actor_id.to_owned());
    Ok(setup(launch_token, true))
}

fn setup(launch_token: String, hook_enabled: bool) -> super::runtime_hook_session::HookSetup {
    super::runtime_hook_session::HookSetup {
        runtime: "claude".into(),
        launch_token,
        hook_enabled,
    }
}

fn is_direct_claude_command(command: &[String]) -> bool {
    command
        .first()
        .and_then(|value| value.rsplit(['/', '\\']).next())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "claude" | "claude.exe"))
}

fn append_settings(command: &mut Vec<String>, cwd: &Path, executable: &Path) -> Result<(), String> {
    let mut effective_settings = None;
    let mut retained = Vec::with_capacity(command.len() + 2);
    let mut post_double_dash = Vec::new();
    let mut index = 0;
    while index < command.len() {
        if command[index] == "--" {
            post_double_dash.extend_from_slice(&command[index..]);
            break;
        }
        if command[index] == "--settings" {
            let value = command
                .get(index + 1)
                .ok_or_else(|| "--settings requires a value".to_owned())?;
            effective_settings = Some(value.clone());
            index += 2;
        } else if let Some(value) = command[index].strip_prefix("--settings=") {
            effective_settings = Some(value.to_owned());
            index += 1;
        } else {
            retained.push(command[index].clone());
            index += 1;
        }
    }

    let mut settings = match effective_settings {
        Some(value) => load_settings(&value, cwd)?,
        None => Map::new(),
    };
    let hooks_value = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if hooks_value.is_null() {
        *hooks_value = Value::Object(Map::new());
    }
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| "Claude settings hooks must be an object".to_owned())?;
    let hook_command = super::codex_mcp::hook_command_for(executable, "claude-state");
    for event in HOOK_EVENTS {
        append_hook_group(hooks, event, &hook_command, None)?;
    }
    append_hook_group(
        hooks,
        "Notification",
        &hook_command,
        Some(NOTIFICATION_MATCHER),
    )?;
    retained.extend(["--settings".into(), Value::Object(settings).to_string()]);
    retained.extend(post_double_dash);
    *command = retained;
    Ok(())
}

fn load_settings(value: &str, cwd: &Path) -> Result<Map<String, Value>, String> {
    let source = if value.trim_start().starts_with('{') {
        value.to_owned()
    } else {
        let path = PathBuf::from(value);
        let path = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        fs::read_to_string(&path).map_err(|error| {
            format!("failed to read Claude settings {}: {error}", path.display())
        })?
    };
    serde_json::from_str::<Value>(&source)
        .map_err(|error| format!("invalid Claude settings JSON: {error}"))?
        .as_object()
        .cloned()
        .ok_or_else(|| "Claude settings must be a JSON object".to_owned())
}

fn append_hook_group(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: &str,
    matcher: Option<&str>,
) -> Result<(), String> {
    let groups = hooks
        .entry(event)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| format!("Claude hook {event} must be an array"))?;
    let mut group = Map::new();
    if let Some(matcher) = matcher {
        group.insert("matcher".into(), Value::String(matcher.into()));
    }
    group.insert(
        "hooks".into(),
        json!([{
            "type": "command",
            "command": command,
            "timeout": HOOK_TIMEOUT_SECONDS
        }]),
    );
    groups.push(Value::Object(group));
    Ok(())
}

#[cfg(test)]
#[path = "claude_hooks_tests.rs"]
mod tests;
