#[cfg(windows)]
use anyhow::{Result, bail};
#[cfg(windows)]
use cccc_client::DaemonClient;
#[cfg(windows)]
use cccc_core::HomeLayout;
#[cfg(windows)]
use cccc_daemon::DetachedDaemon;

fn shutdown_args(expected_pid: u32) -> serde_json::Value {
    serde_json::json!({"expected_pid":expected_pid})
}

#[cfg(windows)]
pub(crate) struct OwnedDetachedDaemon {
    child: std::process::Child,
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
                .start_owned(home)
                .await?
            {
                Some(child) => {
                    let mut owner = Self { child };
                    if super::wait_for_compatible_daemon(client, deadline).await {
                        return Ok(Some(owner));
                    }
                    let cleanup = owner.stop(client).await;
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
                None => {
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

    pub(crate) async fn stop(&mut self, client: &DaemonClient) -> Result<()> {
        if self.wait_for_exit(std::time::Duration::ZERO).await? {
            return Ok(());
        }

        // A shutdown may legitimately wait behind an in-flight global write.
        // Give it the full lifecycle deadline and fence it with the exact PID
        // we spawned so DaemonClient's descriptor retry cannot stop a
        // replacement daemon.
        let deadline = tokio::time::Instant::now() + super::DAEMON_SHUTDOWN_TIMEOUT;
        let lifecycle_client = client.clone().with_timeout(super::DAEMON_SHUTDOWN_TIMEOUT);
        let _ = super::call(
            &lifecycle_client,
            "shutdown",
            shutdown_args(self.child.id()),
        )
        .await;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if self.wait_for_exit(remaining).await? {
            return Ok(());
        }
        self.terminate_owned().await
    }

    async fn terminate_owned(&mut self) -> Result<()> {
        if self.wait_for_exit(std::time::Duration::ZERO).await? {
            return Ok(());
        }
        if let Err(error) = self.child.kill()
            && !self.wait_for_exit(std::time::Duration::ZERO).await?
        {
            bail!(
                "failed to terminate owned daemon {}: {error}",
                self.child.id()
            );
        }
        if self.wait_for_exit(super::DAEMON_SHUTDOWN_TIMEOUT).await? {
            return Ok(());
        }
        bail!(
            "owned daemon {} did not exit within {} seconds",
            self.child.id(),
            super::DAEMON_SHUTDOWN_TIMEOUT.as_secs()
        )
    }

    async fn wait_for_exit(&mut self, timeout: std::time::Duration) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.child.try_wait()?.is_some() {
                return Ok(true);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::shutdown_args;

    #[test]
    fn shutdown_is_fenced_to_the_spawned_daemon() {
        assert_eq!(shutdown_args(41), serde_json::json!({"expected_pid":41}));
    }
}
