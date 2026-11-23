//! Each worker and connector has its own environment; no process-global mutations.
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

pub fn command(test: &str, home: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", test, "--nocapture"])
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env(
            "TMPDIR",
            if cfg!(target_os = "macos") {
                "/private/tmp"
            } else {
                "/tmp"
            },
        )
        .env("TEMP", home)
        .env("TMP", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

pub async fn run(mut command: Command, input: &[u8], private: &[&str]) {
    let mut child = command.spawn().expect("isolated test process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .await
        .expect("fixture input");
    let output = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .expect("test process deadline")
        .expect("test process output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for text in [&stdout, &stderr] {
        for value in private {
            assert!(
                value.is_empty() || !text.contains(value),
                "isolated child diagnostics contained a private value"
            );
        }
    }
    assert!(
        output.status.success(),
        "isolated case failed: {stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed;"),
        "the exact child test must actually run"
    );
}
