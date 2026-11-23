//! Test-only provider handshake: pipe readiness proves entry without polling/sleeps.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::Duration,
};

pub(super) struct ProviderGate {
    entered: File,
    release: File,
    released: bool,
}

impl ProviderGate {
    pub(super) fn new(provider: &Path) -> Self {
        let entered = provider.join("provider-entered");
        let release = provider.join("provider-release");
        assert!(
            std::process::Command::new("mkfifo")
                .args([&entered, &release])
                .status()
                .expect("create provider pipes")
                .success()
        );
        let open = |path| {
            // Keep both ends open: opening never waits for the child, and the
            // readiness reader cannot see EOF before the provider has entered.
            OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(nix::libc::O_NONBLOCK)
                .open(path)
                .expect("open provider pipe")
        };
        Self {
            entered: open(entered),
            release: open(release),
            released: false,
        }
    }

    pub(super) fn wait_entered(&self) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("pipe runtime");
        runtime.block_on(async {
            let pipe = tokio::io::unix::AsyncFd::new(
                self.entered.try_clone().expect("clone entry reader"),
            )
            .expect("pipe readiness");
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let mut ready = pipe.readable().await?;
                    if let Ok(result) = ready.try_io(|pipe| {
                        let mut file = pipe.get_ref();
                        let mut byte = [0];
                        file.read_exact(&mut byte)
                    }) {
                        return result;
                    }
                }
            })
            .await
            .expect("provider entry deadline")
            .expect("provider entered");
        });
    }

    pub(super) fn release(&mut self) -> io::Result<()> {
        if !self.released {
            // Enough releases for all four workers even when a broken fence
            // wrongly submits duplicates. Native capture also has a 10s deadline.
            self.release.write_all(b"go\ngo\ngo\ngo\n")?;
            self.released = true;
        }
        Ok(())
    }
}

impl Drop for ProviderGate {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
