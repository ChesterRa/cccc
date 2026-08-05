use cccc_contracts::utc_now;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;

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
    let events = ledger::read_all(&store.ledger_path(group_id)?)?;
    let bytes = serde_json::to_vec(&events).map_err(io::Error::other)?;
    let state = store.state_dir(group_id)?.join("ledger/snapshots");
    fs::create_dir_all(&state)?;
    let name = format!("{}.json", stamp());
    let path = state.join(&name);
    let snapshot = LedgerSnapshot {
        v: 1,
        group_id: group_id.into(),
        created_at: utc_now(),
        reason: reason.into(),
        event_count: events.len(),
        last_event_id: events
            .last()
            .map(|event| event.id.clone())
            .unwrap_or_default(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
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
    if !active.exists() || active.metadata()?.len() == 0 {
        return Ok(None);
    }
    snapshot(home, group_id, reason)?;
    let state = store.state_dir(group_id)?.join("ledger");
    let segments = state.join("segments");
    fs::create_dir_all(&segments)?;
    let manifest_path = state.join("manifest.json");
    let mut manifest = load_manifest(&manifest_path)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Event;

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
}
