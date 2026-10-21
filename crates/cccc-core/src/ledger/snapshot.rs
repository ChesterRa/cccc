//! Short-lock snapshots: pin file identity and committed byte bounds, then
//! decode outside the append/rotation lock. No descriptor is kept by a cache.
use super::{
    acquire_reader_lock, decode_event_line, is_gzip, ledger_group_id, source_paths, trim_ascii,
};
use cccc_contracts::Event;
use flate2::read::GzDecoder;
use std::fs::{File, Metadata};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[cfg(test)]
static DECODED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) fn decoded_records() -> u64 {
    DECODED.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    pub path: PathBuf,
    pub identity: (u64, u64),
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl Source {
    pub(super) fn from_file(path: &Path, file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        Ok(Self {
            path: path.to_owned(),
            identity: identity(file, &metadata)?,
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
    pub fn extends(&self, previous: &Self) -> bool {
        self.path == previous.path
            && self.identity == previous.identity
            && (self.len > previous.len || self == previous)
    }
}

#[cfg(unix)]
fn identity(_: &File, metadata: &Metadata) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}
#[cfg(not(unix))]
fn identity(file: &File, _: &Metadata) -> io::Result<(u64, u64)> {
    use std::hash::{Hash, Hasher};
    let handle = same_file::Handle::from_file(file.try_clone()?)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    handle.hash(&mut hash);
    Ok((hash.finish(), 0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Location {
    pub source: usize,
    // Byte positions in the decoded stream for gzip, physical bytes otherwise.
    pub offset: u64,
    pub end: u64,
}

pub(crate) struct Snapshot {
    pub sources: Vec<Source>,
    files: Vec<File>,
    group_id: String,
}

impl Snapshot {
    pub fn open(path: &Path) -> io::Result<Self> {
        let _lock = acquire_reader_lock(path)?;
        let mut sources = Vec::new();
        let mut files = Vec::new();
        for path in source_paths(path)? {
            let file = File::open(&path)?;
            sources.push(Source::from_file(&path, &file)?);
            files.push(file);
        }
        Ok(Self {
            sources,
            files,
            group_id: ledger_group_id(path),
        })
    }

    pub fn extends(&self, previous: &[Source]) -> bool {
        self.sources.len() == previous.len()
            && self
                .sources
                .iter()
                .zip(previous)
                .all(|(next, old)| next.extends(old) && (!is_gzip(&next.path) || next == old))
    }

    fn reader(&self, source: usize, offset: u64) -> io::Result<Box<dyn BufRead>> {
        let token = &self.sources[source];
        let mut file = self.files[source].try_clone()?;
        // Cloned descriptors share their offset; each snapshot is used serially.
        file.seek(SeekFrom::Start(0))?;
        if is_gzip(&token.path) {
            let mut decoded = GzDecoder::new(file.take(token.len));
            io::copy(&mut decoded.by_ref().take(offset), &mut io::sink())?;
            Ok(Box::new(BufReader::new(decoded)))
        } else {
            file.seek(SeekFrom::Start(offset))?;
            Ok(Box::new(BufReader::new(
                file.take(token.len.saturating_sub(offset)),
            )))
        }
    }

    pub fn visit(
        &self,
        offsets: &[u64],
        mut visit: impl FnMut(Event, Location) -> ControlFlow<()>,
    ) -> io::Result<Vec<u64>> {
        let mut ends = Vec::new();
        for (source, token) in self.sources.iter().enumerate() {
            let mut offset = offsets.get(source).copied().unwrap_or(0);
            if offset == u64::MAX {
                ends.push(offset);
                continue;
            }
            let mut reader = self.reader(source, offset)?;
            let mut line = Vec::new();
            let mut line_no = 0;
            loop {
                let read = reader.read_until(b'\n', &mut line)?;
                if read == 0 {
                    break;
                }
                line_no += 1;
                #[cfg(test)]
                DECODED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let raw = trim_ascii(&line);
                if line.last() != Some(&b'\n')
                    && !raw.is_empty()
                    && serde_json::from_slice::<serde_json::Value>(raw).is_err()
                {
                    break;
                }
                let end = offset + read as u64;
                if !raw.is_empty()
                    && let Some(event) =
                        decode_event_line(raw, &token.path, line_no, &self.group_id)
                    && visit(
                        event,
                        Location {
                            source,
                            offset,
                            end,
                        },
                    )
                    .is_break()
                {
                    return Ok(ends);
                }
                offset = end;
                line.clear();
            }
            ends.push(if is_gzip(&token.path) {
                u64::MAX
            } else {
                offset
            });
        }
        Ok(ends)
    }

    pub fn event(&self, location: Location) -> io::Result<Event> {
        let mut reader = self.reader(location.source, location.offset)?;
        let length = usize::try_from(location.end - location.offset).map_err(io::Error::other)?;
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes)?;
        serde_json::from_slice(trim_ascii(&bytes)).map_err(io::Error::other)
    }
}
