use super::*;
use cccc_contracts::ActorRuntime;

#[test]
fn public_launcher_override_must_be_an_existing_absolute_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let current = temp.path().join("cccc-rust");
    let launcher = temp.path().join("cccc");
    std::fs::write(&launcher, b"launcher").expect("write launcher");

    assert_eq!(
        select_public_executable(current.clone(), Some(launcher.clone())),
        launcher
    );
    assert_eq!(
        select_public_executable(current.clone(), Some(PathBuf::from("cccc"))),
        current
    );
    let other = temp.path().join("other");
    std::fs::write(&other, b"other").expect("write other");
    assert_eq!(
        select_public_executable(current.clone(), Some(other)),
        current
    );
}

#[test]
fn builds_codex_command_with_compiled_binary() {
    assert_eq!(
        add_command("codex", Path::new("/opt/cccc")).expect("command"),
        ["codex", "mcp", "add", "cccc", "--", "/opt/cccc", "mcp"]
    );
}

#[test]
fn antigravity_uses_native_registration_with_inherited_launcher() {
    assert_eq!(
        add_command("antigravity", Path::new("/opt/cccc")).expect("command"),
        ["agy", "mcp", "add", "cccc", "cccc", "mcp"]
    );
}

#[test]
fn manual_runtime_has_explicit_batch_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let args = SetupArgs {
        runtime: None,
        path: ".".into(),
        runtime_mode: "default".into(),
        login: false,
    };
    let value = setup_one(
        &home,
        &args,
        "custom",
        Path::new("/opt/cccc"),
        &json!({"mcpServers":{}}),
    )
    .expect("manual setup");
    assert_eq!(value["status"], "requires_action");
    assert_eq!(value["mode"], "manual");
}

#[test]
fn batch_count_includes_only_written_or_registered_configuration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let args = SetupArgs {
        runtime: None,
        path: ".".into(),
        runtime_mode: "default".into(),
        login: false,
    };
    let config = json!({"mcpServers":{}});
    let setup =
        |runtime| setup_one(&home, &args, runtime, Path::new("/opt/cccc"), &config).expect("setup");
    let results = [
        // CCCC injects these servers into each managed session: nothing to
        // configure, even when the runtime's CLI is not installed.
        setup("claude"),
        setup("pi"),
        hermes_setup(),
        json!({"runtime":"deepseek","mode":"managed_session","status":"ready","managed":true,"mcp":"injected_per_session"}),
        // Nothing configured: a manual runtime and a missing CLI.
        setup("custom"),
        json!({"runtime":"kimi","status":"unavailable","error":"kimi CLI not found"}),
        // Configuration setup wrote or found registered.
        json!({"runtime":"cline","mode":"auto","status":"added"}),
        json!({"runtime":"cursor","mode":"auto","status":"ready","path":"/home/u/.cursor/mcp.json"}),
        json!({"runtime":"grok","mode":"managed_session","status":"ready","managed":true,"mcp":"native_registry"}),
    ];
    assert_eq!(configured_count(&results), 3);
}

#[test]
fn builds_noninteractive_cline_command_with_compiled_binary() {
    assert_eq!(
        add_command("cline", Path::new("/opt/cccc")).expect("command"),
        [
            "cline",
            "mcp",
            "add",
            "cccc",
            "--yes",
            "--",
            "/opt/cccc",
            "mcp"
        ]
    );
}

fn assert_managed_setup(runtime: &str, actor_runtime: ActorRuntime) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let args = SetupArgs {
        runtime: Some(runtime.into()),
        path: ".".into(),
        runtime_mode: "default".into(),
        login: false,
    };
    let value = setup_one(
        &home,
        &args,
        runtime,
        Path::new("/opt/cccc"),
        &json!({"mcpServers":{}}),
    )
    .expect("managed setup");

    assert_eq!(value["status"], "ready");
    assert_eq!(value["managed"], true);
    assert_eq!(value["mode"], "managed_session");
    assert_eq!(value["mcp"], "injected_per_session");
    assert!(cccc_core::runtime_mcp::add_command(actor_runtime, Path::new("/opt/cccc")).is_none());
    assert!(cccc_core::runtime_mcp::remove_command(actor_runtime).is_none());
}

#[test]
fn claude_setup_reports_session_owned_mcp_without_global_config_mutation() {
    assert_managed_setup("claude", ActorRuntime::Claude);
}

#[test]
fn opencode_setup_reports_session_owned_mcp_without_global_config_mutation() {
    assert_managed_setup("opencode", ActorRuntime::Opencode);
}

#[test]
fn pi_setup_reports_session_owned_mcp_without_global_config_mutation() {
    assert_managed_setup("pi", ActorRuntime::Pi);
}

#[test]
fn deepseek_ready_setup_reports_managed_session_mode() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let value = deepseek_setup_json(
        &home,
        Ok(cccc_daemon::deepseek_setup::DeepSeekSetupOutcome {
            dsh_home: temp.path().join("dsh"),
            profile: temp.path().join("profile.json"),
            packages_installed: false,
            profile_created: false,
        }),
    );
    assert_eq!(value["runtime"], "deepseek");
    assert_eq!(value["mode"], "managed_session");
    assert_eq!(value["status"], "ready");
    assert_eq!(value["managed"], true);
    assert_eq!(value["mcp"], "injected_per_session");
}
