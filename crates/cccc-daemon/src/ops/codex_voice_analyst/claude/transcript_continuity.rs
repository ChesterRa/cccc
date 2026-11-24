//! Preserve the consumed byte prefix across provider-owned transcript moves.
use sha2::{Digest, Sha256};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const PRODUCTION_RELOCATION_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
pub(super) const RELOCATION_TIMEOUT: Duration = PRODUCTION_RELOCATION_TIMEOUT;

pub(super) struct Continuity {
    prefix: Sha256,
    missing_since: Option<tokio::time::Instant>,
    #[cfg(test)]
    timeout: Duration,
}

impl Default for Continuity {
    fn default() -> Self {
        Self {
            prefix: Sha256::new(),
            missing_since: None,
            #[cfg(test)]
            timeout: PRODUCTION_RELOCATION_TIMEOUT,
        }
    }
}

impl Continuity {
    #[cfg(test)]
    pub(super) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[cfg(test)]
    fn timeout(&self) -> Duration {
        self.timeout
    }

    #[cfg(not(test))]
    fn timeout(&self) -> Duration {
        PRODUCTION_RELOCATION_TIMEOUT
    }
}

impl Continuity {
    pub(super) fn consume(&mut self, bytes: &[u8]) {
        self.prefix.update(bytes);
    }

    pub(super) async fn initialize(
        &mut self,
        file: &mut tokio::fs::File,
        offset: u64,
    ) -> io::Result<()> {
        self.prefix = hash_prefix(file, offset, self.timeout()).await?;
        Ok(())
    }

    pub(super) async fn verify(&self, file: &mut tokio::fs::File, offset: u64) -> io::Result<()> {
        let observed = hash_prefix(file, offset, self.timeout()).await?;
        if observed.finalize() != self.prefix.clone().finalize() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude relocated transcript does not preserve consumed history",
            ));
        }
        Ok(())
    }

    pub(super) fn settle<T: Default>(&mut self, result: io::Result<T>) -> io::Result<T> {
        match result {
            Ok(value) => {
                self.missing_since = None;
                Ok(value)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::UnexpectedEof
                ) =>
            {
                let now = tokio::time::Instant::now();
                let since = self.missing_since.get_or_insert(now);
                if now.duration_since(*since) < self.timeout() {
                    Ok(T::default())
                } else {
                    Err(io::Error::new(
                        error.kind(),
                        format!(
                            "Claude transcript relocation did not settle within 10 seconds: {error}"
                        ),
                    ))
                }
            }
            Err(error) => Err(error),
        }
    }
}

async fn hash_prefix(
    file: &mut tokio::fs::File,
    offset: u64,
    timeout: Duration,
) -> io::Result<Sha256> {
    tokio::time::timeout(timeout, async {
        file.seek(std::io::SeekFrom::Start(0)).await?;
        let mut remaining = offset;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            let wanted = remaining.min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..wanted]).await?;
            digest.update(&buffer[..wanted]);
            remaining -= wanted as u64;
        }
        Ok(digest)
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "Claude transcript continuity check timed out",
        )
    })?
}
