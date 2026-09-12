use cccc_contracts::utc_now;
use flate2::read::GzDecoder;
use fs2::FileExt;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::fs::{read_json, write_json};
use crate::ledger;
use crate::{GroupStore, HomeLayout};

#[derive(Debug, Clone, Serialize)]
pub struct LedgerSnapshot {
    pub v: u8,
    pub group_id: String,
    pub created_at: String,
    pub reason: String,
    pub event_count: usize,
    pub last_event_id: String,
    pub sha256: String,
    pub path: String,
}

pub fn snapshot(home: &HomeLayout, group_id: &str, reason: &str) -> io::Result<LedgerSnapshot> {
    let store = GroupStore::new(home.clone())?;
    let ledger_path = store.ledger_path(group_id)?;
    let lock = ledger::acquire_writer_lock(&ledger_path)?;
    let result = snapshot_locked(&store, group_id, reason, &ledger_path);
    let unlock = FileExt::unlock(&lock);
    result.and_then(|value| unlock.map(|()| value))
}

fn snapshot_locked(
    store: &GroupStore,
    group_id: &str,
    reason: &str,
    ledger_path: &Path,
) -> io::Result<LedgerSnapshot> {
    // Validate and hash the canonical Event array in one pass. Maintenance
    // needs no random-access index and must not certify skipped bad records.
    let mut digest = Sha256::new();
    let mut event_count = 0;
    let mut last_event_id = String::new();
    {
        let mut writer = BufWriter::new(&mut digest);
        writer.write_all(b"[")?;
        ledger::visit_validated(ledger_path, |event| {
            if event_count > 0 {
                writer.write_all(b",")?;
            }
            serde_json::to_writer(&mut writer, &event).map_err(io::Error::other)?;
            event_count += 1;
            last_event_id = event.id;
            Ok(())
        })?;
        writer.write_all(b"]")?;
        writer.flush()?;
    }
    let sha256 = format!("{:x}", digest.finalize());
    let state = store.state_dir(group_id)?.join("ledger/snapshots");
    fs::create_dir_all(&state)?;
    let name = format!("{}.json", stamp());
    let path = state.join(&name);
    let snapshot = LedgerSnapshot {
        v: 1,
        group_id: group_id.into(),
        created_at: utc_now(),
        reason: reason.into(),
        event_count,
        last_event_id,
        sha256,
        path: format!("state/ledger/snapshots/{name}"),
    };
    write_json(&path, &snapshot)?;
    write_json(
        &store
            .state_dir(group_id)?
            .join("ledger/snapshot.latest.json"),
        &snapshot,
    )?;
    Ok(snapshot)
}

pub fn compact(home: &HomeLayout, group_id: &str, reason: &str) -> io::Result<Option<PathBuf>> {
    let store = GroupStore::new(home.clone())?;
    let active = store.ledger_path(group_id)?;
    let lock = ledger::acquire_writer_lock(&active)?;
    let result = (|| {
        if !active.exists() || active.metadata()?.len() == 0 {
            return Ok(None);
        }
        snapshot_locked(&store, group_id, reason, &active)?;
        let state = store.state_dir(group_id)?.join("ledger");
        let segments = state.join("segments");
        fs::create_dir_all(&segments)?;
        let manifest_path = state.join("manifest.json");
        let mut manifest = reconcile_manifest(load_manifest(&manifest_path)?, &segments)?;
        let sequence = manifest
            .get("next_segment_seq")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);
        let created_at = stamp();
        let name = format!("ledger.{created_at}.{sequence:06}.jsonl");
        let destination = segments.join(&name);
        fs::rename(&active, &destination)?;
        fs::File::create(&active)?.sync_all()?;
        let size = destination.metadata()?.len();
        let line_count = BufReader::new(fs::File::open(&destination)?)
            .lines()
            .try_fold(0_u64, |count, line| line.map(|_| count + 1))?;
        let segment = json!({
            "id":format!("{sequence:06}"),
            "seq":sequence,
            "path":format!("state/ledger/segments/{name}"),
            "compressed":false,
            "created_at":created_at,
            "sealed_at":created_at,
            "reason":reason,
            "size_bytes":size,
            "line_count":line_count,
        });
        manifest["schema"] = json!(1);
        manifest["active"] = json!({"path":"ledger.jsonl"});
        manifest["next_segment_seq"] = json!(sequence + 1);
        manifest["updated_at"] = json!(created_at);
        if !manifest["segments"].is_array() {
            manifest["segments"] = json!([]);
        }
        manifest["segments"]
            .as_array_mut()
            .expect("segments initialized")
            .push(segment);
        write_json(&manifest_path, &manifest)?;
        Ok(Some(destination))
    })();
    let _ = FileExt::unlock(&lock);
    result
}

fn stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn load_manifest(path: &std::path::Path) -> io::Result<Value> {
    if path.exists() {
        let manifest: Value = read_json(path)?;
        if manifest.is_object() {
            return Ok(manifest);
        }
        return Err(io::Error::other("ledger manifest must be an object"));
    }
    Ok(json!({
        "schema":1,
        "active":{"path":"ledger.jsonl"},
        "next_segment_seq":1,
        "segments":[],
        "updated_at":"",
    }))
}

#[derive(Debug)]
struct PhysicalSegment {
    sequence: u64,
    stamp: String,
    path: PathBuf,
    relative_path: String,
    compressed: bool,
    size_bytes: u64,
    line_count: u64,
}

fn reconcile_manifest(mut manifest: Value, segments_dir: &Path) -> io::Result<Value> {
    let discovered = discover_segments(segments_dir)?;
    let existing = manifest["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("seq")
                .and_then(Value::as_u64)
                .filter(|sequence| *sequence > 0)
                .map(|sequence| (sequence, item.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut reconciled = Vec::with_capacity(discovered.len());
    for segment in discovered.values() {
        let mut item = existing
            .get(&segment.sequence)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        item.insert("id".into(), json!(format!("{:06}", segment.sequence)));
        item.insert("seq".into(), json!(segment.sequence));
        item.insert("path".into(), json!(segment.relative_path));
        item.insert("compressed".into(), json!(segment.compressed));
        item.insert("size_bytes".into(), json!(segment.size_bytes));
        item.insert("line_count".into(), json!(segment.line_count));
        if item
            .get("created_at")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            item.insert("created_at".into(), json!(segment.stamp));
        }
        if item
            .get("sealed_at")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            item.insert("sealed_at".into(), json!(segment.stamp));
        }
        if item
            .get("reason")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            item.insert("reason".into(), json!("recovered"));
        }
        reconciled.push(Value::Object(item));
    }
    let physical_next = discovered
        .keys()
        .next_back()
        .copied()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1);
    let manifested_next = manifest
        .get("next_segment_seq")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1);
    manifest["schema"] = json!(1);
    manifest["active"] = json!({"path":"ledger.jsonl"});
    manifest["next_segment_seq"] = json!(manifested_next.max(physical_next));
    manifest["segments"] = Value::Array(reconciled);
    Ok(manifest)
}

fn discover_segments(segments_dir: &Path) -> io::Result<BTreeMap<u64, PhysicalSegment>> {
    let mut logical = BTreeMap::<String, PhysicalSegment>::new();
    if !segments_dir.is_dir() {
        return Ok(BTreeMap::new());
    }
    for entry in fs::read_dir(segments_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some((stamp, sequence, compressed, logical_name)) = parse_segment_name(name) else {
            continue;
        };
        let relative_path = format!("state/ledger/segments/{name}");
        let candidate = PhysicalSegment {
            sequence,
            stamp,
            size_bytes: path.metadata()?.len(),
            line_count: count_segment_lines(&path, compressed)?,
            path,
            relative_path,
            compressed,
        };
        let replace = logical
            .get(&logical_name)
            .is_none_or(|current| candidate.compressed && !current.compressed);
        if replace {
            logical.insert(logical_name, candidate);
        }
    }
    let mut by_sequence = BTreeMap::new();
    for segment in logical.into_values() {
        if let Some(previous) = by_sequence.insert(segment.sequence, segment) {
            let current = by_sequence
                .get(&previous.sequence)
                .expect("inserted segment exists");
            return Err(io::Error::other(format!(
                "ambiguous ledger segments share sequence {:06}: {} and {}",
                previous.sequence,
                previous.path.display(),
                current.path.display()
            )));
        }
    }
    Ok(by_sequence)
}

fn parse_segment_name(name: &str) -> Option<(String, u64, bool, String)> {
    let (without_gzip, compressed) = name
        .strip_suffix(".gz")
        .map_or((name, false), |value| (value, true));
    let logical_name = without_gzip.to_owned();
    let stem = without_gzip.strip_suffix(".jsonl")?;
    let body = stem.strip_prefix("ledger.")?;
    let (stamp, sequence) = body.rsplit_once('.')?;
    if stamp.is_empty()
        || sequence.len() != 6
        || !sequence.bytes().all(|value| value.is_ascii_digit())
    {
        return None;
    }
    let sequence = sequence.parse().ok()?;
    if sequence == 0 {
        return None;
    }
    Some((stamp.to_owned(), sequence, compressed, logical_name))
}

fn count_segment_lines(path: &Path, compressed: bool) -> io::Result<u64> {
    if compressed {
        BufReader::new(GzDecoder::new(fs::File::open(path)?))
            .lines()
            .try_fold(0_u64, |count, line| line.map(|_| count + 1))
    } else {
        BufReader::new(fs::File::open(path)?)
            .lines()
            .try_fold(0_u64, |count, line| line.map(|_| count + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Event;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn snapshot_does_not_populate_the_query_index() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("streaming snapshot", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        let event = Event::new("chat.message", &group.group_id);
        ledger::append(&path, &event).expect("append");
        crate::ledger_index::invalidate_path(&path);

        let actual = snapshot(&home, &group.group_id, "fixture").expect("snapshot");
        assert_eq!(actual.last_event_id, event.id);
        assert!(
            !crate::ledger_index::is_cached(&path),
            "maintenance must not retain the entire history in a query index"
        );
    }

    #[test]
    fn snapshot_preserves_empty_and_gzip_canonical_hashes_and_default_fields() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("canonical snapshot", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        let empty = snapshot(&home, &group.group_id, "empty").expect("snapshot");
        assert_eq!(empty.event_count, 0);
        assert_eq!(empty.last_event_id, "");
        assert_eq!(empty.sha256, format!("{:x}", Sha256::digest(b"[]")));

        let raw = json!({
            "id":"archived", "ts":"2026-09-08T00:00:00Z", "kind":"chat.message",
            "group_id":group.group_id, "data":{"text":"历史\n\"quoted\"", "number":1.25}
        });
        let first: Event = serde_json::from_value(raw.clone()).expect("default fields");
        let segments = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/segments");
        fs::create_dir_all(&segments).expect("segments");
        let archived = segments.join("ledger.20260908T000000Z.000001.jsonl.gz");
        let mut gzip = flate2::write::GzEncoder::new(
            fs::File::create(&archived).expect("archive"),
            flate2::Compression::default(),
        );
        write!(gzip, "\r\n  {raw}\r\n\t\n").expect("archived record");
        gzip.finish().expect("gzip");
        let last = Event::new("chat.message", &group.group_id);
        // A complete final JSON object without a newline remains readable.
        fs::write(&path, serde_json::to_vec(&last).expect("event")).expect("active");
        let expected = vec![first, last];
        let actual = snapshot(&home, &group.group_id, "fixture").expect("snapshot");
        assert_eq!(actual.event_count, 2);
        assert_eq!(actual.last_event_id, expected[1].id);
        assert_eq!(
            actual.sha256,
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&expected).expect("array"))
            )
        );

        fs::write(&archived, b"broken gzip").expect("corrupt archive");
        assert!(snapshot(&home, &group.group_id, "corrupt").is_err());
    }

    #[test]
    fn maintenance_rejects_json_objects_that_cannot_be_read_as_events() {
        for invalid in [
            json!({"kind":false,"group_id":"fixture"}),
            json!({"kind":"chat.message","group_id":"fixture","unknown":true}),
            json!([
                1,
                "id",
                "2026-09-08T00:00:00Z",
                "chat.message",
                "fixture",
                "",
                "user",
                {}
            ]),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            let home = HomeLayout::from_path(temp.path()).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let group = store.create("invalid event", "").expect("group");
            let path = store.ledger_path(&group.group_id).expect("ledger");
            writeln!(
                fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .expect("ledger"),
                "{invalid}"
            )
            .expect("invalid event fixture");
            let original = fs::read(&path).expect("original");
            assert!(snapshot(&home, &group.group_id, "fixture").is_err());
            assert!(compact(&home, &group.group_id, "fixture").is_err());
            assert_eq!(fs::read(&path).expect("unchanged ledger"), original);
            assert!(
                !store
                    .state_dir(&group.group_id)
                    .expect("state")
                    .join("ledger/snapshot.latest.json")
                    .exists()
            );
        }
    }

    #[test]
    fn maintenance_rejects_records_without_persisted_identity() {
        for field in ["id", "ts"] {
            let temp = tempfile::tempdir().expect("tempdir");
            let home = HomeLayout::from_path(temp.path()).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let group = store.create("missing identity", "").expect("group");
            let path = store.ledger_path(&group.group_id).expect("ledger");
            let mut value =
                serde_json::to_value(Event::new("chat.message", &group.group_id)).expect("event");
            value.as_object_mut().expect("object").remove(field);
            let original = serde_json::to_vec(&value).expect("record");
            fs::write(&path, &original).expect("fixture");
            assert!(
                snapshot(&home, &group.group_id, "missing identity").is_err(),
                "{field} must not be generated while certifying persisted history"
            );
            assert_eq!(fs::read(&path).expect("unchanged ledger"), original);
        }
    }

    #[test]
    fn snapshot_hash_matches_the_canonical_event_array_across_archives() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("snapshot hash", "").expect("group");
        let path = store.ledger_path(&group.group_id).expect("ledger");
        let mut expected = ledger::read_all(&path).expect("initial history");
        for text in ["南京天气 \"quoted\"\nsecond line", "after rotation"] {
            let mut event = Event::new("chat.message", &group.group_id);
            event.data.insert("text".into(), text.into());
            ledger::append(&path, &event).expect("append");
            expected.push(event);
            if text.starts_with("南京") {
                compact(&home, &group.group_id, "fixture").expect("compact");
            }
        }
        let actual = snapshot(&home, &group.group_id, "fixture").expect("snapshot");
        assert_eq!(actual.event_count, expected.len());
        assert_eq!(actual.last_event_id, expected.last().expect("last").id);
        assert_eq!(
            actual.sha256,
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&expected).expect("canonical event array"))
            )
        );
    }

    #[test]
    fn compact_writes_python_compatible_segment_and_manifest() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("compact", "").expect("group");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger"),
            &Event::new("chat.message", &group.group_id),
        )
        .expect("append");

        let segment = compact(&home, &group.group_id, "test")
            .expect("compact")
            .expect("segment");
        let name = segment
            .file_name()
            .and_then(|value| value.to_str())
            .expect("name");
        assert!(name.starts_with("ledger."));
        assert!(name.ends_with(".000001.jsonl"));
        let manifest: Value = read_json(
            &store
                .state_dir(&group.group_id)
                .expect("state")
                .join("ledger/manifest.json"),
        )
        .expect("manifest");
        assert_eq!(manifest["next_segment_seq"], 2);
        assert_eq!(
            manifest["segments"][0]["path"],
            format!("state/ledger/segments/{name}")
        );
        assert_eq!(
            ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
                .expect("events")
                .len(),
            1
        );
    }

    #[test]
    fn compact_recovers_unpublished_segment_before_allocating_sequence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("recover compact", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let first = Event::new("chat.message", &group.group_id);
        ledger::append(&ledger_path, &first).expect("first append");

        let segments = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/segments");
        fs::create_dir_all(&segments).expect("segments");
        let orphan = segments.join("ledger.20260101T000000Z.000001.jsonl");
        fs::rename(&ledger_path, &orphan).expect("inject committed rotation");
        fs::File::create(&ledger_path).expect("replacement active");
        let second = Event::new("chat.message", &group.group_id);
        ledger::append(&ledger_path, &second).expect("second append");

        compact(&home, &group.group_id, "retry")
            .expect("compact")
            .expect("second segment");
        let manifest: Value = read_json(
            &store
                .state_dir(&group.group_id)
                .expect("state")
                .join("ledger/manifest.json"),
        )
        .expect("manifest");
        assert_eq!(manifest["next_segment_seq"], 3);
        assert_eq!(manifest["segments"][0]["seq"], 1);
        assert_eq!(manifest["segments"][1]["seq"], 2);
        assert_eq!(
            manifest["segments"][0]["path"],
            "state/ledger/segments/ledger.20260101T000000Z.000001.jsonl"
        );
        let events = ledger::read_all(&ledger_path).expect("events");
        assert_eq!(
            events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            [first.id.as_str(), second.id.as_str()]
        );
    }

    #[test]
    fn compact_rejects_ambiguous_physical_sequences_before_rotation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("ambiguous compact", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let active = Event::new("chat.message", &group.group_id);
        ledger::append(&ledger_path, &active).expect("active append");

        let segments = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/segments");
        fs::create_dir_all(&segments).expect("segments");
        for stamp in ["20260101T000000Z", "20260102T000000Z"] {
            let path = segments.join(format!("ledger.{stamp}.000001.jsonl"));
            let event = Event::new("chat.message", &group.group_id);
            let mut bytes = serde_json::to_vec(&event).expect("event json");
            bytes.push(b'\n');
            fs::write(path, bytes).expect("segment");
        }
        let original_active = fs::read(&ledger_path).expect("active bytes");

        let error = compact(&home, &group.group_id, "retry").expect_err("ambiguous");
        assert!(
            error
                .to_string()
                .contains("ambiguous ledger segments share sequence")
        );
        assert_eq!(
            fs::read(&ledger_path).expect("active bytes"),
            original_active
        );
    }

    #[test]
    fn maintenance_rejects_malformed_middle_record_before_rotation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("corrupt compact", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&ledger_path, &Event::new("chat.message", &group.group_id))
            .expect("first append");
        fs::OpenOptions::new()
            .append(true)
            .open(&ledger_path)
            .expect("corruption fixture")
            .write_all(b"{\"broken\":\n")
            .expect("malformed middle record");
        ledger::append(&ledger_path, &Event::new("chat.message", &group.group_id))
            .expect("second append");
        let original = fs::read(&ledger_path).expect("active bytes");

        let snapshot_error = snapshot(&home, &group.group_id, "corrupt")
            .expect_err("snapshot must reject malformed source truth");
        assert!(snapshot_error.to_string().contains("malformed ledger JSON"));

        let compact_error = compact(&home, &group.group_id, "corrupt")
            .expect_err("compaction must reject malformed source truth");
        assert!(compact_error.to_string().contains("malformed ledger JSON"));
        assert_eq!(fs::read(&ledger_path).expect("active bytes"), original);
        let segment_dir = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/segments");
        assert!(
            !segment_dir.exists()
                || fs::read_dir(segment_dir)
                    .expect("segments")
                    .next()
                    .is_none()
        );
    }

    #[test]
    fn compact_waits_for_the_writer_lock_and_snapshots_its_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("locked compact", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&ledger_path, &Event::new("chat.message", &group.group_id))
            .expect("seed append");

        let lock = ledger::acquire_writer_lock(&ledger_path).expect("writer lock");
        let mut writer = fs::OpenOptions::new()
            .append(true)
            .open(&ledger_path)
            .expect("active writer");
        let writer_event = Event::new("chat.message", &group.group_id);
        let mut encoded = serde_json::to_vec(&writer_event).expect("event json");
        encoded.push(b'\n');

        let compact_home = home.clone();
        let compact_group_id = group.group_id.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            started_tx.send(()).expect("started receiver");
            compact(&compact_home, &compact_group_id, "writer-lock")
        });
        started_rx.recv().expect("compactor started");
        thread::sleep(Duration::from_millis(250));
        assert!(
            !handle.is_finished(),
            "compaction must remain blocked while the canonical writer lock is held"
        );

        writer.write_all(&encoded).expect("writer commit");
        writer.sync_all().expect("writer sync");
        drop(writer);
        FileExt::unlock(&lock).expect("writer unlock");
        drop(lock);

        handle
            .join()
            .expect("compactor thread")
            .expect("compact")
            .expect("segment");
        let state = store.state_dir(&group.group_id).expect("state");
        let snapshot: Value =
            read_json(&state.join("ledger/snapshot.latest.json")).expect("latest snapshot");
        let manifest: Value = read_json(&state.join("ledger/manifest.json")).expect("manifest");
        assert_eq!(snapshot["last_event_id"], writer_event.id);
        assert_eq!(
            manifest["segments"][0]["line_count"],
            snapshot["event_count"]
        );
    }

    #[test]
    fn snapshot_waits_for_the_writer_lock_before_validating_the_ledger() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("locked snapshot", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&ledger_path, &Event::new("chat.message", &group.group_id))
            .expect("seed append");

        let lock = ledger::acquire_writer_lock(&ledger_path).expect("writer lock");
        let mut writer = fs::OpenOptions::new()
            .append(true)
            .open(&ledger_path)
            .expect("active writer");
        let writer_event = Event::new("chat.message", &group.group_id);
        let encoded = serde_json::to_vec(&writer_event).expect("event json");
        let split = encoded.len() / 2;
        writer
            .write_all(&encoded[..split])
            .expect("partial writer append");
        writer.sync_all().expect("partial writer sync");

        let snapshot_home = home.clone();
        let snapshot_group_id = group.group_id.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            started_tx.send(()).expect("started receiver");
            snapshot(&snapshot_home, &snapshot_group_id, "writer-lock")
        });
        started_rx.recv().expect("snapshot started");
        thread::sleep(Duration::from_millis(250));
        assert!(
            !handle.is_finished(),
            "snapshot must not validate an append while its writer lock is held"
        );

        writer
            .write_all(&encoded[split..])
            .expect("finish writer append");
        writer.write_all(b"\n").expect("writer newline");
        writer.sync_all().expect("writer sync");
        drop(writer);
        FileExt::unlock(&lock).expect("writer unlock");
        drop(lock);

        let result = handle.join().expect("snapshot thread").expect("snapshot");
        assert_eq!(result.last_event_id, writer_event.id);
        assert_eq!(result.event_count, 2);
    }
}
