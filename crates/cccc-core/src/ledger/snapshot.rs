//! Short-lock snapshots: retain source identity and committed byte bounds,
//! pin only the rotating active file, and open immutable archives on demand.
//! Decoding runs outside the writer lock; caches retain no descriptors.
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
thread_local! {
    static GZIP_READERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DECODED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn decoded_records() -> u64 {
    // Snapshot decoding is synchronous on the query thread. Parallel tests
    // must not count each other's unrelated history reads.
    DECODED.with(std::cell::Cell::get)
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
    active: Option<(usize, File)>,
    group_id: String,
}

impl Snapshot {
    pub fn open(path: &Path) -> io::Result<Self> {
        let _lock = acquire_reader_lock(path)?;
        let mut sources = Vec::new();
        let mut active = None;
        for source_path in source_paths(path)? {
            let file = File::open(&source_path)?;
            let index = sources.len();
            sources.push(Source::from_file(&source_path, &file)?);
            if source_path == path {
                // Compaction renames this file. A pin retains the captured
                // prefix without retaining one descriptor per sealed segment.
                active = Some((index, file));
            }
        }
        Ok(Self {
            sources,
            active,
            group_id: ledger_group_id(path),
        })
    }

    pub fn extends(&self, previous: &[Source]) -> bool {
        self.sources.len() == previous.len()
            && self
                .sources
                .iter()
                .zip(previous)
                .enumerate()
                .all(|(index, (next, old))| {
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|(active, _)| *active == index)
                    {
                        next.extends(old)
                    } else {
                        next == old
                    }
                })
    }

    fn file(&self, source: usize) -> io::Result<File> {
        if let Some((index, file)) = &self.active
            && *index == source
        {
            return file.try_clone();
        }
        let token = &self.sources[source];
        let file = File::open(&token.path)?;
        if Source::from_file(&token.path, &file)? != *token {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "ledger archive changed during snapshot",
            ));
        }
        Ok(file)
    }

    fn reader(&self, source: usize, offset: u64) -> io::Result<Box<dyn BufRead>> {
        let token = &self.sources[source];
        let mut file = self.file(source)?;
        // Cloned descriptors share their offset; each snapshot is used serially.
        file.seek(SeekFrom::Start(0))?;
        if is_gzip(&token.path) {
            #[cfg(test)]
            GZIP_READERS.with(|count| count.set(count.get() + 1));
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
                DECODED.with(|count| count.set(count.get() + 1));
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

    /// Read a bounded page in source order, then restore request order. A gzip
    /// prefix is decoded once per source, even for shared-recipient messages.
    pub fn events(&self, locations: &[Location]) -> io::Result<Vec<Event>> {
        let mut ordered: Vec<_> = locations.iter().copied().enumerate().collect();
        ordered.sort_unstable_by_key(|(_, location)| *location);
        let mut result: Vec<Option<Event>> = vec![None; locations.len()];
        let mut start = 0;
        while start < ordered.len() {
            let source = ordered[start].1.source;
            let token = &self.sources[source];
            let mut end = start + 1;
            while end < ordered.len() && ordered[end].1.source == source {
                end += 1;
            }
            let mut position = ordered[start].1.offset;
            let mut compressed = if is_gzip(&token.path) {
                Some(self.reader(source, position)?)
            } else {
                None
            };
            let mut plain = if compressed.is_none() {
                Some(self.file(source)?)
            } else {
                None
            };
            let mut previous: Option<(Location, usize)> = None;
            for &(index, location) in &ordered[start..end] {
                if let Some((last, previous_index)) = previous
                    && last == location
                {
                    result[index] = result[previous_index].clone();
                    continue;
                }
                let event = if let Some(reader) = compressed.as_mut() {
                    let skip = location.offset.checked_sub(position).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "overlapping ledger locations")
                    })?;
                    if io::copy(&mut reader.by_ref().take(skip), &mut io::sink())? != skip {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    let event = read_event(reader, location)?;
                    position = location.end;
                    event
                } else {
                    let file = plain.as_mut().expect("plain source");
                    file.seek(SeekFrom::Start(location.offset))?;
                    read_event(
                        &mut file.take(token.len.saturating_sub(location.offset)),
                        location,
                    )?
                };
                result[index] = Some(event);
                previous = Some((location, index));
            }
            start = end;
        }
        Ok(result
            .into_iter()
            .map(|event| event.expect("requested event"))
            .collect())
    }
}

fn read_event(reader: &mut impl Read, location: Location) -> io::Result<Event> {
    let length = usize::try_from(location.end - location.offset).map_err(io::Error::other)?;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(trim_ascii(&bytes)).map_err(io::Error::other)
}

#[cfg(test)]
mod tests;
