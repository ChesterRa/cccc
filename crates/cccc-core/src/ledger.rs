use cccc_contracts::Event;
use flate2::read::GzDecoder;
use fs2::FileExt;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRevision {
    pub(crate) path: PathBuf,
    pub(crate) len: u64,
    pub(crate) modified: Option<SystemTime>,
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
        let group_id = ledger_group_id(path);
        let revisions = revisions(path)?;
        let mut next_sources: BTreeMap<_, _> = revisions
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
                let (events, consumed_end) = read_source_from(&revision.path, offset, &group_id)?;
                appended.extend(events);
                if let Some(source) = next_sources.get_mut(&revision.path) {
                    source.len = consumed_end;
                }
            }
        }
        self.sources = next_sources;
        Ok(appended)
    }
}

pub fn append(path: &Path, event: &Event) -> io::Result<()> {
    append_with(path, event, append_locked)
}

fn append_with(
    path: &Path,
    event: &Event,
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let lock_path = ledger_lock_path(path);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    if lock.metadata()?.len() == 0 {
        lock.write_all(&[0])?;
        lock.flush()?;
    }
    lock.seek(SeekFrom::Start(0))?;
    lock.lock_exclusive()?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    let original_len = file.metadata()?.len();
    let mut encoded = serde_json::to_vec(event).map_err(io::Error::other)?;
    encoded.push(b'\n');
    let result = write(&mut file, &encoded);
    if let Err(error) = result {
        if matches!(
            appended_bytes_match(&mut file, original_len, &encoded),
            Ok(true)
        ) {
            let _ = FileExt::unlock(&lock);
            crate::ledger_index::note_append(path, event, encoded.len());
            return Ok(());
        }
        file.set_len(original_len).map_err(|rollback| {
            io::Error::other(format!(
                "{error}; rollback_failed: could not truncate incomplete ledger append: {rollback}"
            ))
        })?;
        file.sync_data().map_err(|rollback| {
            io::Error::other(format!(
                "{error}; rollback_failed: could not sync ledger rollback: {rollback}"
            ))
        })?;
        let _ = FileExt::unlock(&lock);
        return Err(error);
    }
    let _ = FileExt::unlock(&lock);
    crate::ledger_index::note_append(path, event, encoded.len());
    Ok(())
}

fn ledger_lock_path(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new(""))
        .join("state/ledger/ledger.lock")
}

fn appended_bytes_match(file: &mut File, original_len: u64, expected: &[u8]) -> io::Result<bool> {
    let expected_len = u64::try_from(expected.len()).map_err(io::Error::other)?;
    if file.metadata()?.len() != original_len.saturating_add(expected_len) {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(original_len))?;
    let mut actual = vec![0; expected.len()];
    file.read_exact(&mut actual)?;
    Ok(actual == expected)
}

fn append_locked(file: &mut File, encoded: &[u8]) -> io::Result<()> {
    file.write_all(encoded)?;
    file.sync_data()
}

pub fn read_all(path: &Path) -> io::Result<Vec<Event>> {
    crate::ledger_index::inspect(path, |events, _| events.to_vec())
}

pub(crate) fn read_all_uncached(path: &Path) -> io::Result<Vec<Event>> {
    let group_id = ledger_group_id(path);
    let mut events = Vec::new();
    for source in source_paths(path)? {
        events.extend(read_source(&source, &group_id)?);
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

pub(crate) fn revisions(path: &Path) -> io::Result<Vec<SourceRevision>> {
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

fn read_source(path: &Path, group_id: &str) -> io::Result<Vec<Event>> {
    if is_gzip(path) {
        return read_events(
            BufReader::new(GzDecoder::new(File::open(path)?)),
            path,
            group_id,
        );
    }
    read_source_from(path, 0, group_id).map(|(events, _)| events)
}

fn read_source_from(path: &Path, offset: u64, group_id: &str) -> io::Result<(Vec<Event>, u64)> {
    if is_gzip(path) {
        return if offset == 0 {
            read_source(path, group_id)
                .and_then(|events| std::fs::metadata(path).map(|metadata| (events, metadata.len())))
        } else {
            Ok((Vec::new(), std::fs::metadata(path)?.len()))
        };
    }
    let mut file = File::open(path)?;
    FileExt::lock_shared(&file)?;
    file.seek(io::SeekFrom::Start(offset))?;
    let result = read_events_from(BufReader::new(&file), path, group_id, offset);
    let unlock = FileExt::unlock(&file);
    result.and_then(|events| unlock.map(|()| events))
}

fn read_events_from(
    mut reader: impl BufRead,
    source: &Path,
    group_id: &str,
    offset: u64,
) -> io::Result<(Vec<Event>, u64)> {
    let mut events = Vec::new();
    let mut line = Vec::new();
    let mut consumed = 0_u64;
    let mut line_no = 0;
    loop {
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        line_no += 1;
        let terminated = line.last() == Some(&b'\n');
        let raw = trim_ascii(&line);
        if !terminated && !raw.is_empty() && serde_json::from_slice::<Value>(raw).is_err() {
            break;
        }
        if !raw.is_empty()
            && let Some(event) = decode_event_line(raw, source, line_no, group_id)
        {
            events.push(event);
        }
        consumed += read as u64;
        line.clear();
    }
    Ok((events, offset + consumed))
}

fn read_events(mut reader: impl BufRead, source: &Path, group_id: &str) -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    let mut line = Vec::new();
    let mut line_no = 0;
    while reader.read_until(b'\n', &mut line)? != 0 {
        line_no += 1;
        let raw = trim_ascii(&line);
        if raw.is_empty() {
            line.clear();
            continue;
        }
        if let Some(event) = decode_event_line(raw, source, line_no, group_id) {
            events.push(event);
        }
        line.clear();
    }
    Ok(events)
}

fn decode_event_line(raw: &[u8], source: &Path, line_no: usize, group_id: &str) -> Option<Event> {
    match serde_json::from_slice(raw) {
        Ok(event) => Some(event),
        Err(error) => match serde_json::from_slice::<Value>(raw) {
            Ok(value) => normalize_legacy_event(&value, raw, group_id).or_else(|| {
                tracing::warn!(
                    source = %source.display(),
                    line = line_no,
                    %error,
                    "skipping unrecognized ledger event"
                );
                None
            }),
            Err(json_error) => {
                tracing::warn!(
                    source = %source.display(),
                    line = line_no,
                    error = %json_error,
                    "skipping malformed ledger line"
                );
                None
            }
        },
    }
}

fn normalize_legacy_event(value: &Value, raw: &[u8], group_id: &str) -> Option<Event> {
    let object = value.as_object()?;
    if object.get("type").and_then(Value::as_str) != Some("chat.ack") {
        return None;
    }
    let target_event_id = nonempty_string(object, "event_id")?;
    let actor_id = nonempty_string(object, "agent")?;
    let mut event = Event::new("chat.ack", group_id);
    event.id = legacy_event_id(raw);
    if let Some(ts) = object
        .get("ts")
        .and_then(Value::as_str)
        .filter(|ts| !ts.is_empty())
    {
        event.ts = ts.to_owned();
    }
    event.by = actor_id.to_owned();
    event.data = Map::from_iter([
        ("actor_id".into(), json!(actor_id)),
        ("event_id".into(), json!(target_event_id)),
    ]);
    Some(event)
}

fn nonempty_string<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    let value = object.get(key)?.as_str()?.trim();
    (!value.is_empty()).then_some(value)
}

fn legacy_event_id(raw: &[u8]) -> String {
    let digest = Sha256::digest(raw);
    format!("{digest:x}")[..32].to_owned()
}

fn ledger_group_id(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned()
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
    let group_id = ledger_group_id(path);
    for source in source_paths(path)?.iter().rev() {
        let remaining = target.saturating_sub(newest_first.len());
        if remaining == 0 {
            break;
        }
        newest_first.extend(read_source_reverse(source, remaining, kind, &group_id)?);
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
    crate::ledger_index::inspect(path, |events, positions| {
        let Some(index) = positions.get(event_id).copied() else {
            return Vec::new();
        };
        events
            .iter()
            .skip(index.saturating_add(1))
            .take(limit)
            .cloned()
            .collect()
    })
}

pub fn inspect<T>(
    path: &Path,
    inspect: impl FnOnce(&[Event], &std::collections::HashMap<String, usize>) -> T,
) -> io::Result<T> {
    crate::ledger_index::inspect(path, inspect)
}

pub fn inspect_status<T>(
    path: &Path,
    inspect: impl FnOnce(
        &[Event],
        &std::collections::HashMap<String, usize>,
        &std::collections::HashMap<String, std::collections::BTreeSet<String>>,
        &std::collections::HashMap<String, std::collections::BTreeSet<String>>,
    ) -> T,
) -> io::Result<T> {
    crate::ledger_index::inspect_status(path, inspect)
}

pub fn find_event(path: &Path, event_id: &str) -> io::Result<Option<Event>> {
    crate::ledger_index::find_event(path, event_id)
}

pub fn find_idempotent(
    path: &Path,
    kind: &str,
    by: &str,
    client_id: &str,
) -> io::Result<Option<Event>> {
    crate::ledger_index::find_idempotent(path, kind, by, client_id)
}

pub fn find_relay(path: &Path, source_event_id: &str) -> io::Result<Option<Event>> {
    crate::ledger_index::find_relay(path, source_event_id)
}

fn read_source_reverse(
    path: &Path,
    limit: usize,
    kind: Option<&str>,
    group_id: &str,
) -> io::Result<Vec<Event>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if is_gzip(path) {
        return read_gzip_tail(path, limit, kind, group_id);
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
                push_reverse_event(
                    &buffer[newline + 1..line_end],
                    path,
                    kind,
                    group_id,
                    &mut events,
                );
                line_end = newline;
            }
            pending = buffer[..line_end].to_vec();
            position = start;
        }

        if position == 0 && events.len() < limit {
            push_reverse_event(&pending, path, kind, group_id, &mut events);
        }
        Ok(events)
    })();
    let unlock_result = FileExt::unlock(&file);
    let events = result?;
    unlock_result?;
    Ok(events)
}

fn read_gzip_tail(
    path: &Path,
    limit: usize,
    kind: Option<&str>,
    group_id: &str,
) -> io::Result<Vec<Event>> {
    let mut reader = BufReader::new(GzDecoder::new(File::open(path)?));
    let mut retained = VecDeque::with_capacity(limit);
    let mut line = Vec::new();
    let mut line_no = 0usize;
    while reader.read_until(b'\n', &mut line)? > 0 {
        line_no += 1;
        let raw = trim_ascii(&line);
        if !raw.is_empty()
            && let Some(event) = decode_event_line(raw, path, line_no, group_id)
            && event_matches_kind(&event, kind)
        {
            if retained.len() == limit {
                retained.pop_front();
            }
            retained.push_back(event);
        }
        line.clear();
    }
    Ok(retained.into_iter().rev().collect())
}

fn push_reverse_event(
    line: &[u8],
    source: &Path,
    kind: Option<&str>,
    group_id: &str,
    events: &mut Vec<Event>,
) {
    let line = trim_ascii(line);
    if line.is_empty() {
        return;
    }
    if let Some(event) = decode_event_line(line, source, 0, group_id)
        && event_matches_kind(&event, kind)
    {
        events.push(event);
    }
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
    fn failed_retry_rolls_back_partial_bytes_when_the_event_already_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let event = Event::new("chat.message", "g_test");
        append(&path, &event).expect("initial append");
        let original = std::fs::read(&path).expect("original ledger");

        let error = append_with(&path, &event, |file, encoded| {
            file.write_all(&encoded[..encoded.len() / 2])?;
            Err(io::Error::other("injected partial write failure"))
        })
        .expect_err("partial retry must fail");

        assert!(error.to_string().contains("injected partial write failure"));
        assert_eq!(
            std::fs::read(&path).expect("rolled back ledger"),
            original,
            "a historical matching event must not make a partial retry look committed"
        );
        assert_eq!(
            read_all_uncached(&path).expect("read rolled back ledger"),
            vec![event]
        );
    }

    #[test]
    fn failed_sync_is_success_when_this_attempt_wrote_the_exact_event() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let event = Event::new("chat.message", "g_test");

        append_with(&path, &event, |file, encoded| {
            file.write_all(encoded)?;
            Err(io::Error::other("injected sync failure"))
        })
        .expect("exact appended bytes are committed");

        assert_eq!(
            read_all_uncached(&path).expect("read committed ledger"),
            vec![event]
        );
    }

    #[test]
    fn reads_legacy_ack_from_gzip_segment_and_skips_unknown_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group = temp.path().join("g_test");
        let path = group.join("ledger.jsonl");
        let segments = group.join("state/ledger/segments");
        std::fs::create_dir_all(&segments).expect("segments");
        let archived = segments.join("ledger.0001.jsonl.gz");
        let file = File::create(&archived).expect("archive");
        let mut encoder = GzEncoder::new(file, Compression::default());
        writeln!(encoder, "{{\"type\":\"unknown.v0\",\"value\":1}}").expect("unknown");
        encoder.write_all(&[0xff, b'\n']).expect("malformed utf-8");
        writeln!(
            encoder,
            "{{\"ts\":\"2026-04-20T10:05:36Z\",\"type\":\"chat.ack\",\"event_id\":\"message-1\",\"agent\":\"reviewer\"}}"
        )
        .expect("legacy ack");
        encoder.finish().expect("finish archive");
        append(&path, &Event::new("chat.message", "g_test")).expect("active event");

        let first = read_all_uncached(&path).expect("read legacy archive");
        let second = read_all_uncached(&path).expect("read legacy archive again");

        assert_eq!(first.len(), 2);
        assert_eq!(
            first, second,
            "legacy IDs must be stable across index rebuilds"
        );
        let ack = &first[0];
        assert_eq!(ack.v, 1);
        assert_eq!(ack.kind, "chat.ack");
        assert_eq!(ack.group_id, "g_test");
        assert_eq!(ack.by, "reviewer");
        assert_eq!(ack.data["actor_id"], "reviewer");
        assert_eq!(ack.data["event_id"], "message-1");
        assert_eq!(ack.id.len(), 32);
    }

    #[test]
    fn reverse_tail_normalizes_legacy_ack_in_active_ledger() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group = temp.path().join("g_test");
        std::fs::create_dir_all(&group).expect("group");
        let path = group.join("ledger.jsonl");
        std::fs::write(
            &path,
            "{\"ts\":\"2026-04-20T10:05:36Z\",\"type\":\"chat.ack\",\"event_id\":\"message-1\",\"agent\":\"reviewer\"}\n",
        )
        .expect("legacy ledger");

        let events = tail_filtered(&path, 1, Some("chat.ack")).expect("tail legacy ack");

        assert_eq!(events.0.len(), 1);
        assert_eq!(events.0[0].data["actor_id"], "reviewer");
        assert!(!events.1);
    }

    #[test]
    fn forward_reads_wait_for_an_in_progress_append() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let event = Event::new("chat.message", "g_test");
        let encoded = serde_json::to_vec(&event).expect("encode event");
        let split = encoded.len() / 2;
        let mut writer = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)
            .expect("open ledger");
        writer.lock_exclusive().expect("lock writer");
        writer.write_all(&encoded[..split]).expect("partial append");
        writer.flush().expect("flush partial append");

        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let read_path = path.clone();
        let reader = std::thread::spawn(move || {
            sender.send(read_all(&read_path)).expect("send read result");
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(
            receiver.try_recv().is_err(),
            "reader observed a partial append"
        );

        writer.write_all(&encoded[split..]).expect("finish append");
        writer.write_all(b"\n").expect("append newline");
        writer.flush().expect("flush complete append");
        FileExt::unlock(&writer).expect("unlock writer");
        let events = receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("reader completed")
            .expect("read ledger");
        reader.join().expect("join reader");
        assert_eq!(events, vec![event]);
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
    fn cached_index_observes_api_appends() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let first = Event::new("chat.message", "g_test");
        let second = Event::new("chat.message", "g_test");
        append(&path, &first).expect("append first");
        assert_eq!(read_all(&path).expect("warm index").len(), 1);

        append(&path, &second).expect("append second");

        let events = read_all(&path).expect("read incrementally updated index");
        assert_eq!(events.len(), 2);
        assert_eq!(find_event(&path, &second.id).expect("find"), Some(second));
    }

    #[test]
    fn cached_index_invalidates_after_external_write() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let first = Event::new("chat.message", "g_test");
        let external = Event::new("actor.activity", "g_test");
        append(&path, &first).expect("append first");
        assert_eq!(read_all(&path).expect("warm index").len(), 1);
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open externally");
        serde_json::to_writer(&mut file, &external).expect("write external event");
        file.write_all(b"\n").expect("write newline");
        file.sync_data().expect("sync external event");

        let events = read_all(&path).expect("read invalidated index");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1], external);
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
    fn follower_returns_an_entire_burst_without_a_tail_window() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        let mut follower = LedgerFollower::default();
        follower.poll(&path).expect("initialize follower");
        for _ in 0..75 {
            append(&path, &Event::new("chat.message", "g_test")).expect("append");
        }

        assert_eq!(follower.poll(&path).expect("burst").len(), 75);
    }

    #[test]
    fn follower_retries_an_unterminated_partial_tail() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ledger.jsonl");
        append(&path, &Event::new("group.create", "g_test")).expect("append initial");
        let mut follower = LedgerFollower::default();
        follower.poll(&path).expect("initialize follower");
        let event = Event::new("chat.message", "g_test");
        let encoded = serde_json::to_vec(&event).expect("encode event");
        let split = encoded.len() / 2;
        let mut writer = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open external writer");

        writer
            .write_all(&encoded[..split])
            .expect("write partial event");
        writer.flush().expect("flush partial event");
        assert!(follower.poll(&path).expect("poll partial event").is_empty());

        writer.write_all(&encoded[split..]).expect("complete event");
        writer.write_all(b"\n").expect("terminate event");
        writer.flush().expect("flush complete event");
        assert_eq!(
            follower.poll(&path).expect("poll complete event"),
            vec![event]
        );
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
