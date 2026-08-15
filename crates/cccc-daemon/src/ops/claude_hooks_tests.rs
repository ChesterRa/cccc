use super::version::{MIN_CLAUDE_VERSION, parse_version};
use super::{NOTIFICATION_MATCHER, append_settings, is_direct_claude_command};
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
        "PostToolUseFailure",
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

    assert!(super::version::supported_version(
        "./claude",
        temp.path(),
        &BTreeMap::new()
    ));
}
