#[cfg(windows)]
use anyhow::{Result, bail};
#[cfg(windows)]
use cccc_client::DaemonClient;
#[cfg(windows)]
use cccc_core::HomeLayout;
#[cfg(windows)]
use cccc_daemon::{DetachedDaemon, StartOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ownership {
    Owned,
    NotRunning,
    Replaced,
}

fn ownership(owned_pid: u32, running_pid: Option<u32>) -> Ownership {
    match running_pid {
        Some(running_pid) if running_pid == owned_pid => Ownership::Owned,
        Some(_) => Ownership::Replaced,
        None => Ownership::NotRunning,
    }
}

#[cfg(windows)]
pub(crate) struct OwnedDetachedDaemon {
    pid: u32,
}

#[cfg(windows)]
impl OwnedDetachedDaemon {
    pub(crate) async fn start(home: &HomeLayout, client: &DaemonClient) -> Result<Option<Self>> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if super::ping(client).await {
                return Ok(None);
            }

            let executable = std::env::current_exe()?;
            match DetachedDaemon::new(executable, ["daemon", "run"])
                .start(home)
                .await?
            {
                StartOutcome::Started(pid) => {
                    let owner = Self { pid };
                    if super::wait_for_compatible_daemon(client, deadline).await {
                        return Ok(Some(owner));
                    }
                    let cleanup = owner.stop(client, home).await;
                    if let Err(error) = cleanup {
                        bail!(
                            "Rust daemon failed to become compatible and cleanup failed: {error}; see {}",
                            home.daemon_dir().join("ccccd.log").display()
                        );
                    }
                    bail!(
                        "Rust daemon failed to become compatible; see {}",
                        home.daemon_dir().join("ccccd.log").display()
                    );
                }
                StartOutcome::AlreadyRunning => {
                    if tokio::time::Instant::now() >= deadline {
                        bail!(
                            "existing daemon did not hand off to the Rust daemon; see {}",
                            home.daemon_dir().join("ccccd.log").display()
                        );
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    pub(crate) async fn stop(&self, client: &DaemonClient, home: &HomeLayout) -> Result<()> {
        match ownership(self.pid, super::running_daemon_pid(client).await) {
            Ownership::Replaced => return Ok(()),
            Ownership::Owned => {
                if super::stop_daemon(client, home)
                    .await
                    .is_ok_and(|response| response.ok)
                {
                    return Ok(());
                }
            }
            Ownership::NotRunning => {}
        }

        let pid = self.pid.to_string();
        let output = std::process::Command::new("taskkill")
            .args(["/PID", pid.as_str(), "/T", "/F"])
            .output()
            .map_err(|error| {
                anyhow::anyhow!("failed to run taskkill for daemon {}: {error}", self.pid)
            })?;

        if matches!(
            ownership(self.pid, super::running_daemon_pid(client).await),
            Ownership::Replaced
        ) {
            return Ok(());
        }
        if !output.status.success()
            && super::wait_for_daemon_lock_release(home, std::time::Duration::from_millis(100))
                .await
                .is_err()
        {
            bail!(
                "failed to terminate daemon {}: {}",
                self.pid,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        super::wait_for_daemon_lock_release(home, super::DAEMON_SHUTDOWN_TIMEOUT).await
    }
}

#[cfg(test)]
mod tests {
    use super::{Ownership, ownership};

    #[test]
    fn only_the_spawned_daemon_is_owned() {
        assert_eq!(ownership(41, Some(41)), Ownership::Owned);
        assert_eq!(ownership(41, None), Ownership::NotRunning);
        assert_eq!(ownership(41, Some(42)), Ownership::Replaced);
    }
}
