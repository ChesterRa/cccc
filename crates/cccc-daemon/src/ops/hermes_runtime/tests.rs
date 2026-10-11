use super::*;
use cccc_core::fs::read_yaml;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const FIXTURE_ROOT: &str = "CCCC_HERMES_REGISTRATION_FIXTURE_ROOT";

fn isolated(test_name: &str, test: impl FnOnce(&HomeLayout, &Path)) {
    if let Some(root) = std::env::var_os(FIXTURE_ROOT).map(PathBuf::from) {
        let home = HomeLayout::from_path(root.join("cccc-home")).expect("fixture CCCC Home");
        home.initialize().expect("initialize isolated CCCC Home");
        test(&home, &root);
        return;
    }
    let temp = tempfile::tempdir().expect("isolated Hermes registration fixture");
    let root = temp.path();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("fake CLI directory");
    std::fs::create_dir_all(root.join("tmp")).expect("isolated temporary directory");
    for (name, body) in [
        ("hermes", include_str!("cli_fixture.py")),
        ("cccc", "#!/bin/sh\nexit 0\n"),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, body).expect("write owned fixture executable");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("make owned fixture executable");
    }
    let mut child = Command::new(std::env::current_exe().expect("current test executable"));
    child
        .args([
            "--exact",
            &format!("ops::hermes_runtime::tests::{test_name}"),
            "--nocapture",
        ])
        .env_clear()
        .env(FIXTURE_ROOT, root)
        .env("PATH", &bin)
        .env("CCCC_LAUNCHER_PATH", bin.join("cccc"))
        .env("TERM", "dumb")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", root.join("tmp"));
    for (key, directory) in [
        ("HOME", "provider-home"),
        ("CCCC_HOME", "cccc-home"),
        ("HERMES_HOME", "hermes-home"),
        ("CODEX_HOME", "codex-home"),
        ("CLAUDE_CONFIG_DIR", "claude-home"),
        ("GROK_HOME", "grok-home"),
        ("PI_CODING_AGENT_DIR", "pi-home"),
        ("XDG_CONFIG_HOME", "xdg-config"),
        ("XDG_DATA_HOME", "xdg-data"),
    ] {
        child.env(key, root.join(directory));
    }
    let output = child
        .output()
        .expect("run isolated Hermes registration test");
    assert!(
        output.status.success(),
        "isolated registration test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn seed_stale_config(root: &Path) -> Value {
    let initial = json!({
        "model":{"provider":"owned-fixture","model":"preserve-model"},
        "mcp_servers":{
            "cccc":{
                "command":"/old/cccc","args":["mcp"],"enabled":true,
                "env":{
                    "CCCC_HOME":"${CCCC_HOME}",
                    "CCCC_GROUP_ID":"${CCCC_GROUP_ID}",
                    "CCCC_ACTOR_ID":"${CCCC_ACTOR_ID}"
                }
            },
            "unrelated":{"command":"/unrelated/server","args":["preserve"],"enabled":false}
        }
    });
    let profile = root.join("hermes-home");
    std::fs::create_dir_all(&profile).expect("fixture Hermes profile");
    std::fs::write(
        profile.join("config.yaml"),
        serde_json::to_vec(&initial).expect("serialize synthetic config"),
    )
    .expect("seed stale synthetic MCP entry");
    initial
}

fn assert_unrelated_config_preserved(root: &Path, initial: &Value) {
    let stored: Value = read_yaml(&root.join("hermes-home/config.yaml"))
        .expect("read resulting fixture configuration");
    assert_eq!(stored["model"], initial["model"]);
    assert_eq!(
        stored["mcp_servers"]["unrelated"],
        initial["mcp_servers"]["unrelated"]
    );
}

fn prepare_request(root: &Path) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: "runtime_hermes_prepare".into(),
        args: json!({"yes":true,"cwd":root})
            .as_object()
            .expect("prepare arguments")
            .clone(),
    }
}

#[test]
fn actor_registration_updates_existing_stale_entry_and_preserves_other_config() {
    isolated(
        "actor_registration_updates_existing_stale_entry_and_preserves_other_config",
        |home, root| {
            let initial = seed_stale_config(root);
            ensure_for_actor(home, root, &BTreeMap::new())
                .expect("Actor registration must update and enable an existing stale entry");
            let config = load_config(&root.join("hermes-home/config.yaml"));
            let state = inspect_mcp(&config, &cccc_command());
            assert_eq!(state["status"], "ready");
            assert_unrelated_config_preserved(root, &initial);
        },
    );
}

#[test]
fn prepare_registration_updates_existing_stale_entry_and_preserves_other_config() {
    isolated(
        "prepare_registration_updates_existing_stale_entry_and_preserves_other_config",
        |home, root| {
            let initial = seed_stale_config(root);
            let prepared = prepare(home, &prepare_request(root))
                .expect("explicit prepare must update and enable an existing stale entry");
            assert_eq!(prepared["status"]["mcp"]["status"], "ready");
            assert_eq!(prepared["status"]["setup_ready"], true);
            assert_unrelated_config_preserved(root, &initial);
        },
    );
}

#[test]
fn failed_probe_saved_disabled_entry_is_rejected_by_actor_and_prepare() {
    isolated(
        "failed_probe_saved_disabled_entry_is_rejected_by_actor_and_prepare",
        |home, root| {
            std::fs::write(root.join("probe-fails"), b"owned failure")
                .expect("configure an explicit tool-discovery failure");
            let initial = seed_stale_config(root);
            let actor_error = ensure_for_actor(home, root, &BTreeMap::new())
                .expect_err("failed Actor tool discovery cannot count as ready");
            assert_eq!(actor_error.code, "runtime_mcp_verification_failed");
            assert_unrelated_config_preserved(root, &initial);

            let initial = seed_stale_config(root);
            let prepare_error = prepare(home, &prepare_request(root))
                .expect_err("failed explicit tool discovery cannot return success");
            assert_eq!(prepare_error.code, "runtime_mcp_verification_failed");
            assert_unrelated_config_preserved(root, &initial);
            assert_ne!(
                inspect_mcp(
                    &load_config(&root.join("hermes-home/config.yaml")),
                    &cccc_command()
                )["status"],
                "ready"
            );
        },
    );
}
