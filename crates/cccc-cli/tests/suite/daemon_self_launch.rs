// Included by the crate-level integration test harness.
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

#[test]
fn installed_cli_starts_daemon_without_a_sibling_daemon_binary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir(&bin_dir).expect("create bin dir");

    let installed = bin_dir.join(executable_name("cccc"));
    std::fs::copy(env!("CARGO_BIN_EXE_cccc"), &installed).expect("copy cccc");

    // A decoy catches regressions to the old sibling `ccccd start` contract.
    let decoy = bin_dir.join(executable_name("ccccd"));
    std::fs::copy(env!("CARGO_BIN_EXE_cccc"), decoy).expect("copy decoy ccccd");

    let home = temp.path().join("home");
    let start = run(&installed, &home, &["daemon", "start"]);
    if !start.status.success() {
        panic!("daemon start failed: {}", detail(&start));
    }

    let status = run(&installed, &home, &["daemon", "status"]);
    let stop = run(&installed, &home, &["daemon", "stop"]);
    wait_for_daemon_exit(&home);

    assert!(
        status.status.success(),
        "daemon status failed: {}",
        detail(&status)
    );
    assert!(
        stop.status.success(),
        "daemon stop failed: {}",
        detail(&stop)
    );
}

fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

fn run(executable: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(executable)
        .args(args)
        .env("CCCC_HOME", home)
        .output()
        .expect("run installed cccc")
}

fn wait_for_daemon_exit(home: &Path) {
    let address = home.join("daemon").join("ccccd.addr.json");
    let deadline = Instant::now() + Duration::from_secs(10);
    while address.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !address.exists(),
        "daemon did not remove {}",
        address.display()
    );
}

fn detail(output: &Output) -> String {
    format!(
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}
