//! Exercise diagnostic capture in a real, isolated daemon process.
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(mut ready: impl FnMut() -> bool, child: &mut Child, what: &str) {
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        if ready() {
            return;
        }
        assert!(
            child.try_wait().expect("poll daemon").is_none(),
            "daemon exited while waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn fixture_command(root: &Path, home: &Path) -> Command {
    let user = root.join("user");
    fs::create_dir_all(&user).expect("user home");
    #[cfg(windows)]
    for leaf in ["Roaming", "Local"] {
        fs::create_dir_all(user.join("AppData").join(leaf)).expect("Windows known folder");
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_cccc"));
    command.env_clear().current_dir(root);
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "TMP",
        "TEMP",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("HOME", &user)
        .env("USERPROFILE", &user)
        .env("CCCC_HOME", home);
    for key in [
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "PI_CODING_AGENT_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ] {
        let path = root.join(key.to_ascii_lowercase());
        fs::create_dir_all(&path).expect("isolated provider home");
        command.env(key, path);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[test]
fn daemon_records_safe_failures_without_stopping() {
    let root = tempfile::tempdir().expect("isolated fixture");
    let home = root.path().join("cccc");
    let mut command = fixture_command(root.path(), &home);
    command.args(["daemon", "run"]);
    let mut child = OwnedChild(command.spawn().expect("start fixture daemon"));
    wait_for(
        || home.join("daemon/ccccd.addr.json").is_file(),
        &mut child.0,
        "daemon readiness",
    );
    let log = home.join("daemon/ccccd-diagnostics.log");
    wait_for(|| log.is_file(), &mut child.0, "diagnostic startup marker");
    let mut duplicate = OwnedChild(
        fixture_command(root.path(), &home)
            .args(["daemon", "run"])
            .spawn()
            .expect("start competing daemon"),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = duplicate.0.try_wait().expect("competing daemon state") {
            break status;
        }
        assert!(Instant::now() < deadline, "competing daemon did not exit");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        !status.success(),
        "only the actual daemon owner may initialize diagnostics"
    );
    let starts = fs::read_to_string(&log)
        .expect("diagnostics")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["kind"] == "start")
        .count();
    assert_eq!(starts, 1);
    // The malformed registry deliberately contains a sentinel that must not
    // appear in diagnostics, even when its parse error is reported by tracing.
    let secret = "private_fixture_token_must_not_be_logged";
    fs::write(home.join("registry.json"), format!("{{{secret}")).expect("invalid fixture registry");
    wait_for(
        || contains_action(&log, "automation_group_discovery"),
        &mut child.0,
        "automation diagnostic",
    );
    let text = fs::read_to_string(&log).expect("diagnostics");
    assert!(!text.contains(secret));
    let records: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON line"))
        .collect();
    assert!(records.iter().any(|record| {
        record["kind"] == "start"
            && record["version"] == env!("CARGO_PKG_VERSION")
            && record["source_id"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
    }));
    assert!(
        records
            .iter()
            .any(|record| record["action"] == "automation_group_discovery"
                && record["code"] == "invalid_data")
    );
    assert!(child.0.try_wait().expect("daemon state").is_none());
}

#[test]
fn unavailable_diagnostics_do_not_prevent_daemon_start() {
    let root = tempfile::tempdir().expect("isolated fixture");
    let home = root.path().join("cccc");
    fs::create_dir_all(home.join("daemon/ccccd-diagnostics.log")).expect("unwritable log path");
    fs::create_dir_all(home.join("groups")).expect("groups directory");
    fs::write(home.join(".cccc-rust-v1"), "fixture").expect("home marker");
    let mut child = OwnedChild(
        fixture_command(root.path(), &home)
            .args(["daemon", "run"])
            .spawn()
            .expect("start fixture daemon"),
    );
    wait_for(
        || home.join("daemon/ccccd.addr.json").is_file(),
        &mut child.0,
        "daemon readiness",
    );
    assert!(child.0.try_wait().expect("daemon state").is_none());
}

#[cfg(unix)]
#[test]
fn closed_stderr_does_not_turn_diagnostic_failure_into_a_daemon_failure() {
    let root = tempfile::tempdir().expect("isolated fixture");
    let home = root.path().join("cccc");
    fs::create_dir_all(home.join("daemon/ccccd-diagnostics.log")).expect("unwritable log path");
    fs::create_dir_all(home.join("groups")).expect("groups directory");
    fs::write(home.join(".cccc-rust-v1"), "fixture").expect("home marker");
    let (reader, writer) = nix::unistd::pipe().expect("stderr pipe");
    drop(reader);
    let mut child = OwnedChild(
        fixture_command(root.path(), &home)
            .args(["daemon", "run"])
            .stderr(Stdio::from(writer))
            .spawn()
            .expect("start fixture daemon"),
    );
    wait_for(
        || home.join("daemon/ccccd.addr.json").is_file(),
        &mut child.0,
        "daemon readiness despite closed stderr",
    );
    assert!(child.0.try_wait().expect("daemon state").is_none());
}

#[cfg(unix)]
#[test]
fn embedded_daemon_uses_the_same_diagnostics() {
    let root = tempfile::tempdir().expect("isolated fixture");
    let home = root.path().join("cccc");
    let mut child = OwnedChild(
        fixture_command(root.path(), &home)
            .args(["--host", "127.0.0.1", "--port", "0", "web"])
            .spawn()
            .expect("start fixture Web host"),
    );
    wait_for(
        || home.join("daemon/ccccd.addr.json").is_file(),
        &mut child.0,
        "embedded daemon readiness",
    );
    let log = home.join("daemon/ccccd-diagnostics.log");
    wait_for(|| log.is_file(), &mut child.0, "diagnostic startup marker");
    let text = fs::read_to_string(log).expect("diagnostics");
    assert!(
        text.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|record| record["kind"] == "start")
    );
}

fn contains_action(path: &Path, action: &str) -> bool {
    fs::read_to_string(path).ok().is_some_and(|text| {
        text.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|record| record["action"] == action)
    })
}
