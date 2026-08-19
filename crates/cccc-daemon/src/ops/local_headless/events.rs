use cccc_contracts::utc_now;
use cccc_core::{GroupStore, HomeLayout};
use fs2::FileExt;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::process;
use uuid::Uuid;

const DEDUPE_SCAN_BYTES: u64 = 256 * 1024;
const DEDUPE_SCAN_LINES: usize = 4096;
const DEDUPE_READY: &str = "index.ready";
const DEDUPE_PENDING: &str = "pending.json";
const MAX_PENDING_LINE_BYTES: u64 = 1024 * 1024;

pub(crate) fn append(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    kind: &str,
    data: Map<String, Value>,
) -> io::Result<()> {
    append_with_dedupe(home, group_id, actor_id, kind, data, None)
}

pub(crate) fn append_with_dedupe(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    kind: &str,
    data: Map<String, Value>,
    dedupe_key: Option<&str>,
) -> io::Result<()> {
    let store = GroupStore::new(home.clone())?;
    let directory = store.state_dir(group_id)?.join("headless");
    fs::create_dir_all(&directory)?;
    let path = directory.join("events.jsonl");
    let mut events = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&path)?;
    let lock_path = directory.join("events.lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)?;
    lock.lock_exclusive()?;
    let result = (|| {
        let marker_dir = directory.join("events.dedupe");
        fs::create_dir_all(&marker_dir)?;
        recover_pending(&path, &marker_dir)?;
        if let Some(key) = dedupe_key.filter(|value| !value.is_empty()) {
            ensure_dedupe_index(&path, &marker_dir)?;
            let marker = marker_dir.join(dedupe_marker_name(key));
            if marker.exists() {
                if marker_matches(&marker, key) {
                    return Ok(());
                }
                if repair_marker_from_events(&path, &marker, key)? {
                    return Ok(());
                }
                return Err(io::Error::other("deepseek dedupe marker is invalid"));
            }
            let payload = event_payload(group_id, actor_id, kind, data, Some(key));
            let line = serialize_event_line(&payload)?;
            let offset = events.seek(SeekFrom::End(0))?;
            write_pending_atomic(&marker_dir, key, offset, &line, &payload)?;
            append_event_line(&mut events, &line)?;
            write_dedupe_marker_atomic(&marker, key)?;
            fs::remove_file(marker_dir.join(DEDUPE_PENDING)).ok();
            return Ok(());
        }
        append_payload(
            &mut events,
            &event_payload(group_id, actor_id, kind, data, None),
        )
    })();
    FileExt::unlock(&lock).ok();
    result
}

pub(crate) fn contains_dedupe(home: &HomeLayout, group_id: &str, key: &str) -> io::Result<bool> {
    if key.is_empty() {
        return Ok(false);
    }
    let store = GroupStore::new(home.clone())?;
    let directory = store.state_dir(group_id)?.join("headless");
    let path = directory.join("events.jsonl");
    if !path.is_file() {
        return Ok(false);
    }
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(directory.join("events.lock"))?;
    lock.lock_exclusive()?;
    let result = (|| {
        let marker_dir = directory.join("events.dedupe");
        fs::create_dir_all(&marker_dir)?;
        recover_pending(&path, &marker_dir)?;
        ensure_dedupe_index(&path, &marker_dir)?;
        Ok(marker_matches(
            &marker_dir.join(dedupe_marker_name(key)),
            key,
        ))
    })();
    FileExt::unlock(&lock).ok();
    result
}

fn event_payload(
    group_id: &str,
    actor_id: &str,
    kind: &str,
    data: Map<String, Value>,
    dedupe_key: Option<&str>,
) -> Value {
    json!({
        "id": Uuid::new_v4().simple().to_string(),
        "ts": utc_now(),
        "group_id": group_id,
        "actor_id": actor_id,
        "type": kind,
        "dedupe_key": dedupe_key,
        "data": data,
    })
}

fn append_payload(file: &mut std::fs::File, payload: &Value) -> io::Result<()> {
    let line = serialize_event_line(payload)?;
    append_event_line(file, &line)
}

fn serialize_event_line(payload: &Value) -> io::Result<Vec<u8>> {
    serde_json::to_vec(payload).map_err(io::Error::other)
}

fn append_event_line(file: &mut std::fs::File, line: &[u8]) -> io::Result<()> {
    file.write_all(line)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_data()
}

fn recover_pending(path: &std::path::Path, marker_dir: &std::path::Path) -> io::Result<()> {
    let pending_path = marker_dir.join(DEDUPE_PENDING);
    if !pending_path.exists() {
        return Ok(());
    }
    let pending: Value = serde_json::from_slice(&fs::read(&pending_path)?)
        .map_err(|_| io::Error::other("invalid deepseek dedupe pending record"))?;
    if pending.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err(io::Error::other(
            "unsupported deepseek dedupe pending schema",
        ));
    }
    let key = pending
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("pending dedupe key missing"))?;
    let payload = pending
        .get("event")
        .ok_or_else(|| io::Error::other("pending dedupe event missing"))?;
    let offset = pending
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other("pending dedupe offset missing"))?;
    let line = pending
        .get("line")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("pending dedupe line missing"))?;
    let line_bytes = line.as_bytes();
    let line_len = pending
        .get("line_len")
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other("pending dedupe line length missing"))?;
    if line_len != line_bytes.len() as u64 || line_len > MAX_PENDING_LINE_BYTES {
        return Err(io::Error::other("pending dedupe line length invalid"));
    }
    let event_id = pending
        .get("event_id")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("pending dedupe event id missing"))?;
    if payload.get("id").and_then(Value::as_str) != Some(event_id)
        || payload.get("dedupe_key").and_then(Value::as_str) != Some(key)
        || serde_json::from_slice::<Value>(line_bytes).ok().as_ref() != Some(payload)
    {
        return Err(io::Error::other("pending dedupe identity mismatch"));
    }
    let marker = marker_dir.join(dedupe_marker_name(key));
    ensure_event_at_offset(path, offset, line_bytes, payload, key)?;
    if !marker_matches(&marker, key) {
        write_dedupe_marker_atomic(&marker, key)?;
    }
    fs::remove_file(pending_path).ok();
    Ok(())
}

fn ensure_event_at_offset(
    path: &std::path::Path,
    offset: u64,
    line: &[u8],
    payload: &Value,
    key: &str,
) -> io::Result<()> {
    let size = path.metadata()?.len();
    if offset > size {
        return Err(io::Error::other("pending dedupe offset beyond event log"));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    if offset == size {
        file.seek(SeekFrom::End(0))?;
        append_event_line(&mut file, line)?;
        return Ok(());
    }
    let needed = offset
        .checked_add(line.len() as u64)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| io::Error::other("pending dedupe offset overflow"))?;
    if needed > size {
        return Err(io::Error::other("pending dedupe event is truncated"));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut actual = vec![0u8; line.len() + 1];
    file.read_exact(&mut actual)?;
    if actual[..line.len()] != line[..] || actual[line.len()] != b'\n' {
        return Err(io::Error::other("pending dedupe event diverged"));
    }
    let parsed: Value = serde_json::from_slice(line).map_err(io::Error::other)?;
    if parsed != *payload
        || parsed.get("id").and_then(Value::as_str) != payload.get("id").and_then(Value::as_str)
        || parsed.get("dedupe_key").and_then(Value::as_str) != Some(key)
    {
        return Err(io::Error::other("pending dedupe event identity mismatch"));
    }
    Ok(())
}

fn find_event(path: &std::path::Path, key: &str, event_id: Option<&str>) -> io::Result<bool> {
    if path.metadata()?.len() > DEDUPE_SCAN_BYTES {
        return Ok(false);
    }
    let mut text = String::new();
    OpenOptions::new()
        .read(true)
        .open(path)?
        .read_to_string(&mut text)?;
    if text.lines().count() > DEDUPE_SCAN_LINES {
        return Ok(false);
    }
    Ok(text.lines().any(|line| {
        serde_json::from_str::<Value>(line)
            .ok()
            .is_some_and(|value| {
                value.get("dedupe_key").and_then(Value::as_str) == Some(key)
                    || event_id
                        .is_some_and(|id| value.get("id").and_then(Value::as_str) == Some(id))
            })
    }))
}

fn marker_matches(path: &std::path::Path, key: &str) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| value.lines().next().map(str::to_owned))
        .is_some_and(|value| value == key)
}

fn ensure_dedupe_index(path: &std::path::Path, marker_dir: &std::path::Path) -> io::Result<()> {
    let ready = marker_dir.join(DEDUPE_READY);
    if ready.exists() {
        return Ok(());
    }
    super::events_migration::scan_dedupe_keys(path, |key| {
        let marker = marker_dir.join(dedupe_marker_name(key));
        if !marker_matches(&marker, key) {
            write_dedupe_marker_atomic(&marker, key)?;
        }
        Ok(())
    })?;
    write_dedupe_marker_atomic(&ready, "ready")
}

fn repair_marker_from_events(
    path: &std::path::Path,
    marker: &std::path::Path,
    key: &str,
) -> io::Result<bool> {
    if find_event(path, key, None)? {
        write_dedupe_marker_atomic(marker, key)?;
        return Ok(true);
    }
    Ok(false)
}

fn write_pending_atomic(
    marker_dir: &std::path::Path,
    key: &str,
    offset: u64,
    line: &[u8],
    payload: &Value,
) -> io::Result<()> {
    let pending = marker_dir.join(DEDUPE_PENDING);
    let temp = marker_dir.join(format!("pending.tmp-{}", process::id()));
    let line = std::str::from_utf8(line)
        .map_err(|_| io::Error::other("pending dedupe line is not UTF-8"))?;
    let event_id = payload
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("pending dedupe event id missing"))?;
    let value = json!({
        "schema": 1,
        "key": key,
        "event_id": event_id,
        "offset": offset,
        "line_len": line.len(),
        "line": line,
        "event": payload
    });
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp)?;
    serde_json::to_writer(&mut file, &value).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_all()?;
    fs::rename(temp, pending)
}

fn dedupe_marker_name(key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(key.as_bytes());
    format!("{:x}.marker", digest.finalize())
}

fn write_dedupe_marker_atomic(path: &std::path::Path, key: &str) -> io::Result<()> {
    let temp = path.with_extension(format!("tmp-{}", process::id()));
    let mut marker = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp)?;
    marker.write_all(key.as_bytes())?;
    marker.write_all(b"\n")?;
    marker.flush()?;
    marker.sync_all()?;
    fs::rename(temp, path)
}
