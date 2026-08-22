use super::*;

pub(super) fn deduped(
    home: &HomeLayout,
    group_id: &str,
    destination_group_id: &str,
    existing: &Option<Value>,
) -> Option<OpResult> {
    let mut receipt = existing.clone()?;
    if !matches!(receipt["status"].as_str().unwrap_or(""), "sent" | "failed") {
        return None;
    }
    if receipt["status"] == "sent"
        && let Err(error) = project_success(home, group_id, destination_group_id, &mut receipt)
    {
        tracing::warn!(
            %group_id,
            %destination_group_id,
            error = %error.message,
            "failed to project a deduplicated Group Bridge receipt"
        );
    }
    let source_event = receipt["source_event_id"]
        .as_str()
        .map(|event_id| find_source_event(home, group_id, event_id))
        .unwrap_or(Value::Null);
    Some(object(json!({
        "queued":false,"receipt":receipt,"source_event":source_event,
        "transport":"group_bridge_session","deduped":true
    })))
}

pub(super) fn find_source_event(home: &HomeLayout, group_id: &str, event_id: &str) -> Value {
    GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(group_id))
        .and_then(|path| cccc_core::ledger::find_event(&path, event_id))
        .ok()
        .flatten()
        .and_then(|event| serde_json::to_value(event).ok())
        .unwrap_or(Value::Null)
}

pub(super) fn project_success(
    home: &HomeLayout,
    source_group_id: &str,
    destination_group_id: &str,
    receipt: &mut Value,
) -> Result<(), OpError> {
    if receipt["projected"] == true || receipt["status"] != "sent" {
        return Ok(());
    }
    let Some(operation) = receipt["operation"]
        .as_str()
        .filter(|operation| matches!(*operation, "remote_send" | "reply_request_cancel"))
    else {
        return Ok(());
    };
    let source_event_id = receipt["source_event_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let operation = operation.to_owned();
    let remote_event_id = receipt["remote_event_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let registration_id = receipt["registration_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let idempotency_key = receipt["idempotency_key"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let (
        Some(source_event_id),
        Some(remote_event_id),
        Some(registration_id),
        Some(idempotency_key),
    ) = (
        source_event_id,
        remote_event_id,
        registration_id,
        idempotency_key,
    )
    else {
        return Ok(());
    };

    let path = GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(source_group_id))
        .map_err(OpError::io)?;
    let already_projected = cccc_core::ledger::read_all(&path)
        .map_err(OpError::io)?
        .into_iter()
        .any(|event| {
            event.kind == "chat.cross_group_receipt"
                && event.data.get("registration_id").and_then(Value::as_str)
                    == Some(registration_id.as_str())
                && event.data.get("idempotency_key").and_then(Value::as_str)
                    == Some(idempotency_key.as_str())
                && event.data.get("remote_event_id").and_then(Value::as_str)
                    == Some(remote_event_id.as_str())
        });
    if !already_projected {
        let mut event = cccc_contracts::Event::new("chat.cross_group_receipt", source_group_id);
        event.by = "system".into();
        event.data = json!({
            "source_event_id":source_event_id,
            "operation":operation,
            "dst_group_id":destination_group_id,
            "dst_event_id":"",
            "remote_event_id":remote_event_id,
            "registration_id":registration_id,
            "idempotency_key":idempotency_key,
            "status":"sent"
        })
        .as_object()
        .cloned()
        .expect("cross-group receipt data is an object");
        cccc_core::ledger::append(&path, &event).map_err(OpError::io)?;
    }

    let mut projected = receipt.clone();
    projected["projected"] = json!(true);
    store_delivery(home, projected.clone())?;
    *receipt = projected;
    Ok(())
}
