use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;

use crate::HomeLayout;
use crate::fs::{read_json, write_json_committed};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ActiveState {
    v: u8,
    active_group_id: String,
    updated_at: String,
}

impl ActiveState {
    fn new(group_id: &str) -> Self {
        Self {
            v: 1,
            active_group_id: group_id.trim().to_owned(),
            updated_at: cccc_contracts::utc_now(),
        }
    }
}

pub fn get(home: &HomeLayout) -> io::Result<Option<String>> {
    let path = home.root().join("active.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw: Value = read_json(&path)?;
    let object = raw.as_object();
    let has_canonical_key = object.is_some_and(|value| value.contains_key("active_group_id"));
    let group_id = object
        .and_then(|value| {
            value
                .get(if has_canonical_key {
                    "active_group_id"
                } else {
                    "group_id"
                })
                .and_then(Value::as_str)
        })
        .unwrap_or_default()
        .trim()
        .to_owned();
    Ok((!group_id.is_empty()).then_some(group_id))
}

pub fn set(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    if group_id.trim().is_empty() {
        return Err(io::Error::other("group_id is required"));
    }
    write_json_committed(
        &home.root().join("active.json"),
        &ActiveState::new(group_id),
    )
}

pub fn clear(home: &HomeLayout) -> io::Result<()> {
    write_json_committed(&home.root().join("active.json"), &ActiveState::new(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::write_json;
    use serde_json::json;

    #[test]
    fn reads_python_document_without_clearing_selection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let path = home.root().join("active.json");
        write_json(
            &path,
            &json!({
                "v":1,
                "active_group_id":"g_python",
                "updated_at":"2026-08-09T00:00:00Z"
            }),
        )
        .expect("python document");

        assert_eq!(get(&home).expect("active").as_deref(), Some("g_python"));
        let persisted: Value = read_json(&path).expect("persisted");
        assert_eq!(persisted["active_group_id"], "g_python");
        assert!(persisted.get("group_id").is_none());
    }

    #[test]
    fn reads_legacy_rust_document_without_writeback() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let path = home.root().join("active.json");
        write_json(&path, &json!({"group_id":"g_legacy"})).expect("legacy document");

        assert_eq!(get(&home).expect("active").as_deref(), Some("g_legacy"));
        let persisted: Value = read_json(&path).expect("persisted");
        assert_eq!(persisted, json!({"group_id":"g_legacy"}));
    }

    #[test]
    fn canonical_key_wins_over_stale_legacy_key() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let path = home.root().join("active.json");
        write_json(
            &path,
            &json!({
                "v":1,
                "active_group_id":"",
                "group_id":"g_stale",
                "updated_at":"2026-08-09T00:00:00Z"
            }),
        )
        .expect("conflicting document");

        assert_eq!(get(&home).expect("active"), None);
        let persisted: Value = read_json(&path).expect("persisted");
        assert_eq!(persisted["active_group_id"], "");
        assert_eq!(persisted["group_id"], "g_stale");
    }
}
