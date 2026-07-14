use cccc_contracts::utc_now;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::PathBuf;
use uuid::Uuid;

use crate::fs::write_json;
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
    let segments = store.state_dir(group_id)?.join("ledger/segments");
    fs::create_dir_all(&segments)?;
    let destination = segments.join(format!("{}.jsonl", stamp()));
    fs::rename(&active, &destination)?;
    fs::File::create(&active)?.sync_all()?;
    Ok(Some(destination))
}

fn stamp() -> String {
    format!(
        "{}_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.6fZ"),
        &Uuid::new_v4().simple().to_string()[..8]
    )
}
