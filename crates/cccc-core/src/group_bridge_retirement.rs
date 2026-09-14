//! One-time retirement of manual Bridge state. No old connection is replayed or
//! converted into account authority. The stable instance private key is retained.
use crate::{GroupStore, HomeLayout, fs, ledger, settings};
use cccc_contracts::Event;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io};

const RECEIPTS: &str = "group_bridge_receipts.yaml";
const RETIRED_FILES: &[&str] = &[
    "group_bridge_credentials.yaml",
    "group_bridge_pairing.yaml",
    "group_bridge_registrations.yaml",
    "group_bridge_identity.yaml",
    RECEIPTS,
];

#[cfg(test)]
#[path = "group_bridge_retirement_tests.rs"]
mod tests;

/// Historical provenance stays readable but never grants a current route.
pub fn is_retired_message(event: &Event) -> bool {
    event.by.starts_with("group_bridge:")
        || event.data.get("source_platform").and_then(Value::as_str) == Some("group_bridge_session")
        || event.data.get("transport").and_then(Value::as_str) == Some("group_bridge_session")
}

pub fn is_retired_receipt(event: &Event) -> bool {
    event.kind == "chat.cross_group_receipt"
        && event.data.get("transport").and_then(Value::as_str) != Some("connect")
        && (event
            .data
            .get("group_bridge_retired")
            .and_then(Value::as_bool)
            == Some(true)
            || event
                .data
                .get("remote_event_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty()))
}

/// Run after acquiring the daemon Home lock and before accepting operations.
/// An error leaves the remaining source state available for the next startup;
/// any receipts already appended are idempotent. It must not enable old workers.
pub fn retire(home: &HomeLayout) -> io::Result<()> {
    let legacy = settings::load(home)?.extra.get("group_bridge").cloned();
    if legacy.is_none_or(|value| value.is_null())
        && !RETIRED_FILES
            .iter()
            .any(|file| home.root().join(file).exists())
    {
        return Ok(());
    }
    fs::with_exclusive_lock(&home.root().join("group_bridge_state.lock"), || {
        let legacy = settings::load(home)?.extra.get("group_bridge").cloned();
        let mut records = BTreeMap::new();
        if let Some(value) = legacy.filter(|value| !value.is_null()) {
            let object = value.as_object().ok_or_else(invalid_state)?;
            if let Some(deliveries) = object.get("deliveries") {
                for record in deliveries.as_array().ok_or_else(invalid_state)? {
                    records.insert(record_key(record)?, record.clone());
                }
            }
        }
        let canonical: Value = match fs::read_yaml(&home.root().join(RECEIPTS)) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => json!({"receipts":{}}),
            Err(error) => return Err(error),
        };
        // Canonical records win over stale settings shadows. Do not import or
        // normalize pairings/credentials merely to retire their receipts.
        for record in canonical
            .get("receipts")
            .and_then(Value::as_object)
            .ok_or_else(invalid_state)?
            .values()
        {
            records.insert(record_key(record)?, record.clone());
        }
        let store = GroupStore::new(home.clone())?;
        let registered: std::collections::HashSet<_> = store
            .list()?
            .into_iter()
            .map(|group| group.group_id)
            .collect();
        for (key, record) in records {
            project_receipt(&store, &registered, &key, &record)?;
        }
        // No source state is removed until every required receipt/notice exists.
        settings::update(home, |value| {
            value.extra.remove("group_bridge");
            Ok(())
        })?;
        for file in RETIRED_FILES {
            match std::fs::remove_file(home.root().join(file)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })
}

fn invalid_state() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "manual Group Bridge retirement requires intact receipt state; original files were retained",
    )
}

fn text<'a>(record: &'a Value, field: &str) -> Option<&'a str> {
    record
        .get(field)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn record_key(record: &Value) -> io::Result<String> {
    let registration = text(record, "registration_id").ok_or_else(invalid_state)?;
    let idempotency = text(record, "idempotency_key").ok_or_else(invalid_state)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(registration, idempotency)).map_err(io::Error::other)?)
    ))
}

fn project_receipt(
    store: &GroupStore,
    registered: &std::collections::HashSet<String>,
    key: &str,
    record: &Value,
) -> io::Result<()> {
    let status = text(record, "status").ok_or_else(invalid_state)?;
    if status == "sent" && record.get("operation").is_none() {
        // Inbound dedupe receipts have a target event, not an outbound source.
        return text(record, "event_id")
            .map(|_| ())
            .ok_or_else(invalid_state);
    }
    if !matches!(
        status,
        "sent" | "failed" | "queued" | "sending" | "retrying"
    ) {
        return Err(invalid_state());
    }
    let operation = text(record, "operation")
        .filter(|value| matches!(*value, "remote_send" | "reply_request_cancel"))
        .ok_or_else(invalid_state)?;
    let source_group = text(record, "src_group_id").ok_or_else(invalid_state)?;
    let directory = store.group_dir(source_group)?;
    if !registered.contains(source_group) {
        // An explicitly removed Group must never be recreated by retirement.
        return if directory.exists() {
            Err(invalid_state())
        } else {
            Ok(())
        };
    }
    let group = store.load(source_group)?;
    let path = store.ledger_path(source_group)?;
    let source_id = text(record, "source_event_id").ok_or_else(invalid_state)?;
    let source = ledger::find_event(&path, source_id)?.ok_or_else(invalid_state)?;
    let destination = text(record, "dst_group_id").ok_or_else(invalid_state)?;
    let registration = text(record, "registration_id").ok_or_else(invalid_state)?;
    let idempotency = text(record, "idempotency_key").ok_or_else(invalid_state)?;
    let client_id = format!("group-bridge:retired:{key}");
    let existing = ledger::inspect(&path, |events, _| {
        events.iter().any(|event| {
            event.kind == "chat.cross_group_receipt"
                && (event.data.get("client_id").and_then(Value::as_str) == Some(&client_id)
                    || event.data.get("registration_id").and_then(Value::as_str)
                        == Some(registration)
                        && event.data.get("idempotency_key").and_then(Value::as_str)
                            == Some(idempotency)
                        && matches!(
                            event.data.get("status").and_then(Value::as_str),
                            Some("sent" | "failed" | "unconfirmed")
                        ))
        })
    })?;
    if !existing {
        let final_status = match status {
            "sent" => "sent",
            "failed" => "failed",
            "queued" if record.get("attempt").and_then(Value::as_u64) == Some(0) => "failed",
            _ => "unconfirmed",
        };
        if final_status == "sent" && text(record, "remote_event_id").is_none() {
            return Err(invalid_state());
        }
        let mut event = Event::new("chat.cross_group_receipt", source_group);
        event.by = "system".into();
        event.scope_key = source.scope_key.clone();
        event.data = json!({
            "client_id":client_id,"source_event_id":source_id,"operation":operation,
            "dst_group_id":destination,"dst_event_id":"",
            "source_message_event_id":record.get("source_message_event_id"),
            "remote_event_id":record.get("remote_event_id"),
            "registration_id":registration,"idempotency_key":idempotency,
            "status":final_status,"group_bridge_retired":true,
            "error": if final_status == "sent" { Value::Null } else { json!({"code":"group_bridge_retired","message":"Manual Group Bridge has been removed. This operation will not be retried."}) }
        }).as_object().expect("receipt object").clone();
        ledger::append(&path, &event)?;
    }
    // One user notice per Group, including after a prior receipt-only partial
    // write. Never enqueue work for an Actor or broadcast to an external IM.
    let notice_id = "group-bridge:retired";
    if ledger::find_idempotent(&path, "system.notify", "system", notice_id)?.is_none() {
        let mut notice = Event::new("system.notify", source_group);
        notice.by = "system".into();
        notice.scope_key = group.active_scope_key;
        notice.data = Map::from_iter([
            ("client_id".into(), json!(notice_id)),
            ("kind".into(), json!("info")),
            ("target_actor_id".into(), json!("user")),
            ("im_visibility".into(), json!("internal")),
            ("title".into(), json!("Manual Group Bridge retired")),
            (
                "message".into(),
                json!(
                    "Manual Group Bridge has been removed. Old operations will not be retried; their final or unconfirmed results remain in this Group. Use CCCC Connect for same-account collaboration."
                ),
            ),
        ]);
        ledger::append(&path, &notice)?;
    }
    Ok(())
}
