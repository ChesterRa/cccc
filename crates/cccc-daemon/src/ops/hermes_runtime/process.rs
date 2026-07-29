use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn run(
    executable: &Path,
    argv: &[String],
    cwd: Option<&Path>,
    input: Option<&str>,
    env: &[(&str, String)],
    timeout: Duration,
) -> io::Result<Value> {
    let mut command = Command::new(executable);
    command
        .args(argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    if let Some(home) = std::env::var_os("HERMES_HOME") {
        command.env("HERMES_HOME", home);
    }
    configure_process_group(&mut command);
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take()
        && let Some(input) = input
    {
        stdin.write_all(input.as_bytes())?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Hermes stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Hermes stderr unavailable"))?;
    let stdout_reader = std::thread::spawn(move || read_all(stdout));
    let stderr_reader = std::thread::spawn(move || read_all(stderr));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_process_group(&mut child);
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "Hermes command timed out after {} seconds",
                    timeout.as_secs()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| io::Error::other("Hermes stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| io::Error::other("Hermes stderr reader panicked"))??;
    Ok(json!({
        "returncode":status.code().unwrap_or(-1),
        "stdout":String::from_utf8_lossy(&stdout),
        "stderr":String::from_utf8_lossy(&stderr)
    }))
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_process_group(child: &mut std::process::Child) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    if let Ok(group_id) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(group_id), Signal::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn terminate_process_group(child: &mut std::process::Child) {
    let _ = child.kill();
}

fn read_all(mut stream: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

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
