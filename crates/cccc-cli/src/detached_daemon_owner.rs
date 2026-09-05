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
    process_tree: cccc_runtime::OwnedProcessTree,
}

#[cfg(windows)]
pub(crate) type SharedOwnedDetachedDaemon =
    std::sync::Arc<tokio::sync::Mutex<Option<OwnedDetachedDaemon>>>;

#[cfg(windows)]
pub(crate) fn shared(owner: Option<OwnedDetachedDaemon>) -> SharedOwnedDetachedDaemon {
    std::sync::Arc::new(tokio::sync::Mutex::new(owner))
}

#[cfg(windows)]
pub(crate) async fn stop_shared(
    owner: &SharedOwnedDetachedDaemon,
    client: &DaemonClient,
) -> Result<()> {
    let expected_pid = owner.lock().await.as_ref().map(|owner| owner.child.id());
    let Some(expected_pid) = expected_pid else {
        return Ok(());
    };
    let deadline = tokio::time::Instant::now() + crate::daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT;
    let lifecycle_client = client
        .clone()
        .with_timeout(crate::daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT);
    let _ = crate::call(&lifecycle_client, "shutdown", shutdown_args(expected_pid)).await;
    loop {
        let mut guard = owner.lock().await;
        let Some(owned) = guard.as_mut() else {
            return Ok(());
        };
        if owned.wait_for_exit(std::time::Duration::ZERO).await? {
            guard.take();
            return Ok(());
        }
        drop(guard);
        if tokio::time::Instant::now() >= deadline {
            return force_stop_shared(owner).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Terminate the exact child retained by the Web host before process::exit can
/// skip its destructor. The daemon's Windows Job object takes its actor tree
/// down with it.
#[cfg(windows)]
pub(crate) async fn force_stop_shared(owner: &SharedOwnedDetachedDaemon) -> Result<()> {
    let mut guard = owner.lock().await;
    let Some(owner) = guard.as_mut() else {
        return Ok(());
    };
    owner.terminate_owned().await?;
    guard.take();
    Ok(())
}

#[cfg(windows)]
impl OwnedDetachedDaemon {
    pub(crate) async fn start(home: &HomeLayout, client: &DaemonClient) -> Result<Option<Self>> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if crate::daemon_lifecycle::ping(client).await {
                return Ok(None);
            }

            let executable = std::env::current_exe()?;
            match DetachedDaemon::new(executable, ["daemon", "run"])
                .start_supervised(home)
                .await?
            {
                Some((child, process_tree)) => {
                    let mut owner = Self {
                        child,
                        process_tree,
                    };
                    if crate::daemon_lifecycle::wait_until_ready(client, deadline).await {
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
        let deadline =
            tokio::time::Instant::now() + crate::daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT;
        let lifecycle_client = client
            .clone()
            .with_timeout(crate::daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT);
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
        if let Err(error) = self.process_tree.terminate()
            && !self.wait_for_exit(std::time::Duration::ZERO).await?
        {
            bail!(
                "failed to terminate owned daemon {}: {error}",
                self.child.id()
            );
        }
        if self
            .wait_for_exit(crate::daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT)
            .await?
        {
            return Ok(());
        }
        bail!(
            "owned daemon {} did not exit within {} seconds",
            self.child.id(),
            crate::daemon_lifecycle::DAEMON_SHUTDOWN_TIMEOUT.as_secs()
        )
    }

    async fn wait_for_exit(&mut self, timeout: std::time::Duration) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self
                .process_tree
                .try_wait(|| self.child.try_wait())?
                .is_some()
            {
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

    #[cfg(windows)]
    #[tokio::test]
    async fn forced_stop_terminates_the_retained_child() {
        let (child, process_tree) = cccc_runtime::OwnedProcessTree::spawn(
            std::process::Command::new("cmd").args(["/C", "ping -n 30 127.0.0.1 > NUL"]),
        )
        .expect("spawn child");
        let owner = super::shared(Some(super::OwnedDetachedDaemon {
            child,
            process_tree,
        }));

        super::force_stop_shared(&owner)
            .await
            .expect("force stop exact child");

        assert!(owner.lock().await.is_none(), "owned child must be consumed");
    }
}
