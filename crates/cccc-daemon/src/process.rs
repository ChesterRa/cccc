use crate::DaemonPaths;
use anyhow::{Context, Result};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::Map;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    AlreadyRunning,
    Started(u32),
}

pub struct DetachedDaemon {
    executable: PathBuf,
    run_args: Vec<OsString>,
}

impl DetachedDaemon {
    pub fn new<I, S>(executable: impl Into<PathBuf>, run_args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            executable: executable.into(),
            run_args: run_args.into_iter().map(Into::into).collect(),
        }
    }

    pub async fn start(&self, home: &HomeLayout) -> Result<StartOutcome> {
        Ok(match self.start_owned(home).await? {
            None => StartOutcome::AlreadyRunning,
            Some(child) => StartOutcome::Started(child.id()),
        })
    }

    /// Start a detached daemon while retaining the operating-system process
    /// handle. Owners that must later stop exactly the process they created
    /// should use this instead of relying on a reusable PID.
    pub async fn start_owned(&self, home: &HomeLayout) -> Result<Option<Child>> {
        home.initialize()?;
        if ping(home).await {
            return Ok(None);
        }

        let paths = DaemonPaths::new(home.clone());
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&paths.log)?;
        let error_log = log.try_clone()?;
        let mut command = self.command(home);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .current_dir(home.root())
            .spawn()
            .with_context(|| format!("spawn Rust daemon via {}", self.executable.display()))?;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if ping(home).await {
                return Ok(Some(child));
            }
            if let Some(status) = child.try_wait().context("poll Rust daemon process")? {
                anyhow::bail!(
                    "Rust daemon exited before becoming ready with {status}; see {}",
                    paths.log.display()
                );
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let _ = child.kill();
        let _ = child.wait();
        anyhow::bail!(
            "Rust daemon failed to become ready; see {}",
            paths.log.display()
        )
    }

    fn command(&self, home: &HomeLayout) -> Command {
        let mut command = detached_command(&self.executable);
        command.args(&self.run_args).env("CCCC_HOME", home.root());
        command
    }
}

async fn ping(home: &HomeLayout) -> bool {
    DaemonClient::new(home.clone())
        .with_timeout(Duration::from_millis(300))
        .call(&DaemonRequest {
            v: 1,
            op: "ping".into(),
            args: Map::new(),
        })
        .await
        .is_ok_and(|response| response.ok)
}

#[cfg(unix)]
fn detached_command(executable: &Path) -> Command {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("nohup");
    command.arg(executable);
    command.process_group(0);
    command
}

#[cfg(windows)]
fn detached_command(executable: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let mut command = Command::new(executable);
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    command
}

#[cfg(test)]
mod tests {
    use super::DetachedDaemon;
    use cccc_core::HomeLayout;
    use std::ffi::OsStr;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn builds_cli_self_launch_command() {
        let home = HomeLayout::from_path(Path::new("test-home").to_path_buf()).expect("home");
        let launch = DetachedDaemon::new("cccc", ["daemon", "run"]);
        let command = launch.command(&home);
        let args = command.get_args().collect::<Vec<_>>();

        #[cfg(unix)]
        {
            assert_eq!(command.get_program(), OsStr::new("nohup"));
            assert_eq!(
                args,
                [OsStr::new("cccc"), OsStr::new("daemon"), OsStr::new("run")]
            );
        }
        #[cfg(windows)]
        {
            assert_eq!(command.get_program(), OsStr::new("cccc"));
            assert_eq!(args, [OsStr::new("daemon"), OsStr::new("run")]);
        }

        assert!(command.get_envs().any(|(key, value)| {
            key == OsStr::new("CCCC_HOME") && value == Some(home.root().as_os_str())
        }));
    }

    #[tokio::test]
    async fn reports_a_child_that_exits_before_becoming_ready() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let result = tokio::time::timeout(Duration::from_secs(3), failing_daemon().start(&home))
            .await
            .expect("failed child must be reported without waiting for the ready timeout");
        let error = result.expect_err("failed child must not be reported as started");
        let detail = format!("{error:#}");
        assert!(detail.contains("exited before becoming ready"), "{detail}");
        assert!(detail.contains("23"), "{detail}");
    }

    #[cfg(unix)]
    fn failing_daemon() -> DetachedDaemon {
        DetachedDaemon::new("/bin/sh", ["-c", "exit 23"])
    }

    #[cfg(windows)]
    fn failing_daemon() -> DetachedDaemon {
        let command = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
        DetachedDaemon::new(command, ["/C", "exit 23"])
    }
}
