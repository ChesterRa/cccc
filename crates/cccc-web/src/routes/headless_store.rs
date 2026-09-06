use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const REPLAY_LINE_LIMIT: usize = 400;
const REVERSE_READ_CHUNK_BYTES: usize = 64 * 1024;
const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
const MAX_INCREMENTAL_READ_BYTES: usize = 1024 * 1024;
const MAX_EVENT_LINE_BYTES: usize = 8 * 1024 * 1024;

const REPLAY_START_TYPES: &[&str] = &[
    "headless.turn.started",
    "headless.control.queued",
    "headless.control.started",
    "headless.control.requeued",
];
const REPLAY_END_TYPES: &[&str] = &[
    "headless.turn.completed",
    "headless.turn.failed",
    "headless.control.completed",
    "headless.control.failed",
];

pub struct HeadlessEventTail {
    path: PathBuf,
    offset: u64,
    pending: Vec<u8>,
    dropping_oversized_line: bool,
}

impl HeadlessEventTail {
    pub fn open(path: PathBuf, replay: bool) -> io::Result<(Self, Vec<Value>)> {
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((
                    Self {
                        path,
                        offset: 0,
                        pending: Vec::new(),
                        dropping_oversized_line: false,
                    },
                    Vec::new(),
                ));
            }
            Err(error) => return Err(error),
        };
        let offset = file.metadata()?.len();
        let (pending, dropping_oversized_line) = unfinished_tail(&mut file, offset)?;
        let replay_end = offset.saturating_sub(pending.len() as u64);
        let events = if replay {
            replay_events_from_file(&mut file, replay_end, REPLAY_LINE_LIMIT)?
        } else {
            Vec::new()
        };
        Ok((
            Self {
                path,
                offset,
                pending,
                dropping_oversized_line,
            },
            events,
        ))
    }

    pub fn read_new(&mut self) -> io::Result<Vec<Value>> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let len = file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
            self.pending.clear();
            self.dropping_oversized_line = false;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let remaining = len.saturating_sub(self.offset);
        let read_len = remaining.min(MAX_INCREMENTAL_READ_BYTES as u64) as usize;
        let mut appended = vec![0; read_len];
        file.read_exact(&mut appended)?;
        self.offset += appended.len() as u64;
        if appended.is_empty() {
            return Ok(Vec::new());
        }

        if self.dropping_oversized_line {
            if let Some(newline) = appended.iter().position(|byte| *byte == b'\n') {
                self.dropping_oversized_line = false;
                self.pending.extend_from_slice(&appended[newline + 1..]);
            }
        } else {
            self.pending.extend_from_slice(&appended);
        }
        if self.pending.len() > MAX_EVENT_LINE_BYTES && !self.pending.contains(&b'\n') {
            self.pending.clear();
            self.dropping_oversized_line = true;
            return Ok(Vec::new());
        }
        let complete_len = self
            .pending
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        if complete_len == 0 {
            return Ok(Vec::new());
        }
        let complete = self.pending.drain(..complete_len).collect::<Vec<_>>();
        Ok(parse_json_lines(&complete))
    }
}

// A writer can be between its JSON and newline writes when a subscriber opens.
// Keep that unfinished record for the live tail instead of losing its prefix.
fn unfinished_tail(file: &mut File, end: u64) -> io::Result<(Vec<u8>, bool)> {
    let mut position = end;
    let mut chunks = Vec::new();
    let mut length = 0;
    while position > 0 {
        let size = position.min(REVERSE_READ_CHUNK_BYTES as u64) as usize;
        position -= size as u64;
        file.seek(SeekFrom::Start(position))?;
        let mut chunk = vec![0; size];
        file.read_exact(&mut chunk)?;
        let newline = chunk.iter().rposition(|byte| *byte == b'\n');
        if let Some(index) = newline {
            chunk.drain(..=index);
        }
        length += chunk.len();
        if length > MAX_EVENT_LINE_BYTES {
            return Ok((Vec::new(), true));
        }
        chunks.push(chunk);
        if newline.is_some() {
            break;
        }
    }
    chunks.reverse();
    Ok((chunks.concat(), false))
}

fn replay_events_from_file(file: &mut File, end: u64, limit: usize) -> io::Result<Vec<Value>> {
    if end == 0 || limit == 0 {
        return Ok(Vec::new());
    }
    let mut position = end;
    let mut newline_count = 0usize;
    let mut chunks = Vec::new();
    let mut bytes_read = 0usize;
    while position > 0 && newline_count <= limit && bytes_read < MAX_REPLAY_BYTES {
        let chunk_len = position.min(REVERSE_READ_CHUNK_BYTES as u64) as usize;
        let chunk_len = chunk_len.min(MAX_REPLAY_BYTES - bytes_read);
        position -= chunk_len as u64;
        file.seek(SeekFrom::Start(position))?;
        let mut chunk = vec![0; chunk_len];
        file.read_exact(&mut chunk)?;
        newline_count += chunk.iter().filter(|byte| **byte == b'\n').count();
        bytes_read += chunk_len;
        chunks.push(chunk);
    }
    chunks.reverse();
    let bytes = chunks.concat();
    let mut lines = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        .collect::<Vec<_>>();
    if position > 0 && !bytes.starts_with(b"\n") && !lines.is_empty() {
        lines.remove(0);
    }
    let start = lines.len().saturating_sub(limit);
    Ok(project_replay_events(
        lines[start..]
            .iter()
            .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
            .collect(),
    ))
}

fn parse_json_lines(bytes: &[u8]) -> Vec<Value> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        .filter(|line| line.len() <= MAX_EVENT_LINE_BYTES)
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

fn project_replay_events(events: Vec<Value>) -> Vec<Value> {
    let mut active_start = BTreeMap::<String, usize>::new();
    let mut latest_completed_start = BTreeMap::<String, usize>::new();
    let mut latest_seen_start = BTreeMap::<String, usize>::new();
    let mut first_seen = BTreeMap::<String, usize>::new();

    for (index, event) in events.iter().enumerate() {
        let Some(actor_id) = event
            .get("actor_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let Some(event_type) = event
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| !kind.is_empty())
        else {
            continue;
        };
        first_seen.entry(actor_id.to_owned()).or_insert(index);
        if REPLAY_START_TYPES.contains(&event_type) {
            active_start.insert(actor_id.to_owned(), index);
            latest_seen_start.insert(actor_id.to_owned(), index);
        } else if REPLAY_END_TYPES.contains(&event_type) {
            let start = active_start
                .remove(actor_id)
                .or_else(|| latest_seen_start.get(actor_id).copied())
                .or_else(|| first_seen.get(actor_id).copied())
                .unwrap_or(index);
            latest_completed_start.insert(actor_id.to_owned(), start);
        }
    }

    let mut replay_start = latest_completed_start;
    replay_start.extend(active_start);
    if replay_start.is_empty() {
        return Vec::new();
    }
    events
        .into_iter()
        .enumerate()
        .filter(|(index, event)| {
            event
                .get("actor_id")
                .and_then(Value::as_str)
                .and_then(|actor_id| replay_start.get(actor_id))
                .is_some_and(|start| index >= start)
        })
        .map(|(_, event)| event)
        .collect()
}

pub fn read_replay_events(path: &Path) -> io::Result<Vec<Value>> {
    HeadlessEventTail::open(path.to_path_buf(), true).map(|(_, events)| events)
}

#[cfg(test)]
mod tests {
    use super::{HeadlessEventTail, read_replay_events};
    use serde_json::{Value, json};
    use std::fs::OpenOptions;
    use std::io::Write;

    fn append(path: &std::path::Path, value: &Value) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open headless event fixture");
        writeln!(
            file,
            "{}",
            serde_json::to_string(value).expect("serialize headless event fixture")
        )
        .expect("append headless event fixture");
    }

    #[test]
    fn replay_keeps_only_the_latest_turn_per_actor_from_a_bounded_tail() {
        let dir = tempfile::tempdir().expect("create temporary group");
        let path = dir.path().join("events.jsonl");
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.turn.started","data":{"turn":1}}),
        );
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.message.delta","data":{"text":"old"}}),
        );
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.turn.completed","data":{}}),
        );
        append(
            &path,
            &json!({"actor_id":"b","type":"headless.turn.started","data":{}}),
        );
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.turn.started","data":{"turn":2}}),
        );
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.message.delta","data":{"text":"new"}}),
        );

        let events = read_replay_events(&path).expect("read replay events");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["actor_id"], "b");
        assert_eq!(events[1]["data"]["turn"], 2);
        assert_eq!(events[2]["data"]["text"], "new");
    }

    #[test]
    fn opening_between_json_and_newline_retains_the_unfinished_record() {
        let dir = tempfile::tempdir().expect("temporary group");
        let path = dir.path().join("events.jsonl");
        append(
            &path,
            &json!({"id":"start","actor_id":"a","type":"headless.turn.started"}),
        );
        let pending = json!({"id":"next","actor_id":"a","type":"headless.message.delta","data":{"delta":"hello"}}).to_string();
        let midpoint = pending.len() / 2;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("headless boundary fixture");
        file.write_all(&pending.as_bytes()[..midpoint])
            .expect("headless boundary fixture");
        let (mut tail, snapshot) =
            HeadlessEventTail::open(path, true).expect("headless boundary fixture");
        assert_eq!(snapshot.len(), 1);
        file.write_all(&pending.as_bytes()[midpoint..])
            .expect("headless boundary fixture");
        assert!(tail.read_new().expect("read incremental tail").is_empty());
        file.write_all(b"\n").expect("headless boundary fixture");
        let live = tail.read_new().expect("read incremental tail");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0]["id"], "next");
        assert!(tail.read_new().expect("read incremental tail").is_empty());
    }

    #[test]
    fn snapshot_and_incremental_tail_share_one_boundary() {
        let dir = tempfile::tempdir().expect("temporary group");
        let path = dir.path().join("events.jsonl");
        append(
            &path,
            &json!({"id":"old","actor_id":"a","type":"headless.turn.started"}),
        );
        let (mut tail, snapshot) =
            HeadlessEventTail::open(path.clone(), true).expect("headless boundary fixture");
        append(
            &path,
            &json!({"id":"new","actor_id":"a","type":"headless.message.delta"}),
        );
        assert_eq!(snapshot[0]["id"], "old");
        assert_eq!(
            tail.read_new().expect("read incremental tail")[0]["id"],
            "new"
        );
        assert!(tail.read_new().expect("read incremental tail").is_empty());
    }

    #[test]
    fn incremental_tail_reads_only_complete_appended_lines() {
        let dir = tempfile::tempdir().expect("create temporary group");
        let path = dir.path().join("events.jsonl");
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.turn.started"}),
        );
        let (mut tail, _) =
            HeadlessEventTail::open(path.clone(), false).expect("open headless event tail");
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.message.delta"}),
        );
        assert_eq!(tail.read_new().expect("read appended event").len(), 1);
        assert!(tail.read_new().expect("read empty tail").is_empty());
    }

    #[test]
    fn incremental_tail_catches_up_in_bounded_chunks_without_losing_events() {
        let dir = tempfile::tempdir().expect("create temporary group");
        let path = dir.path().join("events.jsonl");
        append(
            &path,
            &json!({"actor_id":"a","type":"headless.turn.started"}),
        );
        let (mut tail, _) =
            HeadlessEventTail::open(path.clone(), false).expect("open headless event tail");
        let text = "x".repeat(600_000);
        for index in 0..2 {
            append(
                &path,
                &json!({"actor_id":"a","type":"headless.message.delta","data":{"index":index,"text":text}}),
            );
        }

        let first = tail.read_new().expect("first bounded read");
        let second = tail.read_new().expect("second bounded read");
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0]["data"]["index"], 0);
        assert_eq!(second[0]["data"]["index"], 1);
    }

    #[test]
    fn replay_never_scans_before_the_bounded_tail_window() {
        let dir = tempfile::tempdir().expect("create temporary group");
        let path = dir.path().join("events.jsonl");
        append(
            &path,
            &json!({"actor_id":"old","type":"headless.turn.started"}),
        );
        for index in 0..450 {
            append(
                &path,
                &json!({"actor_id":"noise","type":"headless.message.delta","data":{"index":index}}),
            );
        }
        append(
            &path,
            &json!({"actor_id":"current","type":"headless.turn.started"}),
        );
        append(
            &path,
            &json!({"actor_id":"current","type":"headless.message.delta"}),
        );

        let events = read_replay_events(&path).expect("read bounded replay events");
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| event["actor_id"] == "current"));
    }
}
