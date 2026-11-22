//! Observe the owned child without a timer, while retaining a safe way to clean it up on panic.
use portable_pty::{Child, ExitStatus};
use std::{io, thread};

#[cfg(unix)]
type Owner = std::sync::Arc<std::sync::Mutex<Box<dyn Child + Send + Sync>>>;
#[cfg(not(unix))]
type Owner = Box<dyn portable_pty::ChildKiller + Send + Sync>;

pub(super) struct Process {
    owner: Owner,
    waiter: Option<thread::JoinHandle<()>>,
}

impl Process {
    pub(super) fn watch(
        child: Box<dyn Child + Send + Sync>,
        report: impl FnOnce(io::Result<ExitStatus>) + Send + 'static,
    ) -> Self {
        #[cfg(unix)]
        let (owner, waiter) = {
            use tokio::signal::unix::{SignalKind, signal};
            let owner = std::sync::Arc::new(std::sync::Mutex::new(child));
            let observer = owner.clone();
            let waiter = thread::spawn(move || {
                let status = tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .and_then(|runtime| {
                        runtime.block_on(async {
                            // Register before checking our child: an exit either already happened or
                            // wakes this receiver. Unrelated SIGCHLDs only trigger another check.
                            let mut changes = signal(SignalKind::child())?;
                            loop {
                                match observer.lock().expect("child").try_wait() {
                                    Ok(Some(status)) => return Ok(status),
                                    Ok(None) => {}
                                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                                        continue;
                                    }
                                    Err(error) => return Err(error),
                                }
                                changes.recv().await.ok_or_else(|| {
                                    io::Error::other("child signal stream closed")
                                })?;
                            }
                        })
                    });
                report(status);
            });
            (owner, waiter)
        };
        #[cfg(not(unix))]
        let (owner, waiter) = {
            let mut child = child;
            // ConPTY's cloned killer owns a Windows process handle, independent of wait().
            let owner = child.clone_killer();
            (owner, thread::spawn(move || report(child.wait())))
        };
        Self {
            owner,
            waiter: Some(waiter),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // The observer and cleanup reap through the same Child. A numeric PID is never
            // signalled after another thread reaped it and the OS was free to reuse it.
            let mut child = self
                .owner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        #[cfg(not(unix))]
        if self
            .waiter
            .as_ref()
            .is_some_and(|waiter| !waiter.is_finished())
        {
            let _ = self.owner.kill();
        }
        if let Some(waiter) = self.waiter.take() {
            let _ = waiter.join();
        }
    }
}
