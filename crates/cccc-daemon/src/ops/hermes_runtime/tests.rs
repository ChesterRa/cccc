use super::*;
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

/// The entry earlier CCCC versions wrote, for the CCCC this fixture runs.
fn cccc_entry(root: &Path) -> Value {
    json!({
        "command":root.join("bin/cccc"),"args":["mcp"],"enabled":true,
        "env":{
            "CCCC_HOME":"${CCCC_HOME}",
            "CCCC_GROUP_ID":"${CCCC_GROUP_ID}",
            "CCCC_ACTOR_ID":"${CCCC_ACTOR_ID}"
        }
    })
}

fn seed(root: &Path, profile: Option<&str>, cccc: Value) -> PathBuf {
    let directory = match profile {
        Some(profile) => root.join("hermes-home/profiles").join(profile),
        None => root.join("hermes-home"),
    };
    std::fs::create_dir_all(&directory).expect("fixture Hermes profile");
    let config = json!({
        "model":{"provider":"owned-fixture","model":"preserve-model"},
        "mcp_servers":{
            "cccc":cccc,
            "unrelated":{"command":"/unrelated/server","args":["preserve"],"enabled":false}
        }
    });
    let path = directory.join("config.yaml");
    std::fs::write(&path, serde_json::to_vec(&config).expect("config")).expect("seed config");
    path
}

fn stored(path: &Path) -> Value {
    cccc_core::fs::read_yaml(path).expect("read resulting fixture configuration")
}

fn calls(root: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(root.join("hermes-calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).expect("fixture call"))
        .collect()
}

fn member(command: &[&str]) -> Vec<String> {
    command.iter().map(|arg| (*arg).to_owned()).collect()
}
const GET: [&str; 4] = ["config", "get", "mcp_servers.cccc", "--json"];
const UNSET: [&str; 3] = ["config", "unset", "mcp_servers.cccc"];

/// A Hermes call bound to `profile`.
fn pinned(profile: &str, args: &[&str]) -> Vec<String> {
    let mut call = member(&["--profile", profile]);
    call.extend(member(args));
    call
}

fn removals(root: &Path) -> usize {
    calls(root)
        .iter()
        .filter(|call| call.contains(&"unset".to_owned()))
        .count()
}

fn start(home: &HomeLayout, root: &Path, command: &[&str]) -> Result<Vec<String>, OpError> {
    prepare_member_start(home, &member(command), root, &BTreeMap::new())
}

#[test]
fn member_start_removes_only_the_entry_cccc_wrote() {
    isolated(
        "member_start_removes_only_the_entry_cccc_wrote",
        |home, root| {
            let path = seed(root, None, cccc_entry(root));
            let launched =
                start(home, root, &["hermes", "--tui"]).expect("the entry CCCC wrote is removed");
            assert_eq!(
                launched,
                member(&["hermes", "--profile", "default", "--tui"])
            );
            let config = stored(&path);
            assert!(config["mcp_servers"].get("cccc").is_none());
            assert_eq!(
                config["mcp_servers"]["unrelated"]["args"],
                json!(["preserve"])
            );
            assert_eq!(config["model"]["model"], "preserve-model");
            // Without `-p`, Hermes rereads its sticky profile on every command:
            // everything after the first call names the profile it selected.
            assert_eq!(
                calls(root),
                [
                    member(&["config", "path"]),
                    pinned("default", &["config", "path"]),
                    pinned("default", &GET),
                    pinned("default", &UNSET),
                    pinned("default", &GET),
                ]
            );
            let again = start(home, root, &["hermes", "--tui"]).expect("nothing left to remove");
            assert_eq!(again, launched);
            // With nothing to remove: the profile, then one question about the entry.
            assert_eq!(calls(root).len(), 8);
        },
    );
}

#[test]
fn member_start_refuses_any_other_cccc_entry() {
    isolated("member_start_refuses_any_other_cccc_entry", |home, root| {
        let mut fixed_identity = cccc_entry(root);
        fixed_identity["env"]["CCCC_ACTOR_ID"] = json!("peer1");
        let mut old_path = cccc_entry(root);
        old_path["command"] = json!("/old/cccc");
        let mut extra_field = cccc_entry(root);
        extra_field["timeout"] = json!(30);
        let mut disabled = cccc_entry(root);
        disabled["enabled"] = json!(false);
        for entry in [fixed_identity, old_path, extra_field, disabled] {
            let path = seed(root, None, entry.clone());
            let before = std::fs::read(&path).expect("seeded config");
            let error = start(home, root, &["hermes"])
                .expect_err("an entry CCCC did not write blocks the start");
            assert_eq!(error.code, "hermes_mcp_entry_conflict", "{entry}");
            assert!(
                error.message.contains("cannot confirm who wrote it")
                    && error
                        .message
                        .contains("`hermes --profile default config unset mcp_servers.cccc`"),
                "{}",
                error.message
            );
            assert_eq!(
                std::fs::read(&path).expect("kept config"),
                before,
                "{entry}"
            );
        }
        assert_eq!(removals(root), 0);
    });
}

#[test]
fn a_config_that_does_not_parse_blocks_only_while_hermes_reads_an_entry() {
    isolated(
        "a_config_that_does_not_parse_blocks_only_while_hermes_reads_an_entry",
        |home, root| {
            let directory = root.join("hermes-home");
            std::fs::create_dir_all(&directory).expect("profile");
            let path = directory.join("config.yaml");
            std::fs::write(&path, "mcp_servers: [unclosed\n").expect("broken config");
            // Hermes keeps serving its last good copy of a file that breaks.
            let good = directory.join("last-good.json");
            let copy = json!({"mcp_servers":{"cccc":cccc_entry(root)}});
            std::fs::write(&good, serde_json::to_vec(&copy).expect("copy")).expect("good copy");
            let error =
                start(home, root, &["hermes"]).expect_err("CCCC cannot read the entry Hermes uses");
            assert_eq!(error.code, "hermes_config_invalid");
            std::fs::remove_file(&good).expect("no good copy");
            start(home, root, &["hermes"]).expect("without a good copy Hermes reads no entry");
            assert_eq!(
                std::fs::read_to_string(&path).expect("kept config"),
                "mcp_servers: [unclosed\n"
            );
            assert_eq!(removals(root), 0);
        },
    );
}

#[test]
fn member_profile_selects_the_config_that_is_cleaned() {
    isolated(
        "member_profile_selects_the_config_that_is_cleaned",
        |home, root| {
            let default = seed(root, None, cccc_entry(root));
            let work = seed(root, Some("work"), cccc_entry(root));
            let command = ["hermes", "-p", "work", "--tui"];
            let launched = start(home, root, &command).expect("profile entry removed");
            assert_eq!(launched, member(&command));
            start(home, root, &["hermes", "--profile=work"]).expect("profile entry gone");
            assert!(stored(&work)["mcp_servers"].get("cccc").is_none());
            assert!(stored(&default)["mcp_servers"].get("cccc").is_some());
            assert_eq!(
                calls(root),
                [
                    pinned("work", &GET),
                    pinned("work", &["config", "path"]),
                    pinned("work", &UNSET),
                    pinned("work", &GET),
                    pinned("work", &GET),
                ]
            );
            let error = start(home, root, &["hermes", "-p", "missing"])
                .expect_err("an unknown profile cannot be checked");
            assert_eq!(error.code, "hermes_profile_unavailable");
            assert!(
                error.message.contains("does not exist"),
                "{}",
                error.message
            );
        },
    );
}

#[test]
fn a_member_keeps_the_profile_hermes_selected_at_its_start() {
    isolated(
        "a_member_keeps_the_profile_hermes_selected_at_its_start",
        |home, root| {
            let default = seed(root, None, cccc_entry(root));
            let work = seed(root, Some("work"), cccc_entry(root));
            std::fs::write(root.join("hermes-home/active_profile"), "work").expect("sticky");
            // The user switches back to the default profile right after
            // Hermes reported the work profile's file.
            std::fs::write(root.join("switch-sticky-after-path"), "default").expect("switch");
            let launched = start(home, root, &["hermes"]).expect("work entry removed");
            // The ACP session, first setup and every reopened terminal take
            // their profile from this command.
            assert_eq!(launched, member(&["hermes", "--profile", "work"]));
            assert_eq!(
                crate::ops::codex_voice_analyst::hermes_profile(&launched).expect("profile"),
                Some("work".to_owned())
            );
            assert!(stored(&work)["mcp_servers"].get("cccc").is_none());
            assert!(stored(&default)["mcp_servers"].get("cccc").is_some());
            assert_eq!(
                calls(root)[1..],
                [
                    pinned("work", &["config", "path"]),
                    pinned("work", &GET),
                    pinned("work", &UNSET),
                    pinned("work", &GET),
                ]
            );
        },
    );
}

#[test]
fn a_hermes_home_naming_a_profile_directory_is_used_as_it_is() {
    isolated(
        "a_hermes_home_naming_a_profile_directory_is_used_as_it_is",
        |home, root| {
            let work = seed(root, Some("work"), cccc_entry(root));
            let env = BTreeMap::from([(
                "HERMES_HOME".to_owned(),
                root.join("hermes-home/profiles/work")
                    .to_string_lossy()
                    .into_owned(),
            )]);
            let launched = prepare_member_start(home, &member(&["hermes", "--tui"]), root, &env)
                .expect("work entry removed");
            assert_eq!(launched, member(&["hermes", "--tui"]));
            assert!(stored(&work)["mcp_servers"].get("cccc").is_none());
            assert_eq!(
                calls(root),
                [
                    member(&GET),
                    member(&["config", "path"]),
                    member(&UNSET),
                    member(&GET),
                ]
            );
        },
    );
}

#[test]
fn a_profile_that_does_not_resolve_to_the_selected_file_is_not_pinned() {
    isolated(
        "a_profile_that_does_not_resolve_to_the_selected_file_is_not_pinned",
        |home, root| {
            let path = seed(root, None, cccc_entry(root));
            let before = std::fs::read(&path).expect("seeded config");
            std::fs::write(root.join("pinned-path-differs"), b"").expect("other file");
            let error = start(home, root, &["hermes"]).expect_err("the profile cannot be pinned");
            assert_eq!(error.code, "hermes_profile_mismatch");
            assert_eq!(std::fs::read(&path).expect("kept config"), before);
            assert_eq!(calls(root).len(), 2);
        },
    );
}

#[test]
fn an_entry_hermes_reads_from_another_layer_is_reported_not_removed() {
    isolated(
        "an_entry_hermes_reads_from_another_layer_is_reported_not_removed",
        |home, root| {
            let directory = root.join("hermes-home");
            std::fs::create_dir_all(&directory).expect("profile");
            let path = directory.join("config.yaml");
            let own = json!({"mcp_servers":{"unrelated":{"command":"/unrelated/server"}}});
            std::fs::write(&path, serde_json::to_vec(&own).expect("config")).expect("config");
            // Even an exact copy of CCCC's entry is not CCCC's to remove there.
            let managed = json!({"mcp_servers":{"cccc":cccc_entry(root)}});
            std::fs::write(
                directory.join("managed.json"),
                serde_json::to_vec(&managed).expect("managed layer"),
            )
            .expect("managed layer");
            let error = start(home, root, &["hermes"])
                .expect_err("an entry from another layer blocks the start");
            assert_eq!(error.code, "hermes_mcp_entry_conflict");
            assert!(
                error.message.contains("not a plain entry")
                    && !error.message.contains("config unset"),
                "{}",
                error.message
            );
            assert_eq!(stored(&path), own);
            assert_eq!(removals(root), 0);
        },
    );
}

#[test]
fn removal_that_does_not_take_effect_fails_the_start() {
    isolated(
        "removal_that_does_not_take_effect_fails_the_start",
        |home, root| {
            std::fs::write(root.join("remove-ignored"), b"").expect("ignore removal");
            seed(root, None, cccc_entry(root));
            let error = start(home, root, &["hermes"]).expect_err("the entry is still there");
            assert_eq!(error.code, "hermes_mcp_cleanup_failed");
        },
    );
}

#[test]
fn ops_report_per_session_injection() {
    isolated("ops_report_per_session_injection", |home, root| {
        let path = seed(root, None, cccc_entry(root));
        let before = status(home).expect("status");
        assert_eq!(before["mcp"]["mode"], "injected_per_session");
        assert_eq!(before["mcp"]["persistent_entry"], "cccc_written");
        assert_eq!(before["config_path"], json!(path));
        assert_eq!(before["config_path_verified"], true);
        assert_eq!(before["launch_ready"], true);
        let request = DaemonRequest {
            v: 1,
            op: "runtime_hermes_prepare".into(),
            args: json!({"cwd":root}).as_object().expect("args").clone(),
        };
        let prepared = prepare(home, &request).expect("prepare");
        assert_eq!(prepared["cleanup"]["persistent_entry"], "removed");
        assert_eq!(prepared["status"]["mcp"]["persistent_entry"], "absent");
        let mut other = cccc_entry(root);
        other["command"] = json!("/old/cccc");
        seed(root, None, other);
        let blocked = status(home).expect("status");
        assert_eq!(blocked["launch_ready"], false);
        assert_eq!(blocked["mcp"]["persistent_entry"], "conflict");
        assert_eq!(blocked["issues"], json!(["hermes_mcp_entry_conflict"]));
        let tested = mcp_test(home, &request).expect("mcp test");
        assert_eq!(tested["applicable"], false);
    });
}

#[test]
fn status_reports_a_profile_hermes_cannot_resolve() {
    isolated(
        "status_reports_a_profile_hermes_cannot_resolve",
        |home, root| {
            let directory = root.join("hermes-home");
            std::fs::create_dir_all(&directory).expect("Hermes home");
            std::fs::write(directory.join("active_profile"), "gone").expect("stale profile");
            let reported = status(home).expect("status");
            assert_eq!(reported["launch_ready"], false);
            assert_eq!(reported["issues"], json!(["hermes_profile_unavailable"]));
            assert_eq!(reported["config_path_verified"], false);
            assert_eq!(
                reported["config_path"],
                json!(directory.join("config.yaml"))
            );
            assert_eq!(reported["mcp"]["persistent_entry"], "unknown");
            assert!(
                reported["mcp"]["error"]
                    .as_str()
                    .is_some_and(|error| error.contains("no longer exists")),
                "{reported:?}"
            );
            let error = start(home, root, &["hermes"]).expect_err("a member start fails too");
            assert_eq!(error.code, "hermes_profile_unavailable");
        },
    );
}
