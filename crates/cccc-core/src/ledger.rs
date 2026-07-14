use cccc_contracts::Event;
use flate2::read::GzDecoder;
use fs2::FileExt;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceRevision {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
pub struct LedgerFollower {
    initialized: bool,
    sources: BTreeMap<PathBuf, SourceRevision>,
}

impl LedgerFollower {
    pub fn poll(&mut self, path: &Path) -> io::Result<Vec<Event>> {
        let revisions = revisions(path)?;
        let next_sources: BTreeMap<_, _> = revisions
            .iter()
            .cloned()
            .map(|revision| (revision.path.clone(), revision))
            .collect();
        if !self.initialized {
            self.initialized = true;
            self.sources = next_sources;
            return Ok(Vec::new());
        }
        if self.sources == next_sources {
            return Ok(Vec::new());
        }

        let rotated_source = self.sources.get(path).and_then(|active| {
            let active_was_replaced = next_sources
                .get(path)
                .is_none_or(|current| current.len < active.len);
            active_was_replaced.then(|| {
                revisions
                    .iter()
                    .rev()
                    .find(|revision| {
                        revision.path != path
                            && !self.sources.contains_key(&revision.path)
                            && revision.len >= active.len
                    })
                    .map(|revision| (revision.path.clone(), active.len))
            })?
        });
        let mut appended = Vec::new();
        for revision in &revisions {
            let offset = match self.sources.get(&revision.path) {
                Some(previous) if revision.len >= previous.len => previous.len,
                Some(_) => 0,
                None if rotated_source
                    .as_ref()
                    .is_some_and(|(source, _)| source == &revision.path) =>
                {
                    rotated_source
                        .as_ref()
                        .map_or(revision.len, |(_, offset)| *offset)
                }
                None if revision.path == path => 0,
                None => revision.len,
            };
            if offset < revision.len {
                appended.extend(read_source_from(&revision.path, offset)?);
            }
        }
        self.sources = next_sources;
        Ok(appended)
    }
}

pub fn append(path: &Path, event: &Event) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    file.lock_exclusive()?;
    let result = append_locked(&mut file, event);
    let unlock_result = FileExt::unlock(&file);
    result.and(unlock_result)
}

fn append_locked(file: &mut File, event: &Event) -> io::Result<()> {
    serde_json::to_writer(&mut *file, event).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_data()
}

pub fn read_all(path: &Path) -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    for source in source_paths(path)? {
        events.extend(read_source(&source)?);
    }
    Ok(events)
}

fn source_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let segments = path
        .parent()
        .map(|group| group.join("state/ledger/segments"));
    if let Some(segments) = segments.filter(|dir| dir.is_dir()) {
        let mut selected = BTreeMap::<PathBuf, PathBuf>::new();
        for candidate in std::fs::read_dir(segments)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| is_ledger_segment(candidate))
        {
            let logical = logical_segment_path(&candidate);
            let replace = !selected.contains_key(&logical) || is_gzip(&candidate);
            if replace {
                selected.insert(logical, candidate);
            }
        }
        paths.extend(selected.into_values());
    }
    if path.exists() {
        paths.push(path.to_path_buf());
    }
    Ok(paths)
}

fn revisions(path: &Path) -> io::Result<Vec<SourceRevision>> {
    source_paths(path)?
        .into_iter()
        .map(|source| {
            let metadata = source.metadata()?;
            Ok(SourceRevision {
                path: source,
                len: metadata.len(),
                modified: metadata.modified().ok(),
            })
        })
        .collect()
}

fn read_source(path: &Path) -> io::Result<Vec<Event>> {
    if is_gzip(path) {
        return read_events(BufReader::new(GzDecoder::new(File::open(path)?)));
    }
    read_source_from(path, 0)
}

fn read_source_from(path: &Path, offset: u64) -> io::Result<Vec<Event>> {
    if is_gzip(path) {
        return if offset == 0 {
            read_source(path)
        } else {
            Ok(Vec::new())
        };
    }
    let mut file = File::open(path)?;
    file.seek(io::SeekFrom::Start(offset))?;
    read_events(BufReader::new(file))
}

fn read_events(reader: impl BufRead) -> io::Result<Vec<Event>> {
    reader
        .lines()
        .filter(|line| line.as_ref().map_or(true, |value| !value.trim().is_empty()))
        .map(|line| {
            let line = line?;
            serde_json::from_str(&line).map_err(io::Error::other)
        })
        .collect()
}

fn is_ledger_segment(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    name.ends_with(".jsonl") || name.ends_with(".jsonl.gz")
}

fn is_gzip(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "gz")
}

fn logical_segment_path(path: &Path) -> PathBuf {
    if !is_gzip(path) {
        return path.to_path_buf();
    }
    path.with_extension("")
}

pub fn tail(path: &Path, limit: usize) -> io::Result<Vec<Event>> {
    let events = read_all(path)?;
    let start = events.len().saturating_sub(limit);
    Ok(events[start..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    #[test]
    fn appends_and_reads_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        append(&path, &Event::new("group.create", "g_test")).expect("append");
        append(&path, &Event::new("chat.message", "g_test")).expect("append");
        let events = tail(&path, 1).expect("tail");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "chat.message");
    }

    #[test]
    fn follower_starts_at_end_and_only_returns_appended_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        append(&path, &Event::new("group.create", "g_test")).expect("append initial");

        let mut follower = LedgerFollower::default();
        assert!(follower.poll(&path).expect("initial poll").is_empty());
        assert!(follower.poll(&path).expect("unchanged poll").is_empty());

        append(&path, &Event::new("chat.message", "g_test")).expect("append message");
        let events = follower.poll(&path).expect("changed poll");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "chat.message");
    }

    #[test]
    fn follower_does_not_replay_after_truncation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        append(&path, &Event::new("group.create", "g_test")).expect("append initial");
        append(&path, &Event::new("chat.message", "g_test")).expect("append message");

        let mut follower = LedgerFollower::default();
        follower.poll(&path).expect("initial poll");
        std::fs::write(&path, "").expect("truncate");
        assert!(follower.poll(&path).expect("truncated poll").is_empty());

        append(&path, &Event::new("chat.message", "g_test")).expect("append after truncate");
        let events = follower.poll(&path).expect("poll after truncate");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "chat.message");
    }

    #[test]
    fn follower_continues_from_active_ledger_after_rotation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        append(&path, &Event::new("group.create", "g_test")).expect("append initial");

        let mut follower = LedgerFollower::default();
        follower.poll(&path).expect("initial poll");
        append(&path, &Event::new("chat.message", "g_test")).expect("append before rotation");

        let segments = temp.path().join("state/ledger/segments");
        std::fs::create_dir_all(&segments).expect("segments");
        std::fs::rename(&path, segments.join("ledger.0001.jsonl")).expect("rotate");
        File::create(&path).expect("new active ledger");

        let events = follower.poll(&path).expect("poll after rotation");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "chat.message");
    }

    #[test]
    fn reads_python_gzip_segments_without_duplicate_plain_source() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let segments = temp.path().join("state/ledger/segments");
        std::fs::create_dir_all(&segments).expect("segments");
        let event = Event::new("chat.message", "g_test");
        let encoded = format!("{}\n", serde_json::to_string(&event).expect("event"));
        let plain = segments.join("ledger.20260101T000000Z.000001.jsonl");
        std::fs::write(&plain, &encoded).expect("plain segment");
        let gzip = plain.with_extension("jsonl.gz");
        let mut encoder =
            GzEncoder::new(File::create(&gzip).expect("gzip"), Compression::default());
        encoder.write_all(encoded.as_bytes()).expect("gzip data");
        encoder.finish().expect("finish gzip");
        File::create(&path).expect("active ledger");

        let events = read_all(&path).expect("read all");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event.id);
    }
}
