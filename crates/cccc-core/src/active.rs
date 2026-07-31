use serde::{Deserialize, Serialize};
use std::io;

use crate::HomeLayout;
use crate::fs::{read_json, write_json_committed};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct ActiveState {
    #[serde(default)]
    group_id: String,
}

pub fn get(home: &HomeLayout) -> io::Result<Option<String>> {
    let path = home.root().join("active.json");
    if !path.exists() {
        return Ok(None);
    }
    let state: ActiveState = read_json(&path)?;
    Ok((!state.group_id.is_empty()).then_some(state.group_id))
}

pub fn set(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    if group_id.is_empty() {
        return Err(io::Error::other("group_id is required"));
    }
    write_json_committed(
        &home.root().join("active.json"),
        &ActiveState {
            group_id: group_id.into(),
        },
    )
}

pub fn clear(home: &HomeLayout) -> io::Result<()> {
    let path = home.root().join("active.json");
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
