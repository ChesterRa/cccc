use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub(super) fn run(
    command: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> io::Result<Output> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty MCP command"))?;
    let mut process = Command::new(program);
    process
        .args(args)
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut process);
    let mut child = process.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("MCP command stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("MCP command stderr unavailable"))?;
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
                format!("MCP command timed out after {} seconds", timeout.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| io::Error::other("MCP stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| io::Error::other("MCP stderr reader panicked"))??;
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

fn read_all(mut stream: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    Ok(bytes)
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn timeout_terminates_descendants() {
        let temp = tempfile::tempdir().expect("tempdir");
        let marker = temp.path().join("descendant-finished");
        let env = BTreeMap::from([(
            "CCCC_MCP_TIMEOUT_MARKER".into(),
            marker.to_string_lossy().into_owned(),
        )]);
        let error = run(
            &[
                "/bin/sh".into(),
                "-c".into(),
                "(sleep 1; printf done > \"$CCCC_MCP_TIMEOUT_MARKER\") & wait".into(),
            ],
            temp.path(),
            &env,
            Duration::from_millis(50),
        )
        .expect_err("timeout");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        std::thread::sleep(Duration::from_millis(1_200));
        assert!(!marker.exists());
    }
}
