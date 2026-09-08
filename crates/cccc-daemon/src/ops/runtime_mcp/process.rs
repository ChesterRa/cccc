use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

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
    let inherited_path = std::env::var_os("PATH");
    let search_path = env
        .get("PATH")
        .map(OsStr::new)
        .or(inherited_path.as_deref());
    let program = cccc_core::runtime_mcp::resolve_program_in(program, search_path, cwd);
    let mut process = Command::new(program);
    process.args(args).current_dir(cwd).envs(env);
    let output = cccc_runtime::capture_command_blocking(&mut process, None, timeout, 2_000_000)?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MCP command output exceeds 2000000 bytes per stream",
        ));
    }
    Ok(Output {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn oversized_command_output_is_an_error_not_partial_setup_data() {
        let temp = tempfile::tempdir().expect("tempdir");
        let error = run(
            &[
                "/bin/sh".into(),
                "-c".into(),
                "head -c 2000001 /dev/zero >&2".into(),
            ],
            temp.path(),
            &BTreeMap::new(),
            Duration::from_secs(3),
        )
        .expect_err("oversized output");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn leader_exit_does_not_wait_for_inherited_output_pipes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Instant::now();
        let output = run(
            &[
                "/bin/sh".into(),
                "-c".into(),
                "sleep 2 & printf parent".into(),
            ],
            temp.path(),
            &BTreeMap::new(),
            Duration::from_millis(100),
        )
        .expect("parent completes");
        assert_eq!(output.stdout, "parent");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "inherited output outlived the command deadline"
        );
    }

    #[test]
    fn resolves_relative_path_entries_from_the_child_working_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        let executable = bin.join("runtime-mcp-fixture");
        std::fs::write(&executable, b"#!/bin/sh\nprintf child-cwd\n").expect("fixture");
        let mut permissions = executable.metadata().expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("permissions");
        let env = BTreeMap::from([("PATH".into(), "bin".into())]);

        let output = run(
            &["runtime-mcp-fixture".into()],
            temp.path(),
            &env,
            super::super::CHECK_TIMEOUT,
        )
        .expect("run fixture");

        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "child-cwd");
    }

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
