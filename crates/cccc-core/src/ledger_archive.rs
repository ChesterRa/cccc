use cccc_contracts::utc_now;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use fs2::FileExt;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::fs::{read_json, write_json};
use crate::ledger;
use crate::{GroupDoc, GroupStore, HomeLayout};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SegmentCodec {
    Plain,
    Gzip,
}

impl SegmentCodec {
    pub(crate) fn from_path(path: &Path) -> Self {
        Self::from_name(
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(""),
        )
    }

    pub(crate) fn from_name(name: &str) -> Self {
        if name.ends_with(".gz") {
            Self::Gzip
        } else {
            Self::Plain
        }
    }

    pub(crate) fn compressed(self) -> bool {
        self == Self::Gzip
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Gzip => "gzip",
        }
    }

    pub(crate) fn open_reader<'a, R: Read + 'a>(self, reader: R) -> Box<dyn BufRead + 'a> {
        open_segment_reader(self, reader)
    }
}

pub(crate) fn open_segment_reader<'a, R: Read + 'a>(
    codec: SegmentCodec,
    reader: R,
) -> Box<dyn BufRead + 'a> {
    match codec {
        SegmentCodec::Plain => Box::new(BufReader::new(reader)),
        SegmentCodec::Gzip => Box::new(BufReader::new(GzDecoder::new(reader))),
    }
}

pub(crate) fn open_segment(path: &Path) -> io::Result<Box<dyn BufRead>> {
    Ok(SegmentCodec::from_path(path).open_reader(fs::File::open(path)?))
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
    let codec = configured_codec(&store.load(group_id)?)?;
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
        let base_name = format!("ledger.{created_at}.{sequence:06}.jsonl");
        let plain_destination = segments.join(&base_name);
        // Publish the plain segment first. A crash after this rename leaves a
        // discoverable segment, never an unreadable half-written archive.
        fs::rename(&active, &plain_destination)?;
        fs::File::create(&active)?.sync_all()?;
        let plain_bytes = plain_destination.metadata()?.len();
        let (line_count, plain_sha256) = segment_stats(&plain_destination, SegmentCodec::Plain)?;
        let (destination, final_codec) = if codec == SegmentCodec::Gzip {
            let gzip_name = format!("{base_name}.gz");
            let destination = segments.join(&gzip_name);
            let temp = segments.join(format!(".{gzip_name}.tmp"));
            match compress_verified(&plain_destination, &temp, &plain_sha256, line_count) {
                Ok(()) => {
                    fs::rename(&temp, &destination)?;
                    crate::fs::sync_dir(&segments)?;
                    fs::remove_file(&plain_destination)?;
                    crate::fs::sync_dir(&segments)?;
                    (destination, SegmentCodec::Gzip)
                }
                Err(error) => {
                    let _ = fs::remove_file(&temp);
                    tracing::warn!(
                        diagnostic_action = "ledger_segment_compress",
                        %error,
                        "ledger segment kept plain after compression verification failed"
                    );
                    (plain_destination.clone(), SegmentCodec::Plain)
                }
            }
        } else {
            (plain_destination.clone(), SegmentCodec::Plain)
        };
        let size_bytes = destination.metadata()?.len();
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let mut segment = json!({
            "id":format!("{sequence:06}"),
            "seq":sequence,
            "path":format!("state/ledger/segments/{name}"),
            "codec":final_codec.as_str(),
            "compressed":final_codec.compressed(),
            "created_at":created_at,
            "sealed_at":created_at,
            "reason":reason,
            "size_bytes":size_bytes,
            "line_count":line_count,
        });
        if final_codec.compressed() {
            segment["plain_bytes"] = json!(plain_bytes);
        }
        manifest["schema"] = json!(1);
        manifest["active"] = json!({"path":"ledger.jsonl"});
        manifest["next_segment_seq"] = json!(sequence + 1);
        manifest["updated_at"] = json!(created_at);
        manifest["segments"]
            .as_array_mut()
            .expect("segments initialized")
            .push(segment);
        #[cfg(test)]
        if take_fail_manifest_write() {
            return Err(io::Error::other("injected ledger manifest write failure"));
        }
        write_json(&manifest_path, &manifest)?;
        Ok(Some(destination))
    })();
    let _ = FileExt::unlock(&lock);
    result
}

/// Validate the ledger settings accepted by the normal group-settings patch.
/// Null is the existing merge API's way to remove a key and restore its
/// default, so it is accepted separately from a present invalid value.
pub fn validate_settings(patch: &serde_json::Map<String, Value>) -> io::Result<()> {
    if let Some(value) = patch.get("ledger_segment_codec")
        && !value.is_null()
        && !matches!(value.as_str(), Some("gzip" | "none"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ledger_segment_codec must be gzip or none",
        ));
    }
    Ok(())
}

fn configured_codec(group: &GroupDoc) -> io::Result<SegmentCodec> {
    match setting(group, "ledger_segment_codec") {
        None => Ok(SegmentCodec::Gzip),
        Some(Value::String(value)) if value == "gzip" => Ok(SegmentCodec::Gzip),
        Some(Value::String(value)) if value == "none" => Ok(SegmentCodec::Plain),
        Some(value) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("ledger_segment_codec must be gzip or none, got {value}"),
        )),
    }
}

fn setting<'a>(group: &'a GroupDoc, key: &str) -> Option<&'a Value> {
    group
        .extra
        .get("settings")
        .and_then(Value::as_object)
        .and_then(|settings| settings.get(key))
}

fn compress_verified(
    plain: &Path,
    temp: &Path,
    expected_sha256: &str,
    expected_lines: u64,
) -> io::Result<()> {
    let mut source = BufReader::new(fs::File::open(plain)?);
    let mut encoder = GzEncoder::new(fs::File::create(temp)?, Compression::default());
    io::copy(&mut source, &mut encoder)?;
    let file = encoder.finish()?;
    file.sync_all()?;
    #[cfg(test)]
    if take_truncate_gzip_tmp() {
        fs::OpenOptions::new().write(true).open(temp)?.set_len(4)?;
    }
    let (lines, sha256) = segment_stats(temp, SegmentCodec::Gzip)?;
    if lines != expected_lines || sha256 != expected_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "gzip segment did not decode to the sealed plain segment",
        ));
    }
    Ok(())
}

fn segment_stats(path: &Path, codec: SegmentCodec) -> io::Result<(u64, String)> {
    let mut reader = codec.open_reader(fs::File::open(path)?);
    let mut digest = Sha256::new();
    let mut newlines = 0_u64;
    let mut total = 0_u64;
    let mut last = None;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        newlines += buffer[..read].iter().filter(|byte| **byte == b'\n').count() as u64;
        total += read as u64;
        last = buffer[..read].last().copied();
    }
    let lines = newlines + u64::from(total > 0 && last != Some(b'\n'));
    Ok((lines, format!("{:x}", digest.finalize())))
}

#[cfg(test)]
thread_local! {
    static TRUNCATE_GZIP_TMP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAIL_MANIFEST_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn fail_next_gzip_verification() {
    TRUNCATE_GZIP_TMP.with(|flag| flag.set(true));
}

#[cfg(test)]
fn take_truncate_gzip_tmp() -> bool {
    TRUNCATE_GZIP_TMP.with(|flag| flag.replace(false))
}

#[cfg(test)]
fn fail_next_manifest_write() {
    FAIL_MANIFEST_WRITE.with(|flag| flag.set(true));
}

#[cfg(test)]
fn take_fail_manifest_write() -> bool {
    FAIL_MANIFEST_WRITE.with(|flag| flag.replace(false))
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
    codec: SegmentCodec,
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
        item.insert("codec".into(), json!(segment.codec.as_str()));
        item.insert("compressed".into(), json!(segment.codec.compressed()));
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
        let Some((stamp, sequence, codec, logical_name)) = parse_segment_name(name) else {
            continue;
        };
        let relative_path = format!("state/ledger/segments/{name}");
        let candidate = PhysicalSegment {
            sequence,
            stamp,
            size_bytes: path.metadata()?.len(),
            line_count: count_segment_lines(&path, codec)?,
            path,
            relative_path,
            codec,
        };
        let replace = logical
            .get(&logical_name)
            .is_none_or(|current| candidate.codec.compressed() && !current.codec.compressed());
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

pub(crate) fn parse_segment_name(name: &str) -> Option<(String, u64, SegmentCodec, String)> {
    let (without_gzip, codec) = name
        .strip_suffix(".gz")
        .map_or((name, SegmentCodec::Plain), |value| {
            (value, SegmentCodec::Gzip)
        });
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
    Some((stamp.to_owned(), sequence, codec, logical_name))
}

fn count_segment_lines(path: &Path, codec: SegmentCodec) -> io::Result<u64> {
    codec
        .open_reader(fs::File::open(path)?)
        .lines()
        .try_fold(0_u64, |count, line| line.map(|_| count + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Event;
    use std::path::Path;
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
        store
            .mutate(&group.group_id, |group| {
                group
                    .extra
                    .entry("settings".to_owned())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .expect("settings object")
                    .insert("ledger_segment_codec".into(), json!("none"));
                Ok(())
            })
            .expect("plain segment codec");
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
        assert_eq!(manifest["segments"][0]["codec"], "plain");
        assert_eq!(manifest["segments"][0]["compressed"], false);
        assert_eq!(
            ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
                .expect("events")
                .len(),
            1
        );
    }

    fn real_fixture() -> Vec<u8> {
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ledger-redacted.jsonl"))
            .expect("real ledger fixture")
    }

    fn fixture_events(bytes: &[u8]) -> Vec<Event> {
        bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).expect("fixture event"))
            .collect()
    }

    fn set_setting(store: &GroupStore, group_id: &str, key: &str, value: Value) {
        store
            .mutate(group_id, |group| {
                group
                    .extra
                    .entry("settings".to_owned())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .expect("settings object")
                    .insert(key.to_owned(), value);
                Ok(())
            })
            .expect("group setting");
    }

    #[test]
    fn compact_defaults_to_verified_gzip_segments() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("gzip compact", "").expect("group");
        let active = store.ledger_path(&group.group_id).expect("ledger");
        let bytes = real_fixture();
        let expected = fixture_events(&bytes);
        fs::write(&active, &bytes).expect("fixture ledger");

        let segment = compact(&home, &group.group_id, "test")
            .expect("compact")
            .expect("segment");
        let name = segment
            .file_name()
            .and_then(|value| value.to_str())
            .expect("name");
        assert!(name.ends_with(".jsonl.gz"), "{name}");
        assert_eq!(
            fs::read_dir(segment.parent().expect("segments"))
                .expect("segments")
                .count(),
            1
        );
        let manifest: Value = read_json(
            &store
                .state_dir(&group.group_id)
                .expect("state")
                .join("ledger/manifest.json"),
        )
        .expect("manifest");
        let sealed = &manifest["segments"][0];
        assert_eq!(sealed["codec"], "gzip");
        assert_eq!(sealed["compressed"], true);
        assert_eq!(sealed["plain_bytes"], bytes.len() as u64);
        assert_eq!(sealed["line_count"], expected.len() as u64);
        assert_eq!(
            sealed["size_bytes"],
            fs::metadata(&segment).expect("metadata").len()
        );

        let events = ledger::read_all(&active).expect("events");
        assert_eq!(
            events.iter().map(|event| &event.id).collect::<Vec<_>>(),
            expected.iter().map(|event| &event.id).collect::<Vec<_>>()
        );
        assert_eq!(
            open_segment(&segment).expect("decode").lines().count(),
            expected.len()
        );
        let found = crate::ledger_index::find_event(&active, &expected[0].id)
            .expect("index")
            .expect("fixture event in gzip segment");
        assert_eq!(found.id, expected[0].id);
    }

    #[test]
    fn failed_gzip_verification_keeps_the_plain_segment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("gzip fallback", "").expect("group");
        let active = store.ledger_path(&group.group_id).expect("ledger");
        let bytes = real_fixture();
        let expected = fixture_events(&bytes);
        fs::write(&active, &bytes).expect("fixture ledger");

        fail_next_gzip_verification();
        let segment = compact(&home, &group.group_id, "test")
            .expect("fallback compact")
            .expect("segment");
        let name = segment
            .file_name()
            .and_then(|value| value.to_str())
            .expect("name");
        assert!(name.ends_with(".jsonl"), "{name}");
        assert!(
            fs::read_dir(segment.parent().expect("segments"))
                .expect("segments")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".gz"))
        );
        let manifest: Value = read_json(
            &store
                .state_dir(&group.group_id)
                .expect("state")
                .join("ledger/manifest.json"),
        )
        .expect("manifest");
        assert_eq!(manifest["segments"][0]["codec"], "plain");
        assert_eq!(manifest["segments"][0]["compressed"], false);
        assert_eq!(
            ledger::read_all(&active).expect("events").len(),
            expected.len()
        );
    }

    #[test]
    fn invalid_ledger_codec_fails_instead_of_defaulting() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("invalid settings", "").expect("group");
        let active = store.ledger_path(&group.group_id).expect("ledger");
        let bytes = real_fixture();
        fs::write(&active, &bytes).expect("fixture ledger");

        for invalid in [json!(false), json!(1), json!("zstd")] {
            set_setting(&store, &group.group_id, "ledger_segment_codec", invalid);
            let error = compact(&home, &group.group_id, "test").expect_err("invalid codec");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(fs::read(&active).expect("active"), bytes);
        }
    }

    #[test]
    fn published_segment_survives_manifest_write_failure_and_is_reconciled() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("manifest failure", "").expect("group");
        let active = store.ledger_path(&group.group_id).expect("ledger");
        let bytes = real_fixture();
        let expected = fixture_events(&bytes);
        fs::write(&active, &bytes).expect("fixture ledger");

        fail_next_manifest_write();
        assert!(compact(&home, &group.group_id, "test").is_err());
        let segments = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/segments");
        assert_eq!(
            fs::read_dir(&segments)
                .expect("segments")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".jsonl.gz"))
                .count(),
            1
        );
        assert_eq!(
            ledger::read_all(&active).expect("published gzip").len(),
            expected.len()
        );

        ledger::append(&active, &Event::new("chat.message", &group.group_id)).expect("append");
        compact(&home, &group.group_id, "repair").expect("reconcile");
        let manifest: Value = read_json(
            &store
                .state_dir(&group.group_id)
                .expect("state")
                .join("ledger/manifest.json"),
        )
        .expect("manifest");
        assert_eq!(manifest["segments"].as_array().expect("segments").len(), 2);
        assert_eq!(
            ledger::read_all(&active).expect("all events").len(),
            expected.len() + 1
        );
    }

    #[test]
    fn gzip_snapshot_resumes_at_a_decoded_offset() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("gzip snapshot", "").expect("group");
        let active = store.ledger_path(&group.group_id).expect("ledger");
        let bytes = real_fixture();
        let expected = fixture_events(&bytes);
        let first_line = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("first line") as u64
            + 1;
        fs::write(&active, &bytes).expect("fixture ledger");
        let segment = compact(&home, &group.group_id, "test")
            .expect("compact")
            .expect("segment");
        let snapshot = crate::ledger::snapshot::Snapshot::open(&active).expect("snapshot");
        let source = snapshot
            .sources
            .iter()
            .position(|source| source.path == segment)
            .expect("gzip source");
        let mut offsets = vec![0; snapshot.sources.len()];
        offsets[source] = first_line;
        let mut ids = Vec::new();
        snapshot
            .visit(&offsets, |event, _| {
                ids.push(event.id);
                std::ops::ControlFlow::Continue(())
            })
            .expect("resume snapshot");
        assert_eq!(
            ids,
            expected[1..]
                .iter()
                .map(|event| event.id.clone())
                .collect::<Vec<_>>()
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
