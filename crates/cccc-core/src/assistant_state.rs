//! Canonical durable state for built-in assistants.
//!
//! User configuration remains in `group.yaml:assistants`. Recoverable workflow
//! state lives in the stable `state/assistants.json` contract inherited from
//! 0.4.35. Older native builds mixed both classes under
//! `group.yaml:assistants`; that shape is imported once and then reduced to
//! configuration-only data.

use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs::{read_json, with_exclusive_lock, write_json_committed};
use crate::{GroupDoc, GroupStore, HomeLayout};

const SCHEMA: u64 = 1;
const ASSISTANT_ID: &str = "voice_secretary";
const STATE_FILE: &str = "assistants.json";
const RUST_STATE_KEY: &str = "rust_state";
const COMMON_FLAT_KEYS: &[&str] = &[
    "assistant",
    "voice_secretary",
    "sessions",
    "prompt_draft",
    "voice_prompt_drafts",
    "voice_prompt_requests",
    "ask_requests",
];

/// Load assistant state as the flat view expected by the native daemon.
/// Common fields are projected from the canonical 0.4.35 schema.
pub fn load(home: &HomeLayout, group_id: &str) -> io::Result<Value> {
    migrate_legacy_group_state(home, group_id)?;
    let store = GroupStore::new(home.clone())?;
    let group = store.load(group_id)?;
    let path = state_path(&store, group_id)?;
    with_exclusive_lock(&lock_path(&path), || {
        let canonical = load_canonical_unlocked(&path, group_id)?;
        Ok(canonical_to_flat(&canonical, &group))
    })
}

/// Mutate shared assistant workflow state under one cross-process file lock.
pub fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> io::Result<T> {
    migrate_legacy_group_state(home, group_id)?;
    let store = GroupStore::new(home.clone())?;
    let group = store.load(group_id)?;
    let path = state_path(&store, group_id)?;
    with_exclusive_lock(&lock_path(&path), || {
        let mut canonical = load_canonical_unlocked(&path, group_id)?;
        let mut flat = canonical_to_flat(&canonical, &group);
        let result = change(flat.as_object_mut().expect("assistant state initialized"))?;
        persist_flat_unlocked(&path, group_id, &mut canonical, &flat)?;
        Ok(result)
    })
}

fn state_path(store: &GroupStore, group_id: &str) -> io::Result<PathBuf> {
    Ok(store.state_dir(group_id)?.join(STATE_FILE))
}

fn lock_path(path: &Path) -> PathBuf {
    path.with_extension("json.lock")
}

fn empty_canonical(group_id: &str) -> Value {
    json!({
        "schema":SCHEMA,
        "group_id":group_id,
        "assistants":{},
        "voice_sessions":{},
        "voice_prompt_drafts":{},
        "voice_prompt_requests":{},
        "voice_ask_requests":{},
        RUST_STATE_KEY:{},
    })
}

fn load_canonical_unlocked(path: &Path, group_id: &str) -> io::Result<Value> {
    let mut value = if path.exists() {
        read_json::<Value>(path)?
    } else {
        empty_canonical(group_id)
    };
    let Some(root) = value.as_object_mut() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "assistant state must be a JSON object",
        ));
    };
    let schema = root.get("schema").and_then(Value::as_u64).unwrap_or(0);
    if schema != SCHEMA {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported assistant state schema: {schema}"),
        ));
    }
    let stored_group = root.get("group_id").and_then(Value::as_str).unwrap_or("");
    if !stored_group.is_empty() && stored_group != group_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "assistant state group_id does not match its group directory",
        ));
    }
    normalize_canonical(root, group_id);
    Ok(value)
}

fn normalize_canonical(root: &mut Map<String, Value>, group_id: &str) {
    root.insert("schema".into(), json!(SCHEMA));
    root.insert("group_id".into(), json!(group_id));
    for key in [
        "assistants",
        "voice_sessions",
        "voice_prompt_drafts",
        "voice_prompt_requests",
        "voice_ask_requests",
        RUST_STATE_KEY,
    ] {
        if !root.get(key).is_some_and(Value::is_object) {
            root.insert(key.into(), json!({}));
        }
    }
}

fn canonical_to_flat(canonical: &Value, group: &GroupDoc) -> Value {
    let mut flat = canonical
        .get(RUST_STATE_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in ["voice_prompt_drafts", "voice_prompt_requests"] {
        flat.insert(
            key.into(),
            canonical.get(key).cloned().unwrap_or_else(|| json!({})),
        );
    }
    flat.insert(
        "sessions".into(),
        map_records_as_array(canonical.get("voice_sessions"), "session_id", false),
    );
    flat.insert(
        "ask_requests".into(),
        map_records_as_array(canonical.get("voice_ask_requests"), "request_id", true),
    );

    let mut assistant = group
        .extra
        .get("assistants")
        .and_then(|value| value.get(ASSISTANT_ID))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(runtime) = canonical
        .get("assistants")
        .and_then(|value| value.get(ASSISTANT_ID))
        .and_then(Value::as_object)
    {
        for (key, value) in runtime {
            assistant.insert(key.clone(), value.clone());
        }
    }
    let assistant = Value::Object(assistant);
    flat.insert("assistant".into(), assistant.clone());
    flat.insert(ASSISTANT_ID.into(), assistant);
    flat.insert(
        "prompt_draft".into(),
        latest_pending_draft(canonical.get("voice_prompt_drafts")),
    );
    Value::Object(flat)
}

fn map_records_as_array(value: Option<&Value>, id_field: &str, newest_first: bool) -> Value {
    let mut records = value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| {
            let mut record = value.as_object()?.clone();
            if record
                .get(id_field)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                record.insert(id_field.into(), json!(key));
            }
            Some((key.clone(), Value::Object(record)))
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        let left_order = record_order(&left.1, &left.0);
        let right_order = record_order(&right.1, &right.0);
        if newest_first {
            right_order.cmp(&left_order)
        } else {
            left_order.cmp(&right_order)
        }
    });
    Value::Array(records.into_iter().map(|(_, value)| value).collect())
}

fn record_order(value: &Value, fallback: &str) -> String {
    value
        .get("updated_at")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| value.get("created_at").and_then(Value::as_str))
        .unwrap_or(fallback)
        .to_owned()
}

fn latest_pending_draft(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, draft)| draft["status"] == "pending")
        .max_by_key(|(key, draft)| record_order(draft, key))
        .map(|(_, draft)| draft.clone())
        .unwrap_or(Value::Null)
}

fn persist_flat_unlocked(
    path: &Path,
    group_id: &str,
    canonical: &mut Value,
    flat: &Value,
) -> io::Result<()> {
    let root = canonical
        .as_object_mut()
        .expect("canonical assistant state initialized");
    normalize_canonical(root, group_id);
    let flat_root = flat.as_object().cloned().unwrap_or_default();

    let assistant = flat_root
        .get("assistant")
        .or_else(|| flat_root.get(ASSISTANT_ID))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let assistants = root
        .get_mut("assistants")
        .and_then(Value::as_object_mut)
        .expect("assistant map normalized");
    let durable_runtime = durable_assistant_runtime(&assistant);
    if durable_runtime.as_object().is_some_and(Map::is_empty) {
        assistants.remove(ASSISTANT_ID);
    } else {
        assistants.insert(ASSISTANT_ID.into(), durable_runtime);
    }

    root.insert(
        "voice_sessions".into(),
        records_array_as_map(flat_root.get("sessions"), "session_id"),
    );
    for key in ["voice_prompt_drafts", "voice_prompt_requests"] {
        root.insert(
            key.into(),
            flat_root
                .get(key)
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({})),
        );
    }
    root.insert(
        "voice_ask_requests".into(),
        records_array_as_map(flat_root.get("ask_requests"), "request_id"),
    );

    let common = COMMON_FLAT_KEYS.iter().copied().collect::<HashSet<_>>();
    let rust_state = flat_root
        .into_iter()
        .filter(|(key, _)| !common.contains(key.as_str()))
        .collect::<Map<_, _>>();
    root.insert(RUST_STATE_KEY.into(), Value::Object(rust_state));
    write_json_committed(path, canonical)
}

fn durable_assistant_runtime(value: &Value) -> Value {
    let Some(source) = value.as_object() else {
        return json!({});
    };
    let mut runtime = Map::new();
    for key in ["lifecycle", "updated_at"] {
        if let Some(value) = source.get(key) {
            runtime.insert(key.into(), value.clone());
        }
    }
    if let Some(health) = source.get("health").and_then(Value::as_object) {
        let mut health = health.clone();
        for key in [
            "actor",
            "service",
            "pid",
            "port",
            "host",
            "alive",
            "exit_code",
            "websocket",
        ] {
            health.remove(key);
        }
        runtime.insert("health".into(), Value::Object(health));
    }
    Value::Object(runtime)
}

fn records_array_as_map(value: Option<&Value>, id_field: &str) -> Value {
    let records = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let id = value.get(id_field).and_then(Value::as_str)?.trim();
            (!id.is_empty() && value.is_object()).then(|| (id.to_owned(), value.clone()))
        })
        .collect::<Map<_, _>>();
    Value::Object(records)
}

fn migrate_legacy_group_state(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    let store = GroupStore::new(home.clone())?;
    let group = store.load(group_id)?;
    let Some(legacy) = group
        .extra
        .get("assistants")
        .filter(|value| has_legacy_workflow(value))
    else {
        return Ok(());
    };
    let path = state_path(&store, group_id)?;
    with_exclusive_lock(&lock_path(&path), || {
        let mut canonical = load_canonical_unlocked(&path, group_id)?;
        merge_legacy_into_canonical(&mut canonical, legacy);
        write_json_committed(&path, &canonical)
    })?;

    // Canonical is committed first. If this cleanup fails, the next read safely
    // retries it and canonical fields continue to win every conflict.
    store.mutate(group_id, |group| {
        let legacy = group
            .extra
            .get("assistants")
            .cloned()
            .unwrap_or(Value::Null);
        let config = configuration_only(&legacy);
        if config.as_object().is_some_and(Map::is_empty) {
            group.extra.remove("assistants");
        } else {
            group.extra.insert("assistants".into(), config);
        }
        Ok(())
    })
}

fn has_legacy_workflow(value: &Value) -> bool {
    let Some(root) = value.as_object() else {
        return false;
    };
    root.contains_key("assistant")
        || [
            "documents",
            "sessions",
            "ask_requests",
            "prompt_draft",
            "voice_prompt_drafts",
            "voice_prompt_requests",
            "input_latest_seq",
            "input_read_cursor",
        ]
        .iter()
        .any(|key| root.contains_key(*key))
        || root
            .get(ASSISTANT_ID)
            .and_then(Value::as_object)
            .is_some_and(|assistant| {
                assistant.contains_key("lifecycle") || assistant.contains_key("health")
            })
}

fn configuration_only(value: &Value) -> Value {
    let Some(root) = value.as_object() else {
        return json!({});
    };
    let mut configurations = Map::new();
    for (key, value) in root {
        if key == "assistant" || COMMON_FLAT_KEYS.contains(&key.as_str()) {
            continue;
        }
        if let Some(config) = config_fields(value) {
            configurations.insert(key.clone(), config);
        }
    }
    if !configurations.contains_key(ASSISTANT_ID) {
        if let Some(config) = root
            .get("assistant")
            .and_then(config_fields)
            .or_else(|| root.get(ASSISTANT_ID).and_then(config_fields))
        {
            configurations.insert(ASSISTANT_ID.into(), config);
        }
    }
    Value::Object(configurations)
}

fn config_fields(value: &Value) -> Option<Value> {
    let source = value.as_object()?;
    let mut config = Map::new();
    for key in ["enabled", "config"] {
        if let Some(value) = source.get(key) {
            config.insert(key.into(), value.clone());
        }
    }
    (!config.is_empty()).then_some(Value::Object(config))
}

fn merge_legacy_into_canonical(canonical: &mut Value, legacy: &Value) {
    let Some(legacy) = legacy.as_object() else {
        return;
    };
    let root = canonical
        .as_object_mut()
        .expect("canonical assistant state initialized");
    let group_id = root
        .get("group_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    normalize_canonical(root, &group_id);

    let candidate = legacy.get("assistant").or_else(|| legacy.get(ASSISTANT_ID));
    if let Some(candidate) = candidate {
        let imported = durable_assistant_runtime(candidate);
        let assistants = root["assistants"]
            .as_object_mut()
            .expect("assistant map normalized");
        let target = assistants.entry(ASSISTANT_ID).or_insert_with(|| json!({}));
        merge_missing_object(target, &imported);
    }
    merge_missing_records(
        root.get_mut("voice_sessions").expect("sessions normalized"),
        legacy.get("sessions"),
        "session_id",
    );
    for key in ["voice_prompt_drafts", "voice_prompt_requests"] {
        merge_missing_map(
            root.get_mut(key).expect("prompt map normalized"),
            legacy.get(key),
        );
    }
    if let Some(draft) = legacy.get("prompt_draft").filter(|value| value.is_object()) {
        let request_id = draft["request_id"].as_str().unwrap_or("");
        if !request_id.is_empty() {
            root["voice_prompt_drafts"]
                .as_object_mut()
                .expect("draft map normalized")
                .entry(request_id)
                .or_insert_with(|| draft.clone());
        }
    }
    merge_missing_records(
        root.get_mut("voice_ask_requests").expect("asks normalized"),
        legacy.get("ask_requests"),
        "request_id",
    );

    let common = COMMON_FLAT_KEYS.iter().copied().collect::<HashSet<_>>();
    let rust_state = root[RUST_STATE_KEY]
        .as_object_mut()
        .expect("rust state normalized");
    for (key, value) in legacy {
        if !common.contains(key.as_str()) {
            rust_state
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
}

fn merge_missing_object(target: &mut Value, source: &Value) {
    let Some(source) = source.as_object() else {
        return;
    };
    if !target.is_object() {
        *target = json!({});
    }
    let target = target.as_object_mut().expect("object initialized");
    for (key, value) in source {
        target.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

fn merge_missing_map(target: &mut Value, source: Option<&Value>) {
    let Some(source) = source.and_then(Value::as_object) else {
        return;
    };
    let target = target.as_object_mut().expect("canonical map normalized");
    for (key, value) in source {
        if value.is_object() {
            target.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
}

fn merge_missing_records(target: &mut Value, source: Option<&Value>, id_field: &str) {
    let Some(source) = source.and_then(Value::as_array) else {
        return;
    };
    let target = target.as_object_mut().expect("canonical map normalized");
    for value in source {
        let Some(id) = value
            .get(id_field)
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        target.entry(id).or_insert_with(|| value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, HomeLayout, GroupStore, GroupDoc) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("assistant state", "").expect("group");
        (temp, home, store, group)
    }

    #[test]
    fn python_canonical_workflow_projects_into_the_native_view() {
        let (_temp, home, store, group) = fixture();
        let path = state_path(&store, &group.group_id).expect("path");
        write_json_committed(
            &path,
            &json!({
                "schema":1,
                "group_id":group.group_id,
                "assistants":{"voice_secretary":{"lifecycle":"working","health":{"source":"python"}}},
                "voice_sessions":{
                    "session-new":{"session_id":"session-new","updated_at":"2026-08-10T02:00:00Z"},
                    "session-old":{"session_id":"session-old","updated_at":"2026-08-10T01:00:00Z"}
                },
                "voice_prompt_drafts":{"draft-a":{"request_id":"draft-a","status":"pending","updated_at":"2026-08-10T01:00:00Z"}},
                "voice_prompt_requests":{"draft-a":{"request_id":"draft-a"}},
                "voice_ask_requests":{
                    "ask-new":{"request_id":"ask-new","status":"pending","updated_at":"2026-08-10T02:00:00Z"},
                    "ask-old":{"request_id":"ask-old","status":"pending","updated_at":"2026-08-10T01:00:00Z"}
                },
                "rust_state":{"native_extension":{"revision":7}}
            }),
        )
        .expect("canonical state");

        let state = load(&home, &group.group_id).expect("load");
        assert_eq!(state["assistant"]["lifecycle"], "working");
        assert_eq!(state["prompt_draft"]["request_id"], "draft-a");
        assert_eq!(state["ask_requests"][0]["request_id"], "ask-new");
        assert_eq!(state["sessions"][0]["session_id"], "session-old");
        assert_eq!(state["sessions"][1]["session_id"], "session-new");
        assert_eq!(state["native_extension"]["revision"], 7);
    }

    #[test]
    fn legacy_group_workflow_migrates_once_while_configuration_stays_in_group_yaml() {
        let (_temp, home, store, group) = fixture();
        store
            .mutate(&group.group_id, |group| {
                group.extra.insert(
                    "assistants".into(),
                    json!({
                        "assistant":{"enabled":true,"config":{"recognition_language":"ja"},"lifecycle":"idle","health":{"source":"rust"}},
                        "voice_prompt_drafts":{"draft-a":{"request_id":"draft-a","status":"pending"}},
                        "ask_requests":[{"request_id":"ask-a","status":"pending"}]
                    }),
                );
                Ok(())
            })
            .expect("legacy state");
        let path = state_path(&store, &group.group_id).expect("path");
        write_json_committed(
            &path,
            &json!({
                "schema":1,
                "group_id":group.group_id,
                "assistants":{"voice_secretary":{"lifecycle":"working","health":{"source":"python"}}},
                "voice_sessions":{},
                "voice_prompt_drafts":{},
                "voice_prompt_requests":{},
                "voice_ask_requests":{}
            }),
        )
        .expect("canonical state");

        let state = load(&home, &group.group_id).expect("migrate");
        assert_eq!(state["assistant"]["lifecycle"], "working");
        assert_eq!(state["assistant"]["health"]["source"], "python");
        assert_eq!(state["prompt_draft"]["request_id"], "draft-a");
        assert_eq!(state["ask_requests"][0]["request_id"], "ask-a");
        let group = store.load(&group.group_id).expect("group");
        assert_eq!(group.extra["assistants"][ASSISTANT_ID]["enabled"], true);
        assert_eq!(
            group.extra["assistants"][ASSISTANT_ID]["config"]["recognition_language"],
            "ja"
        );
        assert!(group.extra["assistants"].get("assistant").is_none());
        assert!(
            group.extra["assistants"]
                .get("voice_prompt_drafts")
                .is_none()
        );
    }

    #[test]
    fn native_updates_write_python_maps_and_drop_process_observations() {
        let (_temp, home, store, group) = fixture();
        update(&home, &group.group_id, |state| {
            state.insert(
                "assistant".into(),
                json!({
                    "lifecycle":"waiting",
                    "health":{"status":"draft_ready","pid":123,"service":{"port":7777}},
                    "config":{"recognition_language":"ja"}
                }),
            );
            state.insert(
                "voice_prompt_drafts".into(),
                json!({"draft-a":{"request_id":"draft-a","status":"pending"}}),
            );
            state.insert(
                "ask_requests".into(),
                json!([{"request_id":"ask-a","status":"pending"}]),
            );
            state.insert("native_extension".into(), json!({"revision":9}));
            Ok(())
        })
        .expect("update");

        let canonical: Value =
            read_json(&state_path(&store, &group.group_id).expect("path")).expect("canonical");
        assert_eq!(
            canonical["assistants"][ASSISTANT_ID]["lifecycle"],
            "waiting"
        );
        assert_eq!(
            canonical["assistants"][ASSISTANT_ID]["health"]["status"],
            "draft_ready"
        );
        assert!(
            canonical["assistants"][ASSISTANT_ID]["health"]
                .get("pid")
                .is_none()
        );
        assert!(
            canonical["assistants"][ASSISTANT_ID]
                .get("config")
                .is_none()
        );
        assert_eq!(
            canonical["voice_prompt_drafts"]["draft-a"]["status"],
            "pending"
        );
        assert_eq!(
            canonical["voice_ask_requests"]["ask-a"]["status"],
            "pending"
        );
        assert_eq!(canonical[RUST_STATE_KEY]["native_extension"]["revision"], 9);
    }
}
