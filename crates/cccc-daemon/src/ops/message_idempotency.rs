use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub fn find(
    home: &HomeLayout,
    group_id: &str,
    kind: &str,
    by: &str,
    args: &Map<String, Value>,
) -> Option<Event> {
    let client_id = args
        .get("client_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let path = GroupStore::new(home.clone())
        .ok()?
        .ledger_path(group_id)
        .ok()?;
    ledger::find_idempotent(&path, kind, by, client_id)
        .ok()
        .flatten()
}

pub fn find_relay(home: &HomeLayout, group_id: &str, source_event_id: &str) -> Option<Event> {
    let path = GroupStore::new(home.clone())
        .ok()?
        .ledger_path(group_id)
        .ok()?;
    ledger::find_relay(&path, source_event_id).ok().flatten()
}

pub fn tracked_client_id(group_id: &str, by: &str, key: &str) -> String {
    let digest = Sha256::digest(format!("{group_id}\0{by}\0{key}"));
    format!("tracked-send:{digest:.32x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_client_id_matches_python_shape() {
        let value = tracked_client_id("group", "user", "request");
        assert_eq!(value, "tracked-send:abed96f07487e072ead638feb064bf1a");
    }
}
