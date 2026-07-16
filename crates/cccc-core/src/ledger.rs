use cccc_contracts::Event;
use flate2::read::GzDecoder;
use fs2::FileExt;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
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
    pub fn at_end(path: &Path) -> io::Result<(Self, Option<String>)> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;
        FileExt::lock_shared(&file)?;
        let result = (|| {
            let sources = revisions(path)?
                .into_iter()
                .map(|revision| (revision.path.clone(), revision))
                .collect();
            let cursor = tail(path, 1)?.last().map(|event| event.id.clone());
            Ok((
                Self {
                    initialized: true,
                    sources,
                },
                cursor,
            ))
        })();
        let unlock = FileExt::unlock(&file);
        result.and_then(|value| unlock.map(|()| value))
    }

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
    tail_filtered(path, limit, None).map(|(events, _)| events)
}

pub fn tail_filtered(
    path: &Path,
    limit: usize,
    kind: Option<&str>,
) -> io::Result<(Vec<Event>, bool)> {
    if limit == 0 {
        return Ok((Vec::new(), false));
    }

    let target = limit.saturating_add(1);
    let mut newest_first = Vec::with_capacity(target);
    for source in source_paths(path)?.iter().rev() {
        let remaining = target.saturating_sub(newest_first.len());
        if remaining == 0 {
            break;
        }
        newest_first.extend(read_source_reverse(source, remaining, kind)?);
    }

    let has_more = newest_first.len() > limit;
    newest_first.truncate(limit);
    newest_first.reverse();
    Ok((newest_first, has_more))
}

pub fn events_after(path: &Path, event_id: &str, limit: usize) -> io::Result<Vec<Event>> {
    if event_id.trim().is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let events = read_all(path)?;
    let Some(index) = events.iter().position(|event| event.id == event_id) else {
        return Ok(Vec::new());
    };
    Ok(events
        .into_iter()
        .skip(index.saturating_add(1))
        .take(limit)
        .collect())
}

fn read_source_reverse(path: &Path, limit: usize, kind: Option<&str>) -> io::Result<Vec<Event>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if is_gzip(path) {
        return Ok(read_source(path)?
            .into_iter()
            .rev()
            .filter(|event| event_matches_kind(event, kind))
            .take(limit)
            .collect());
    }

    const CHUNK_SIZE: u64 = 64 * 1024;
    let mut file = File::open(path)?;
    FileExt::lock_shared(&file)?;
    let result: io::Result<Vec<Event>> = (|| {
        let mut position = file.metadata()?.len();
        let mut pending = Vec::new();
        let mut events = Vec::with_capacity(limit);

        while position > 0 && events.len() < limit {
            let start = position.saturating_sub(CHUNK_SIZE);
            let chunk_len = usize::try_from(position - start).map_err(io::Error::other)?;
            let mut buffer = vec![0; chunk_len];
            file.seek(io::SeekFrom::Start(start))?;
            file.read_exact(&mut buffer)?;
            buffer.extend_from_slice(&pending);

            let mut line_end = buffer.len();
            while line_end > 0 && events.len() < limit {
                let Some(newline) = buffer[..line_end].iter().rposition(|byte| *byte == b'\n')
                else {
                    break;
                };
                push_reverse_event(&buffer[newline + 1..line_end], kind, &mut events)?;
                line_end = newline;
            }
            pending = buffer[..line_end].to_vec();
            position = start;
        }

        if position == 0 && events.len() < limit {
            push_reverse_event(&pending, kind, &mut events)?;
        }
        Ok(events)
    })();
    let unlock_result = FileExt::unlock(&file);
    let events = result?;
    unlock_result?;
    Ok(events)
}

fn push_reverse_event(line: &[u8], kind: Option<&str>, events: &mut Vec<Event>) -> io::Result<()> {
    let line = trim_ascii(line);
    if line.is_empty() {
        return Ok(());
    }
    let event: Event = serde_json::from_slice(line).map_err(io::Error::other)?;
    if event_matches_kind(&event, kind) {
        events.push(event);
    }
    Ok(())
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

fn event_matches_kind(event: &Event, kind: Option<&str>) -> bool {
    match kind.map(str::trim).filter(|value| !value.is_empty()) {
        None => true,
        Some("chat") => event.kind == "chat.message",
        Some(expected) => event.kind == expected,
    }
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
    fn filtered_tail_reads_from_end_and_reports_more_matches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        for kind in [
            "chat.message",
            "actor.activity",
            "chat.message",
            "chat.read",
            "chat.message",
        ] {
            append(&path, &Event::new(kind, "g_test")).expect("append");
        }

        let (events, has_more) = tail_filtered(&path, 2, Some("chat")).expect("filtered tail");

        assert!(has_more);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| event.kind == "chat.message"));
        assert!(events[0].ts <= events[1].ts);
    }

    #[test]
    fn filtered_tail_only_reads_archives_when_active_file_is_insufficient() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let segments = temp.path().join("state/ledger/segments");
        std::fs::create_dir_all(&segments).expect("segments");
        let archived = segments.join("ledger.0001.jsonl");
        append(&archived, &Event::new("chat.message", "g_test")).expect("append archive");
        append(&path, &Event::new("actor.activity", "g_test")).expect("append activity");
        append(&path, &Event::new("chat.message", "g_test")).expect("append active");

        let (events, has_more) = tail_filtered(&path, 2, Some("chat")).expect("filtered tail");

        assert!(!has_more);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "chat.message");
        assert_eq!(events[1].kind, "chat.message");
    }

    #[test]
    fn events_after_returns_reconnect_replay_in_order() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let first = Event::new("chat.message", "g_test");
        let second = Event::new("chat.message", "g_test");
        let third = Event::new("chat.message", "g_test");
        append(&path, &first).expect("append first");
        append(&path, &second).expect("append second");
        append(&path, &third).expect("append third");

        let replay = events_after(&path, &first.id, 10).expect("events after");

        assert_eq!(
            replay.iter().map(|event| &event.id).collect::<Vec<_>>(),
            vec![&second.id, &third.id]
        );
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
