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
    update_resume: Option<PathBuf>,
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
            update_resume: None,
        }
    }

    /// An explicit permit for the daemon started by an update's replacement
    /// host. Ordinary callers must not adopt an in-progress update's daemon.
    pub fn with_update_resume(mut self, transaction_dir: PathBuf) -> Self {
        self.update_resume = Some(transaction_dir);
        self
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
        Ok(self
            .start_inner(home, false, None)
            .await?
            .map(|(child, _)| child))
    }

    /// Start a Web-owned daemon with a tree registered before readiness waits.
    pub async fn start_supervised(
        &self,
        home: &HomeLayout,
    ) -> Result<Option<(Child, cccc_runtime::OwnedProcessTree)>> {
        self.supervised_start(home, None).await
    }

    /// Share the caller's startup budget without cancelling a future that owns a child.
    /// Reclaiming a child after that deadline remains a separate lifecycle operation.
    pub async fn start_supervised_until(
        &self,
        home: &HomeLayout,
        deadline: tokio::time::Instant,
    ) -> Result<Option<(Child, cccc_runtime::OwnedProcessTree)>> {
        self.supervised_start(home, Some(deadline)).await
    }

    async fn supervised_start(
        &self,
        home: &HomeLayout,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Option<(Child, cccc_runtime::OwnedProcessTree)>> {
        Ok(self
            .start_inner(home, true, deadline)
            .await?
            .map(|(child, tree)| (child, tree.expect("supervised spawn owns a tree"))))
    }

    async fn start_inner(
        &self,
        home: &HomeLayout,
        supervised: bool,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Option<(Child, Option<cccc_runtime::OwnedProcessTree>)>> {
        home.initialize()?;
        cccc_core::attended_update::check_start(
            home,
            self.update_resume.as_deref(),
            &cccc_core::attended_update::BuildIdentity::current(),
        )?;
        if ping(home, deadline).await {
            return Ok(None);
        }
        check_startup_deadline(deadline)?;

        let paths = DaemonPaths::new(home.clone());
        #[cfg(unix)]
        crate::server::validate_endpoint_path(&paths)?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&paths.log)?;
        let log_start = log.metadata()?.len();
        let error_log = log.try_clone()?;
        let mut command = self.command(home);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .current_dir(home.root());
        check_startup_deadline(deadline)?;
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

        let ready_deadline =
            deadline.unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(30));
        loop {
            if ping(home, deadline).await {
                return Ok(Some((child, tree)));
            }
            let status = match &tree {
                Some(tree) => tree.try_wait(|| child.try_wait()),
                None => child.try_wait(),
            }
            .context("poll Rust daemon process")?;
            if let Some(status) = status {
                let recovery = session_recovery_hint(&paths.log, log_start)
                    .map(|hint| format!("; {hint}"))
                    .unwrap_or_default();
                anyhow::bail!(
                    "Rust daemon exited before becoming ready with {status}{recovery}; see {}",
                    paths.log.display()
                );
            }
            if tokio::time::Instant::now() >= ready_deadline {
                break;
            }
            let next_probe = tokio::time::Instant::now() + Duration::from_millis(100);
            tokio::time::sleep_until(deadline.map_or(next_probe, |end| end.min(next_probe))).await;
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
        if let Some(transaction) = &self.update_resume {
            command.arg("--update-resume").arg(transaction);
        }
        command
    }
}

/// Surface only the fixed recovery command, never arbitrary provider error
/// text from the shared log, which may contain private environment values.
fn session_recovery_hint(log: &Path, start: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(log).ok()?;
    let end = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(start.max(end.saturating_sub(64 * 1024))))
        .ok()?;
    let mut bytes = Vec::new();
    file.take(64 * 1024).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let prefix = "cccc daemon process-recovery --owner session --owner-id ";
    for part in text.rsplit(prefix).take(text.matches(prefix).count()) {
        let Some((id, suffix)) = part.split_once(' ') else {
            continue;
        };
        let Ok(parsed) = uuid::Uuid::parse_str(id) else {
            continue;
        };
        if id == parsed.simple().to_string() && suffix.starts_with("list") {
            return Some(format!(
                "session ownership needs review; run `{prefix}{id} list`"
            ));
        }
    }
    None
}

fn check_startup_deadline(deadline: Option<tokio::time::Instant>) -> Result<()> {
    if deadline.is_some_and(|end| tokio::time::Instant::now() >= end) {
        anyhow::bail!("Rust daemon startup deadline elapsed before process creation");
    }
    Ok(())
}

async fn ping(home: &HomeLayout, deadline: Option<tokio::time::Instant>) -> bool {
    let now = tokio::time::Instant::now();
    let probe_deadline = now + Duration::from_millis(300);
    let probe_deadline = deadline.map_or(probe_deadline, |end| end.min(probe_deadline));
    if now >= probe_deadline {
        return false;
    }
    let client = DaemonClient::new(home.clone());
    let request = DaemonRequest {
        v: 1,
        op: "ping".into(),
        args: Map::new(),
    };
    matches!(tokio::time::timeout_at(probe_deadline, client.call(&request)).await,
        Ok(Ok(response)) if response.ok)
        && tokio::time::Instant::now() < probe_deadline
}

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "The daemon has its own detached startup policy"
)]
fn detached_command(executable: &Path) -> Command {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("nohup");
    command.arg(executable);
    command.process_group(0);
    command
}

#[cfg(windows)]
#[expect(
    clippy::disallowed_methods,
    reason = "The daemon uses explicit detached flags, not the background default"
)]
fn detached_command(executable: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(executable);
    command.creation_flags(DETACHED_CREATION_FLAGS);
    command
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
