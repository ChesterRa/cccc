//! Preserve the consumed byte prefix across provider-owned transcript moves.
use sha2::{Digest, Sha256};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub(super) const RELOCATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(super) struct Continuity {
    prefix: Sha256,
    missing_since: Option<tokio::time::Instant>,
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
        self.prefix = hash_prefix(file, offset).await?;
        Ok(())
    }

    pub(super) async fn verify(&self, file: &mut tokio::fs::File, offset: u64) -> io::Result<()> {
        let observed = hash_prefix(file, offset).await?;
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
                if now.duration_since(*since) < RELOCATION_TIMEOUT {
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

async fn hash_prefix(file: &mut tokio::fs::File, offset: u64) -> io::Result<Sha256> {
    tokio::time::timeout(RELOCATION_TIMEOUT, async {
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
