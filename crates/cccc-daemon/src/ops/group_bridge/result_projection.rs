use super::*;

pub(super) fn deduped(
    home: &HomeLayout,
    group_id: &str,
    existing: &Option<Value>,
) -> Option<OpResult> {
    if !existing.as_ref().is_some_and(|receipt| {
        matches!(
            receipt["status"].as_str().unwrap_or(""),
            "delivered" | "sent" | "failed"
        )
    }) {
        return None;
    }
    let source_event = existing
        .as_ref()
        .and_then(|receipt| receipt["source_event_id"].as_str())
        .and_then(|event_id| {
            GroupStore::new(home.clone())
                .and_then(|store| store.ledger_path(group_id))
                .and_then(|path| cccc_core::ledger::find_event(&path, event_id))
                .ok()
                .flatten()
        })
        .and_then(|event| serde_json::to_value(event).ok())
        .unwrap_or(Value::Null);
    Some(object(json!({
        "queued":false,"receipt":existing,"source_event":source_event,
        "transport":"group_bridge_session","deduped":true
    })))
}

pub(super) fn persist_source_event(
    home: &HomeLayout,
    receipt: &mut Value,
    source_event: &Value,
) -> Result<(), OpError> {
    let Some(source_event_id) = source_event["id"].as_str() else {
        return Ok(());
    };
    receipt["source_event_id"] = json!(source_event_id);
    store_delivery(home, receipt.clone())
}
