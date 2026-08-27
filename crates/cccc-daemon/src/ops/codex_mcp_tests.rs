use super::launcher::{prepend_executable_dir, resolve_on_path, valid_public_launcher};
use super::overrides::{append_overrides, hook_hash};
use super::{
    CodexLaunch, begin_hook_launch, configure_with_executable, hook_command_for_platform,
    is_direct_codex_command, record_launch_issue,
};
use cccc_core::HomeLayout;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn appends_absolute_mcp_overrides_and_trusts_only_injected_hooks() {
    let mut command = vec!["codex".into(), "--search".into()];
    append_overrides(
        &mut command,
        Path::new("/tmp/cccc home"),
        Path::new("/tmp/cccc bin/cccc"),
        "g_test",
        "backend",
    );
    assert!(command.contains(&"mcp_servers.cccc.command=\"/tmp/cccc bin/cccc\"".into()));
    assert!(command.contains(&"mcp_servers.cccc.args=[\"mcp\"]".into()));
    assert!(command.contains(&"mcp_servers.cccc.env.CCCC_HOME=\"/tmp/cccc home\"".into()));
    assert!(command.contains(&"mcp_servers.cccc.env.CCCC_GROUP_ID=\"g_test\"".into()));
    assert!(command.contains(&"mcp_servers.cccc.env.CCCC_ACTOR_ID=\"backend\"".into()));
    for event in [
        "UserPromptSubmit",
        "PermissionRequest",
        "PostToolUse",
        "Stop",
    ] {
        assert!(
            command
                .iter()
                .any(|item| item.starts_with(&format!("hooks.{event}=")))
        );
    }
    assert!(
        !command
            .iter()
            .any(|item| item.starts_with("hooks.PostToolUseFailure="))
    );
    let state = command
        .iter()
        .find(|item| item.starts_with("hooks.state="))
        .expect("session hook trust state");
    assert!(state.contains("/<session-flags>/config.toml:session_start:0:0"));
    assert!(!state.contains("project"));
    assert!(!state.contains("plugin"));
    assert!(!command.contains(&"--dangerously-bypass-hook-trust".into()));
}

#[test]
fn inserts_all_overrides_before_the_prompt_tail() {
    let mut command = vec![
        "codex".into(),
        "--search".into(),
        "--".into(),
        "prompt".into(),
    ];
    append_overrides(
        &mut command,
        Path::new("/tmp/home"),
        Path::new("/tmp/cccc"),
        "g_test",
        "peer",
    );

    let separator = command
        .iter()
        .position(|item| item == "--")
        .expect("separator");
    assert_eq!(&command[separator..], ["--", "prompt"]);
    assert!(!command[..separator].contains(&"--dangerously-bypass-hook-trust".into()));
    assert!(
        command[..separator]
            .iter()
            .any(|item| item.starts_with("mcp_servers.cccc.command="))
    );
    assert!(
        command[..separator]
            .iter()
            .any(|item| item.starts_with("hooks.SessionStart="))
    );
}

#[test]
fn hook_hash_matches_codex_normalized_identity() {
    assert_eq!(
        hook_hash("user_prompt_submit", "/usr/bin/true"),
        "sha256:6990bafd84f554a7905347cfff30dc8ac278a24b17f343073271fc9737efd49f"
    );
}

#[test]
fn windows_hook_command_leaves_a_shell_safe_path_unquoted() {
    assert_eq!(
        hook_command_for_platform(
            Path::new(r"C:\project\cccc\target\release\cccc.exe"),
            "codex-state",
            true,
        ),
        r"C:\project\cccc\target\release\cccc.exe hook codex-state"
    );
}

#[test]
fn windows_hook_command_quotes_spaces_and_cmd_metacharacters() {
    assert_eq!(
        hook_command_for_platform(
            Path::new(r"C:\Program Files\CCCC\cccc.exe"),
            "codex-state",
            true,
        ),
        r#""C:\Program Files\CCCC\cccc.exe" hook codex-state"#
    );
    assert_eq!(
        hook_command_for_platform(
            Path::new(r"C:\project&tools\cccc.exe"),
            "claude-state",
            true,
        ),
        r#""C:\project&tools\cccc.exe" hook claude-state"#
    );
}

#[test]
fn unix_hook_command_keeps_single_quote_escaping() {
    assert_eq!(
        hook_command_for_platform(Path::new("/tmp/cccc's bin/cccc"), "codex-state", false),
        r#"'/tmp/cccc'"'"'s bin/cccc' hook codex-state"#
    );
}

#[test]
fn public_launcher_override_requires_an_absolute_cccc_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let launcher = temp
        .path()
        .join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    let private = temp.path().join(if cfg!(windows) {
        "cccc-rust.exe"
    } else {
        "cccc-rust"
    });
    fs::write(&launcher, b"launcher").expect("write launcher");
    fs::write(&private, b"private").expect("write private");

    assert!(valid_public_launcher(&launcher));
    assert!(!valid_public_launcher(&private));
    assert!(!valid_public_launcher(Path::new("cccc")));
}

#[test]
fn only_interactive_codex_commands_are_hook_eligible() {
    assert!(is_direct_codex_command(&[
        "codex".into(),
        "--search".into()
    ]));
    assert!(is_direct_codex_command(&[
        "codex".into(),
        "resume".into(),
        "session".into(),
    ]));
    assert!(!is_direct_codex_command(&[
        "codex".into(),
        "app-server".into(),
    ]));
    assert!(!is_direct_codex_command(&[
        "wrapper".into(),
        "codex".into()
    ]));
}

#[test]
fn prepends_binary_directory_without_duplicate() {
    let mut env = BTreeMap::from([("PATH".into(), "/usr/bin:/tmp/bin".into())]);
    prepend_executable_dir(&mut env, Path::new("/tmp/bin/cccc"));
    let paths = std::env::split_paths(env.get("PATH").expect("path")).collect::<Vec<_>>();
    assert_eq!(
        paths.first().map(std::path::PathBuf::as_path),
        Some(Path::new("/tmp/bin"))
    );
    assert_eq!(
        paths
            .iter()
            .filter(|path| *path == Path::new("/tmp/bin"))
            .count(),
        1
    );
}

#[test]
fn resolves_relative_path_entries_against_the_daemon_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("create bin");
    let executable = bin.join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    fs::write(&executable, b"launcher").expect("write launcher");
    let paths = std::env::join_paths([Path::new("bin")]).expect("relative PATH");

    assert_eq!(
        resolve_on_path(&paths, Some(temp.path())).as_deref(),
        Some(executable.as_path())
    );
}

#[test]
fn resolves_absolute_path_entries_without_a_daemon_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = temp
        .path()
        .join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    fs::write(&executable, b"launcher").expect("write launcher");
    let paths = std::env::join_paths([temp.path()]).expect("absolute PATH");

    assert_eq!(
        resolve_on_path(&paths, None).as_deref(),
        Some(executable.as_path())
    );
}

#[test]
fn each_hook_launch_rotates_the_environment_fence_atomically() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let mut env = BTreeMap::new();
    let first =
        begin_hook_launch(&home, "codex", "g_test", "peer1", &mut env).expect("first launch");
    let first_state =
        cccc_core::codex_hook_state::read(&home, "g_test", "peer1").expect("first state");
    assert_eq!(env["CCCC_HOOK_LAUNCH_TOKEN"], first);
    assert_eq!(first_state.launch_token, first);
    assert!(first_state.awaiting_session_start);

    let second =
        begin_hook_launch(&home, "codex", "g_test", "peer1", &mut env).expect("second launch");
    let second_state =
        cccc_core::codex_hook_state::read(&home, "g_test", "peer1").expect("second state");
    assert_ne!(first, second);
    assert_eq!(env["CCCC_HOOK_LAUNCH_TOKEN"], second);
    assert_eq!(second_state.launch_token, second);

    let mut claude_env = BTreeMap::new();
    let claude = begin_hook_launch(&home, "claude", "g_test", "claude-peer", &mut claude_env)
        .expect("claude launch");
    let claude_state =
        cccc_core::codex_hook_state::read_runtime(&home, "claude", "g_test", "claude-peer")
            .expect("claude state");
    assert_eq!(claude_env["CCCC_HOOK_LAUNCH_TOKEN"], claude);
    assert_eq!(claude_state.launch_token, claude);
    assert_eq!(claude_state.observation, "pty_fail_closed");
}

#[test]
fn missing_codex_executable_records_a_specific_setup_issue() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let mut command = vec!["codex".into()];
    let mut env = BTreeMap::new();
    let setup = configure_with_executable(
        CodexLaunch {
            home: &home,
            group_id: "g_test",
            actor_id: "peer1",
            cwd: temp.path(),
        },
        &mut command,
        &mut env,
        None,
        |_, _, _, _| true,
    )
    .expect("fail-closed setup");

    assert_eq!(command, ["codex"]);
    assert!(!setup.hook_enabled);
    assert!(!env["CCCC_HOOK_LAUNCH_TOKEN"].is_empty());
    let state = cccc_core::codex_hook_state::read(&home, "g_test", "peer1").expect("hook issue");
    assert_eq!(state.event, "HookUnavailableExecutable");
    assert!(state.awaiting_session_start);
}

#[test]
fn unsupported_hook_config_keeps_mcp_without_injecting_hooks() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let executable = temp
        .path()
        .join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    fs::write(&executable, b"launcher").expect("write launcher");
    let mut command = vec!["codex".into(), "--search".into()];
    let mut env = BTreeMap::new();

    let setup = configure_with_executable(
        CodexLaunch {
            home: &home,
            group_id: "g_test",
            actor_id: "peer1",
            cwd: temp.path(),
        },
        &mut command,
        &mut env,
        Some(executable),
        |_, _, _, _| false,
    )
    .expect("safe fallback");

    assert!(
        command
            .iter()
            .any(|item| item.starts_with("mcp_servers.cccc.command="))
    );
    assert!(
        !command
            .iter()
            .any(|item| item.starts_with("hooks.SessionStart="))
    );
    assert_eq!(env["CCCC_HOME"], temp.path().to_string_lossy());
    assert!(!setup.hook_enabled);
    let state = cccc_core::codex_hook_state::read(&home, "g_test", "peer1").expect("hook issue");
    assert_eq!(state.event, "HookUnavailableSettings");
}

#[test]
fn setup_issue_write_failures_propagate_for_both_runtimes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    for runtime in ["codex", "claude"] {
        let actor_id = format!("{runtime}-peer");
        let mut env = BTreeMap::new();
        let token = begin_hook_launch(&home, runtime, "g_test", &actor_id, &mut env)
            .expect("initial pending");
        let state_dir = home.daemon_dir().join(format!("{runtime}-hook-state"));
        fs::remove_dir_all(&state_dir).expect("remove state directory");
        fs::write(&state_dir, "blocks directory recreation").expect("blocking file");

        assert!(
            record_launch_issue(
                &home,
                runtime,
                "g_test",
                &actor_id,
                &token,
                "HookUnavailableExecutable"
            )
            .is_err(),
            "{runtime} setup issue write must fail"
        );
    }
}
