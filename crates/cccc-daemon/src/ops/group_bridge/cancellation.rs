use cccc_contracts::{DaemonRequest, Event, GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION, utc_now};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg};

use super::state::{bridge_state, find_delivery, items, nonempty, route, store_delivery};

pub(crate) fn propagate(
    home: &HomeLayout,
    source_group_id: &str,
    source_message: &Event,
    cancel_event: &Event,
) -> Value {
    match propagate_inner(home, source_group_id, source_message, cancel_event) {
        Ok(value) => value,
        Err(error) => json!({
            "state":"failed",
            "error":{"code":error.code,"message":error.message}
        }),
    }
}

fn propagate_inner(
    home: &HomeLayout,
    source_group_id: &str,
    source_message: &Event,
    cancel_event: &Event,
) -> Result<Value, OpError> {
    let destination_group_id = source_message
        .data
        .get("dst_group_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if !destination_group_id.is_empty()
        && let Some(destination) = super::super::message_idempotency::find_relay(
            home,
            destination_group_id,
            &source_message.id,
        )
    {
        let event = append_destination_cancel(
            home,
            destination_group_id,
            &destination.id,
            source_group_id,
            &source_message.id,
            &cancel_event.id,
            "system",
        )?;
        return Ok(json!({
            "state":"sent","transport":"local","event":event,
            "event_id":event.id
        }));
    }

    let state = bridge_state(home)?;
    let original = items(&state, "deliveries").iter().find(|receipt| {
        receipt["operation"] == "remote_send"
            && receipt["source_event_id"].as_str() == Some(source_message.id.as_str())
    });
    let Some(original) = original else {
        return Ok(json!({"state":"not_applicable"}));
    };
    let registration_id = original["registration_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            OpError::new(
                "missing_registration_id",
                "source Group Bridge receipt has no registration id",
            )
        })?;
    let idempotency_key = format!("reply-request-cancel:{}", cancel_event.id);
    let payload = json!({
        "source_group_id":source_group_id,
        "source_message_event_id":source_message.id,
        "source_cancel_event_id":cancel_event.id,
        "remote_source_event_id":original["remote_event_id"]
    });
    if find_delivery(&state, registration_id, &idempotency_key).is_none() {
        store_delivery(
            home,
            json!({
                "operation":"reply_request_cancel","ok":false,"status":"queued",
                "registration_id":registration_id,"idempotency_key":idempotency_key,
                "src_group_id":source_group_id,"dst_group_id":destination_group_id,
                "source_event_id":cancel_event.id,
                "source_message_event_id":source_message.id,
                "payload":payload,"transport":"group_bridge_session",
                "attempt":0,"max_attempts":5,"first_queued_at":utc_now(),
                "next_attempt_at":utc_now(),"error":null
            }),
        )?;
    }
    attempt(home, source_group_id, registration_id, &idempotency_key)
}

pub(super) fn retry(
    home: &HomeLayout,
    source_group_id: &str,
    registration_id: &str,
    idempotency_key: &str,
) -> OpResult {
    attempt(home, source_group_id, registration_id, idempotency_key).and_then(|value| {
        value
            .as_object()
            .cloned()
            .ok_or_else(|| OpError::new("internal_error", "cancellation retry result is invalid"))
    })
}

fn attempt(
    home: &HomeLayout,
    source_group_id: &str,
    registration_id: &str,
    idempotency_key: &str,
) -> Result<Value, OpError> {
    let state = bridge_state(home)?;
    let existing = find_delivery(&state, registration_id, idempotency_key)
        .ok_or_else(|| OpError::new("delivery_not_found", "cancellation receipt was not found"))?;
    if matches!(existing["status"].as_str().unwrap_or(""), "sent" | "failed") {
        return Ok(json!({"state":existing["status"],"receipt":existing}));
    }
    let route = route(&state, registration_id, source_group_id)?;
    let source_message_event_id = existing["source_message_event_id"]
        .as_str()
        .or_else(|| {
            existing
                .pointer("/payload/source_message_event_id")
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    let original = items(&state, "deliveries").iter().find(|receipt| {
        receipt["operation"] == "remote_send"
            && receipt["registration_id"] == registration_id
            && receipt["source_event_id"].as_str() == Some(source_message_event_id)
    });
    let remote_source_event_id = original
        .and_then(|receipt| receipt["remote_event_id"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if remote_source_event_id.is_none() {
        if original.is_some_and(|receipt| receipt["status"] == "failed") {
            let mut completed = existing;
            completed["ok"] = json!(true);
            completed["status"] = json!("sent");
            completed["remote_not_created"] = json!(true);
            completed["updated_at"] = json!(utc_now());
            store_delivery(home, completed.clone())?;
            return Ok(json!({"state":"sent","receipt":completed}));
        }
        let mut deferred = existing;
        deferred["status"] = json!("retrying");
        let deferred_at = utc_now();
        deferred["updated_at"] = json!(deferred_at);
        deferred["next_attempt_at"] = json!(super::next_retry_at(
            deferred["attempt"].as_u64().unwrap_or(0),
            &deferred_at,
        ));
        deferred["error"] = json!({
            "code":"source_delivery_pending",
            "message":"source message delivery has not produced a remote event yet",
            "retriable":true,"transport":"group_bridge_session"
        });
        store_delivery(home, deferred.clone())?;
        return Ok(json!({"state":"retrying","receipt":deferred}));
    }

    let remote_group_id = nonempty(&route, &["remote_group_id", "target_group_id"])
        .ok_or_else(|| OpError::new("missing_remote_group_id", "remote group id is required"))?;
    let remote_peer_id = route["remote_peer_id"].as_str().unwrap_or("");
    let mut payload = existing["payload"].as_object().cloned().unwrap_or_default();
    payload.insert(
        "remote_source_event_id".into(),
        json!(remote_source_event_id.unwrap_or_default()),
    );
    let attempt = existing["attempt"].as_u64().unwrap_or(0) + 1;
    let max_attempts = existing["max_attempts"].as_u64().unwrap_or(5);
    let mut sending = existing;
    sending["status"] = json!("sending");
    sending["attempt"] = json!(attempt);
    sending["payload"] = json!(payload);
    sending["last_attempt_at"] = json!(utc_now());
    sending["error"] = Value::Null;
    store_delivery(home, sending.clone())?;

    let request = json!({
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "op":"reply_request_cancel","src_group_id":source_group_id,
        "target_group_id":remote_group_id,"remote_peer_id":remote_peer_id,
        "idempotency_key":idempotency_key,"payload":payload
    });
    let via_owner = super::session_runtime::deliver(
        home,
        &DaemonRequest {
            v: 1,
            op: "group_bridge_session_deliver".into(),
            args: json!({
                "group_id":source_group_id,"remote_group_id":remote_group_id,
                "remote_peer_id":remote_peer_id,"operation":"reply_request_cancel",
                "idempotency_key":idempotency_key,"payload":payload,"timeout_ms":5_000
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        },
    )
    .ok()
    .map(Value::Object)
    .filter(super::delivery_succeeded)
    .or_else(|| {
        crate::group_bridge_sessions::send(
            source_group_id,
            &remote_group_id,
            remote_peer_id,
            request,
        )
        .filter(super::delivery_succeeded)
    });
    let remote = via_owner.map(Ok).unwrap_or_else(|| {
        match (
            nonempty(&route, &["remote_endpoint", "endpoint", "url"]),
            nonempty(&route, &["credential", "token"]),
        ) {
            (Some(endpoint), Some(credential)) => super::post_delivery(
                &endpoint,
                &credential,
                "reply_request_cancel",
                idempotency_key,
                &payload,
            ),
            _ => Err(OpError::new(
                "peer_session_unavailable",
                "no live Group Bridge session and no authenticated HTTP fallback",
            )),
        }
    });
    let remote = match remote {
        Ok(remote) => remote,
        Err(error) => {
            let terminal = attempt >= max_attempts;
            let mut failed = sending;
            let failed_at = utc_now();
            failed["status"] = json!(if terminal { "failed" } else { "retrying" });
            failed["updated_at"] = json!(failed_at);
            failed["next_attempt_at"] = json!(if terminal {
                String::new()
            } else {
                super::next_retry_at(attempt, &failed_at)
            });
            failed["error"] = json!({
                "code":error.code,"message":error.message,
                "retriable":!terminal,"transport":"group_bridge_session"
            });
            store_delivery(home, failed.clone())?;
            return Ok(json!({"state":failed["status"],"receipt":failed}));
        }
    };
    let remote_event_id = remote
        .pointer("/result/event/id")
        .or_else(|| remote.pointer("/result/event_id"))
        .or_else(|| remote.get("event_id"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut receipt = sending;
    receipt["ok"] = json!(true);
    receipt["status"] = json!("sent");
    receipt["next_attempt_at"] = json!("");
    receipt["remote_event_id"] = remote_event_id;
    receipt["updated_at"] = json!(utc_now());
    store_delivery(home, receipt.clone())?;
    let mut projected = receipt.clone();
    super::result_projection::project_success(
        home,
        source_group_id,
        &remote_group_id,
        &mut projected,
    )?;
    Ok(json!({"state":"sent","receipt":projected}))
}

pub(super) fn receive(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let target_group_id = required_arg(request, "target_group_id")?;
    let src_group_id = required_arg(request, "src_group_id")?;
    let remote_peer_id = required_arg(request, "remote_peer_id")?;
    let payload = request
        .args
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| OpError::new("invalid_payload", "cancellation payload must be an object"))?;
    if payload.len() != 4
        || payload.keys().any(|field| {
            !matches!(
                field.as_str(),
                "source_group_id"
                    | "source_message_event_id"
                    | "source_cancel_event_id"
                    | "remote_source_event_id"
            )
        })
    {
        return Err(OpError::new(
            "invalid_payload",
            "cancellation payload contains unsupported fields",
        ));
    }
    let state = bridge_state(home)?;
    let trusted = items(&state, "trusts").iter().any(|trust| {
        trust["status"] == "active"
            && trust["group_id"] == target_group_id
            && trust["remote_group_id"] == src_group_id
            && trust["remote_peer_id"] == remote_peer_id
    });
    if !trusted {
        return Err(OpError::new(
            "unauthorized_peer",
            "remote peer is not trusted for this group",
        ));
    }
    let payload_source_group = required_payload(&payload, "source_group_id")?;
    if payload_source_group != src_group_id {
        return Err(OpError::new(
            "source_group_mismatch",
            "cancellation source group does not match the session",
        ));
    }
    let event = append_destination_cancel(
        home,
        &target_group_id,
        &required_payload(&payload, "remote_source_event_id")?,
        &src_group_id,
        &required_payload(&payload, "source_message_event_id")?,
        &required_payload(&payload, "source_cancel_event_id")?,
        &format!("group_bridge:{remote_peer_id}"),
    )?;
    object(json!({"ok":true,"event_id":event.id,"event":event}))
}

fn append_destination_cancel(
    home: &HomeLayout,
    target_group_id: &str,
    remote_source_event_id: &str,
    src_group_id: &str,
    source_message_event_id: &str,
    source_cancel_event_id: &str,
    by: &str,
) -> Result<Event, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(target_group_id).map_err(OpError::io)?;
    let source = ledger::find_event(&path, remote_source_event_id)
        .map_err(OpError::io)?
        .ok_or_else(|| {
            OpError::new(
                "event_not_found",
                "relayed request-reply event was not found",
            )
        })?;
    if source.kind != "chat.message"
        || source.data.get("message_mode").and_then(Value::as_str) != Some("request_reply")
        || source.data.get("src_group_id").and_then(Value::as_str) != Some(src_group_id)
        || source.data.get("src_event_id").and_then(Value::as_str) != Some(source_message_event_id)
    {
        return Err(OpError::new(
            "source_event_mismatch",
            "relayed request-reply source does not match the cancellation provenance",
        ));
    }
    if let Some(existing) = ledger::read_all(&path)
        .map_err(OpError::io)?
        .into_iter()
        .find(|event| {
            event.kind == "chat.reply_request.cancelled"
                && event.data.get("source_event_id").and_then(Value::as_str)
                    == Some(remote_source_event_id)
        })
    {
        return Ok(existing);
    }
    let mut event = Event::new("chat.reply_request.cancelled", target_group_id);
    event.by = by.into();
    event.data = json!({
        "source_event_id":remote_source_event_id,
        "src_group_id":src_group_id,
        "src_event_id":source_cancel_event_id,
        "src_message_event_id":source_message_event_id
    })
    .as_object()
    .cloned()
    .expect("reply cancellation data is an object");
    ledger::append(&path, &event).map_err(OpError::io)?;
    Ok(event)
}

fn required_payload(payload: &Map<String, Value>, field: &str) -> Result<String, OpError> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| OpError::new("invalid_payload", format!("{field} is required")))
}
