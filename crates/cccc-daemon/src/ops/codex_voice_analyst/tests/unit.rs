use super::super::*;
use super::super::{launch, launch_command, process};

#[test]
fn workspace_binding_requires_an_existing_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("analyst-workdir");
    std::fs::create_dir_all(&root).expect("root");

    let binding = launch::bind_workspace(&root).expect("binding");
    assert_eq!(binding.root, root.canonicalize().expect("canonical root"));
    assert!(launch::bind_workspace(&temp.path().join("missing")).is_err());
}

#[test]
fn default_launch_uses_one_effective_prefix_for_app_server_and_remote_tui() {
    let executable = std::env::current_exe().expect("test executable");
    let prepared = launch_command::prepare(
        &[executable.to_string_lossy().into_owned()],
        &BTreeMap::new(),
    )
    .expect("command");

    assert_eq!(
        &prepared.app_server[..prepared.remote_tui_prefix.len()],
        prepared.remote_tui_prefix.as_slice()
    );
    assert_eq!(
        &prepared.app_server[prepared.remote_tui_prefix.len()..],
        ["app-server", "--listen", "ws://127.0.0.1:0"]
    );
    assert!(
        prepared
            .remote_tui_prefix
            .windows(2)
            .any(|pair| pair == ["-c", "web_search=\"live\""])
    );
}

#[test]
fn app_server_launch_matches_the_codex_actor_yolo_policy() {
    let executable = std::env::current_exe().expect("test executable");
    let configured = vec![
        executable.to_string_lossy().into_owned(),
        "-c".into(),
        "shell_environment_policy.inherit=all".into(),
        "--dangerously-bypass-approvals-and-sandbox".into(),
        "--search".into(),
        "--profile".into(),
        "voice".into(),
        "-c".into(),
        "model=\"gpt-5.6-sol\"".into(),
    ];
    let prepared = launch_command::prepare(&configured, &BTreeMap::new()).expect("command");
    assert_eq!(
        prepared.remote_tui_prefix,
        vec![
            executable.to_string_lossy().as_ref(),
            "--search",
            "--profile",
            "voice",
            "-c",
            "model=\"gpt-5.6-sol\"",
            "--dangerously-bypass-approvals-and-sandbox",
            "-c",
            "shell_environment_policy.inherit=all",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
        ]
    );
    assert_eq!(
        prepared.app_server,
        vec![
            executable.to_string_lossy().as_ref(),
            "--search",
            "--profile",
            "voice",
            "-c",
            "model=\"gpt-5.6-sol\"",
            "--dangerously-bypass-approvals-and-sandbox",
            "-c",
            "shell_environment_policy.inherit=all",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
            "app-server",
            "--listen",
            "ws://127.0.0.1:0",
        ]
    );
}

#[test]
fn app_server_rejects_actor_commands_with_a_subcommand_or_prompt() {
    let executable = std::env::current_exe().expect("test executable");
    for trailing in ["resume", "inspect this repository"] {
        assert!(
            launch_command::prepare(
                &[executable.to_string_lossy().into_owned(), trailing.into()],
                &BTreeMap::new(),
            )
            .is_err()
        );
    }
}

#[test]
fn app_server_replaces_actor_host_policy_but_preserves_user_model_options() {
    let executable = std::env::current_exe().expect("test executable");
    let configured = vec![
        executable.to_string_lossy().into_owned(),
        "--model".into(),
        "gpt-test".into(),
        "-c".into(),
        "approval_policy=\"on-request\"".into(),
        "-c".into(),
        "mcp_servers.cccc.enabled=false".into(),
        "--sandbox".into(),
        "read-only".into(),
    ];
    let prepared = launch_command::prepare(&configured, &BTreeMap::new()).expect("command");
    let command = &prepared.app_server;
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--model", "gpt-test"])
    );
    assert!(!command.iter().any(|value| value == "on-request"));
    assert!(
        !command
            .iter()
            .any(|value| value == "mcp_servers.cccc.enabled=false")
    );
    assert!(!command.iter().any(|value| value == "read-only"));
    assert!(
        command
            .iter()
            .any(|value| value == "approval_policy=\"never\"")
    );
    assert!(
        command
            .iter()
            .any(|value| value == "sandbox_mode=\"danger-full-access\"")
    );
    assert_eq!(
        prepared.remote_tui_prefix,
        vec![
            executable.to_string_lossy().as_ref(),
            "--model",
            "gpt-test",
            "-c",
            "web_search=\"live\"",
            "--dangerously-bypass-approvals-and-sandbox",
            "-c",
            "shell_environment_policy.inherit=all",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
        ]
    );
}

#[test]
fn app_server_and_remote_tui_preserve_custom_codex_provider_configuration() {
    let executable = std::env::current_exe().expect("test executable");
    let configured = vec![
        executable.to_string_lossy().into_owned(),
        "-c".into(),
        "model_provider=\"ZAI\"".into(),
        "--model".into(),
        "glm-5.3".into(),
        "-c".into(),
        "model_providers.ZAI.env_key=\"ZAI_API_KEY\"".into(),
        "-c".into(),
        "web_search=\"disabled\"".into(),
    ];
    let prepared = launch_command::prepare(&configured, &BTreeMap::new()).expect("command");
    for expected in [
        ["-c", "model_provider=\"ZAI\""],
        ["--model", "glm-5.3"],
        ["-c", "model_providers.ZAI.env_key=\"ZAI_API_KEY\""],
        ["-c", "web_search=\"disabled\""],
    ] {
        assert!(prepared.app_server.windows(2).any(|pair| pair == expected));
        assert!(
            prepared
                .remote_tui_prefix
                .windows(2)
                .any(|pair| pair == expected)
        );
    }
    assert_eq!(
        prepared
            .app_server
            .iter()
            .filter(|argument| argument.as_str() == "web_search=\"disabled\"")
            .count(),
        1
    );
    assert_eq!(
        prepared
            .remote_tui_prefix
            .iter()
            .filter(|argument| argument.as_str() == "web_search=\"disabled\"")
            .count(),
        1
    );
    assert!(
        !prepared
            .remote_tui_prefix
            .iter()
            .any(|argument| argument == "app-server" || argument == "--listen")
    );
    assert!(
        !prepared
            .remote_tui_prefix
            .iter()
            .any(|argument| argument.starts_with("mcp_servers.cccc"))
    );
}

#[test]
fn app_server_endpoint_must_be_an_ip_loopback_websocket() {
    assert_eq!(
        process::parse_listening_endpoint("  listening on: ws://127.0.0.1:43123  ").as_deref(),
        Some("ws://127.0.0.1:43123")
    );
    assert!(process::validate_loopback_endpoint("ws://127.0.0.1:43123").is_ok());
    assert!(process::validate_loopback_endpoint("ws://[::1]:43123").is_ok());
    assert!(process::validate_loopback_endpoint("ws://0.0.0.0:43123").is_err());
    assert!(process::validate_loopback_endpoint("wss://127.0.0.1:43123").is_err());
    assert!(process::validate_loopback_endpoint("ws://localhost:43123").is_err());
    assert_eq!(ElicitationAction::Decline.as_str(), "decline");
}
