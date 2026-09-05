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

#[cfg(windows)]
const DETACHED_CREATION_FLAGS: u32 = 0x0000_0200 | 0x0000_0008;

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
        Ok(self.start_inner(home, false).await?.map(|(child, _)| child))
    }

    /// Start a Web-owned daemon with a tree registered before readiness waits.
    pub async fn start_supervised(
        &self,
        home: &HomeLayout,
    ) -> Result<Option<(Child, cccc_runtime::OwnedProcessTree)>> {
        Ok(self
            .start_inner(home, true)
            .await?
            .map(|(child, tree)| (child, tree.expect("supervised spawn owns a tree"))))
    }

    async fn start_inner(
        &self,
        home: &HomeLayout,
        supervised: bool,
    ) -> Result<Option<(Child, Option<cccc_runtime::OwnedProcessTree>)>> {
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
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .current_dir(home.root());
        let (mut child, tree) = if supervised {
            #[cfg(unix)]
            let spawned = cccc_runtime::OwnedProcessTree::spawn(&mut command);
            #[cfg(windows)]
            let spawned = cccc_runtime::OwnedProcessTree::spawn_with_creation_flags(
                &mut command,
                DETACHED_CREATION_FLAGS,
            );
            let (child, tree) = spawned
                .with_context(|| format!("spawn Rust daemon via {}", self.executable.display()))?;
            (child, Some(tree))
        } else {
            (
                command.spawn().with_context(|| {
                    format!("spawn Rust daemon via {}", self.executable.display())
                })?,
                None,
            )
        };

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if ping(home).await {
                return Ok(Some((child, tree)));
            }
            let status = match &tree {
                Some(tree) => tree.try_wait(|| child.try_wait()),
                None => child.try_wait(),
            }
            .context("poll Rust daemon process")?;
            if let Some(status) = status {
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

        if let Some(tree) = &tree {
            let _ = tree.terminate();
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
    let mut command = Command::new(executable);
    command.creation_flags(DETACHED_CREATION_FLAGS);
    command
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
