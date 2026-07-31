use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const HOOK_TIMEOUT_SECONDS: u64 = 3;
const MIN_CLAUDE_VERSION: (u64, u64, u64) = (2, 1, 141);
const HOOK_EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
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

fn supported_version(executable: &str, cwd: &Path, env: &BTreeMap<String, String>) -> bool {
    Command::new(executable)
        .arg("--version")
        .current_dir(cwd)
        .envs(env)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            parse_version(&text)
        })
        .is_some_and(|version| version >= MIN_CLAUDE_VERSION)
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word
            .trim_matches(|character: char| !character.is_ascii_digit() && character != '.')
            .split('.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    })
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
mod tests {
    use super::{
        MIN_CLAUDE_VERSION, NOTIFICATION_MATCHER, append_settings, is_direct_claude_command,
        parse_version,
    };
    use serde_json::Value;
    use std::fs;
    use std::path::Path;

    #[test]
    fn merges_the_effective_inline_settings_into_one_argument() {
        let mut command = vec![
            "claude".into(),
            "--settings".into(),
            r#"{"language":"ignored"}"#.into(),
            "--model".into(),
            "sonnet".into(),
            "--settings".into(),
            r#"{"language":"chinese","hooks":{"Stop":[{"matcher":"existing"}]}}"#.into(),
        ];
        append_settings(
            &mut command,
            Path::new("/workspace"),
            Path::new("/tmp/cccc bin/cccc"),
        )
        .expect("merge settings");

        assert_eq!(
            command.iter().filter(|item| *item == "--settings").count(),
            1
        );
        assert_eq!(&command[..3], ["claude", "--model", "sonnet"]);
        let settings: Value =
            serde_json::from_str(command.last().expect("settings")).expect("inline settings");
        assert_eq!(settings["language"], "chinese");
        assert_eq!(settings["hooks"]["Stop"][0]["matcher"], "existing");
        assert_eq!(settings["hooks"]["Stop"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            settings["hooks"]["Notification"][0]["matcher"],
            NOTIFICATION_MATCHER
        );
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "Notification",
            "Stop",
            "SessionEnd",
        ] {
            let handler = settings["hooks"][event]
                .as_array()
                .and_then(|groups| groups.last())
                .map(|group| &group["hooks"][0])
                .expect("CCCC hook");
            assert_eq!(handler["type"], "command");
            assert_eq!(handler["timeout"], 3);
            assert!(
                handler["command"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("hook claude-state")
            );
        }
        assert!(settings["hooks"]["StopFailure"].is_null());
        assert!(settings["hooks"]["SubagentStart"].is_null());
    }

    #[test]
    fn merges_a_relative_settings_file_without_mutating_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("claude.json");
        fs::write(&path, r#"{"env":{"EXAMPLE":"kept"}}"#).expect("settings file");
        let mut command = vec!["claude".into(), "--settings=claude.json".into()];

        append_settings(&mut command, temp.path(), Path::new("/bin/cccc"))
            .expect("merge file settings");

        let settings: Value =
            serde_json::from_str(command.last().expect("settings")).expect("inline settings");
        assert_eq!(settings["env"]["EXAMPLE"], "kept");
        assert_eq!(
            fs::read_to_string(path).expect("original file"),
            r#"{"env":{"EXAMPLE":"kept"}}"#
        );
    }

    #[test]
    fn merges_an_absolute_settings_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("absolute.json");
        fs::write(&path, r#"{"model":"sonnet"}"#).expect("settings file");
        let mut command = vec![
            "claude".into(),
            "--settings".into(),
            path.to_string_lossy().into_owned(),
        ];

        append_settings(&mut command, Path::new("/ignored"), Path::new("/bin/cccc"))
            .expect("merge absolute settings");

        let settings: Value =
            serde_json::from_str(command.last().expect("settings")).expect("inline settings");
        assert_eq!(settings["model"], "sonnet");
    }

    #[test]
    fn rejects_invalid_settings_without_partially_rewriting_command() {
        let original = vec!["claude".into(), "--settings".into(), "missing.json".into()];
        let mut command = original.clone();
        assert!(
            append_settings(
                &mut command,
                Path::new("/workspace"),
                Path::new("/bin/cccc")
            )
            .is_err()
        );
        assert_eq!(command, original);
    }

    #[test]
    fn does_not_treat_prompt_text_after_double_dash_as_cli_settings() {
        let mut command = vec![
            "claude".into(),
            "--".into(),
            "--settings".into(),
            "is prompt text".into(),
        ];
        append_settings(
            &mut command,
            Path::new("/workspace"),
            Path::new("/bin/cccc"),
        )
        .expect("append settings");
        assert_eq!(command[0], "claude");
        assert_eq!(command[1], "--settings");
        assert_eq!(&command[3..], ["--", "--settings", "is prompt text"]);
    }

    #[test]
    fn only_direct_claude_commands_are_eligible() {
        assert!(is_direct_claude_command(&["claude".into()]));
        assert!(is_direct_claude_command(&["/opt/bin/claude".into()]));
        assert!(is_direct_claude_command(&[r"C:\bin\claude.exe".into()]));
        assert!(!is_direct_claude_command(&[
            "wrapper".into(),
            "claude".into()
        ]));
        assert!(!is_direct_claude_command(&[]));
    }

    #[test]
    fn parses_and_enforces_the_documented_version_floor() {
        assert_eq!(parse_version("2.1.205 (Claude Code)"), Some((2, 1, 205)));
        assert_eq!(parse_version("claude 2.1.141"), Some(MIN_CLAUDE_VERSION));
        assert!(parse_version("unknown").is_none());
        assert!((2, 1, 140) < MIN_CLAUDE_VERSION);
    }

    #[cfg(unix)]
    #[test]
    fn probes_a_relative_claude_executable_from_actor_cwd() {
        use std::collections::BTreeMap;
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp.path().join("claude");
        fs::write(&executable, "#!/bin/sh\necho '2.1.205 (Claude Code)'\n").expect("fake claude");
        let mut permissions = fs::metadata(&executable).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("permissions");

        assert!(super::supported_version(
            "./claude",
            temp.path(),
            &BTreeMap::new()
        ));
    }
}
