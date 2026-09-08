use serde_json::{Value, json};
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub(super) fn run(
    executable: &Path,
    argv: &[String],
    cwd: Option<&Path>,
    input: Option<&str>,
    env: &[(&str, String)],
    timeout: Duration,
) -> io::Result<Value> {
    let mut command = Command::new(executable);
    command.args(argv);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    let output = cccc_runtime::capture_command_blocking(
        &mut command,
        input.map(str::as_bytes),
        timeout,
        2_000_000,
    )?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Hermes command output exceeds 2000000 bytes per stream",
        ));
    }
    Ok(json!({
        "returncode":output.status.code().unwrap_or(-1),
        "stdout":String::from_utf8_lossy(&output.stdout),
        "stderr":String::from_utf8_lossy(&output.stderr)
    }))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn explicit_profile_environment_overrides_the_host_default() {
        const CHILD: &str = "CCCC_TEST_HERMES_ENV_PRECEDENCE";
        if std::env::var_os(CHILD).is_none() {
            let output = Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", "ops::hermes_runtime::process::tests::explicit_profile_environment_overrides_the_host_default", "--nocapture"])
                .env(CHILD, "1").env("HERMES_HOME", "host-profile")
                .output().expect("isolated environment fixture");
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let result = run(
            Path::new("/bin/sh"),
            &["-c".into(), "printf '%s' \"$HERMES_HOME\"".into()],
            None,
            None,
            &[("HERMES_HOME", "actor-profile".into())],
            Duration::from_secs(2),
        )
        .expect("profile command");
        assert_eq!(result["stdout"], "actor-profile");
        let inherited = run(
            Path::new("/bin/sh"),
            &["-c".into(), "printf '%s' \"$HERMES_HOME\"".into()],
            None,
            None,
            &[],
            Duration::from_secs(2),
        )
        .expect("inherited profile command");
        assert_eq!(inherited["stdout"], "host-profile");
    }

    #[test]
    fn oversized_command_output_is_an_error_not_partial_setup_data() {
        let error = run(
            Path::new("/bin/sh"),
            &["-c".into(), "head -c 2000001 /dev/zero".into()],
            None,
            None,
            &[],
            Duration::from_secs(3),
        )
        .expect_err("oversized output");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn timeout_includes_blocked_standard_input() {
        let started = Instant::now();
        let error = run(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 2".into()],
            None,
            Some(&"x".repeat(1_000_000)),
            &[],
            Duration::from_millis(50),
        )
        .expect_err("timeout");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn command_timeout_terminates_stalled_hermes_processes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let marker = temp.path().join("descendant-finished");
        let error = run(
            Path::new("/bin/sh"),
            &[
                "-c".into(),
                "(sleep 1; printf done > \"$CCCC_TIMEOUT_MARKER\") & wait".into(),
            ],
            None,
            None,
            &[("CCCC_TIMEOUT_MARKER", marker.to_string_lossy().into_owned())],
            Duration::from_millis(50),
        )
        .expect_err("timeout");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        std::thread::sleep(Duration::from_millis(1_200));
        assert!(
            !marker.exists(),
            "timed-out Hermes descendant was left running"
        );
    }
}
