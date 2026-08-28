use cccc_contracts::{DaemonRequest, GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION, utc_now};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::dispatch::{OpError, OpResult, object, required_arg};

pub(super) mod cancellation;
mod payload;
mod reply;
mod result_projection;
mod session_runtime;
mod state;
#[cfg(test)]
mod tests;

use payload::{
    normalize_outbound_payload, remote_reply_recipients, store_remote_attachments,
    validate_remote_payload,
};
use state::{
    bridge_state, dispatch_message, find_delivery, items, nonempty, route, store_delivery,
};

type RetryKey = (String, String, String, String);

static ACTIVE_RETRIES: OnceLock<Mutex<HashSet<RetryKey>>> = OnceLock::new();
const SENDING_STALE_SECONDS: i64 = 120;

pub(super) fn route_ready(home: &HomeLayout, trust: &Value) -> bool {
    session_runtime::route_ready(home, trust)
}

pub(super) fn preflight_upload(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let destination_id = required_arg(request, "dst_group_id")?;
    if group_id == destination_id {
        return Err(OpError::new(
            "invalid_dst_group_id",
            "dst_group_id must be different from group_id",
        ));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let source = store.load(&group_id).map_err(OpError::not_found)?;
    let by = request
        .args
        .get("by")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("user");
    cccc_core::permissions::require_group(&source, by)
        .map_err(|error| OpError::new("permission_denied", error.to_string()))?;
    let state = bridge_state(home)?;
    let trust = items(&state, "trusts").iter().find(|trust| {
        trust["status"] == "active"
            && trust["group_id"] == group_id
            && trust["remote_group_id"] == destination_id
    });
    let Some(trust) = trust else {
        if store.load(&destination_id).is_ok() {
            return Err(OpError::new(
                "attachments_not_supported",
                "attachments are only supported for remote Group Bridge messages",
            ));
        }
        return Err(OpError::new(
            "group_bridge_route_not_found",
            "no active Group Bridge route exists for the destination group",
        ));
    };
    if !matches!(
        trust["remote_access_level"].as_str().unwrap_or("messages"),
        "messages" | "read" | "full"
    ) {
        return Err(OpError::new(
            "permission_denied",
            "remote trust does not allow messages",
        ));
    }
    let mut payload = Map::new();
    for field in [
        "text",
        "format",
        "message_mode",
        "to",
        "refs",
        "priority",
        "reply_required",
        "requires_ack",
    ] {
        if let Some(value) = request.args.get(field).cloned() {
            payload.insert(field.into(), value);
        }
    }
    if payload.get("to").is_none()
        || payload
            .get("to")
            .and_then(Value::as_array)
            .is_some_and(|recipients| recipients.is_empty())
    {
        payload.insert("to".into(), json!(["@foreman"]));
    }
    if request
        .args
        .get("has_attachments")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        payload.insert("attachments".into(), json!([{"kind":"file"}]));
    }
    normalize_outbound_payload(request, &mut payload)?;
    object(json!({"ready":true}))
}

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "remote_send" => remote_send(home, request),
        "remote_delivery_status" => delivery_status(home, request),
        "group_bridge_receive_remote_send" => receive(home, request),
        "group_bridge_receive_reply_request_cancel" => cancellation::receive(home, request),
        "group_bridge_session_open" => session_runtime::open(home, request),
        "group_bridge_session_close" => session_runtime::close(home, request),
        "group_bridge_session_poll" => session_runtime::poll(home, request),
        "group_bridge_session_complete" => session_runtime::complete(home, request),
        "group_bridge_session_ready" => session_runtime::ready(home, request),
        "group_bridge_session_deliver" => session_runtime::deliver(home, request),
        _ => return None,
    })
}

fn remote_send(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    remote_send_inner(home, request, true)
}

pub(crate) fn schedule_pending_route_retry(
    home: HomeLayout,
    group_id: String,
    remote_group_id: String,
    remote_peer_id: String,
) {
    schedule_route_retry(home, group_id, remote_group_id, remote_peer_id, false);
}

pub(crate) fn schedule_due_retries(home: HomeLayout) {
    let Ok(state) = bridge_state(&home) else {
        return;
    };
    let now = chrono::Utc::now();
    let due_registrations = items(&state, "deliveries")
        .iter()
        .filter(|receipt| receipt_retry_due(receipt, now, true))
        .filter_map(|receipt| nonempty(receipt, &["registration_id"]))
        .collect::<HashSet<_>>();
    if due_registrations.is_empty() {
        return;
    }
    let routes = ["outbounds", "trusts", "registrations"]
        .into_iter()
        .flat_map(|section| items(&state, section))
        .filter(|trust| trust["status"] == "active")
        .filter(|trust| {
            nonempty(trust, &["registration_id", "trust_id"])
                .is_some_and(|id| due_registrations.contains(&id))
        })
        .filter_map(|trust| {
            Some((
                nonempty(trust, &["group_id"])?,
                nonempty(trust, &["remote_group_id"])?,
                nonempty(trust, &["remote_peer_id"])?,
            ))
        })
        .collect::<HashSet<_>>();
    for (group_id, remote_group_id, remote_peer_id) in routes {
        schedule_route_retry(
            home.clone(),
            group_id,
            remote_group_id,
            remote_peer_id,
            true,
        );
    }
}

fn schedule_route_retry(
    home: HomeLayout,
    group_id: String,
    remote_group_id: String,
    remote_peer_id: String,
    due_only: bool,
) {
    let key = (
        home.root().to_string_lossy().into_owned(),
        group_id.clone(),
        remote_group_id.clone(),
        remote_peer_id.clone(),
    );
    let active = ACTIVE_RETRIES.get_or_init(|| Mutex::new(HashSet::new()));
    if !active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key.clone())
    {
        return;
    }
    std::thread::spawn(move || {
        retry_pending_for_route(
            &home,
            &group_id,
            &remote_group_id,
            &remote_peer_id,
            due_only,
        );
        ACTIVE_RETRIES
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
    });
}

fn retry_pending_for_route(
    home: &HomeLayout,
    group_id: &str,
    remote_group_id: &str,
    remote_peer_id: &str,
    due_only: bool,
) {
    let Ok(state) = bridge_state(home) else {
        return;
    };
    let now = chrono::Utc::now();
    let route_ids = ["outbounds", "trusts", "registrations"]
        .into_iter()
        .flat_map(|section| items(&state, section))
        .filter(|trust| {
            trust["status"] == "active"
                && trust["group_id"] == group_id
                && trust["remote_group_id"] == remote_group_id
                && trust["remote_peer_id"] == remote_peer_id
        })
        .filter_map(|trust| nonempty(trust, &["registration_id", "trust_id"]))
        .collect::<HashSet<_>>();
    if route_ids.is_empty() {
        return;
    }
    let mut pending = items(&state, "deliveries")
        .iter()
        .filter(|receipt| {
            receipt["registration_id"]
                .as_str()
                .is_some_and(|id| route_ids.contains(id))
                && receipt_retry_due(receipt, now, due_only)
        })
        .cloned()
        .collect::<Vec<_>>();
    pending.sort_by_key(|receipt| (receipt["operation"] == "reply_request_cancel") as u8);
    for receipt in pending {
        let Some(registration_id) = receipt["registration_id"].as_str() else {
            continue;
        };
        let Some(idempotency_key) = receipt["idempotency_key"].as_str() else {
            continue;
        };
        if receipt["operation"].as_str() == Some("reply_request_cancel") {
            let _ = cancellation::retry(home, group_id, registration_id, idempotency_key);
            continue;
        }
        let payload = receipt
            .get("source_record_payload")
            .filter(|value| value.is_object())
            .or_else(|| receipt.get("payload").filter(|value| value.is_object()))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut args = json!({
            "group_id":group_id,
            "registration_id":registration_id,
            "idempotency_key":idempotency_key,
            "by":payload.get("source_by").cloned().unwrap_or_else(|| json!("user")),
            "payload":payload
        })
        .as_object()
        .cloned()
        .expect("retry request is an object");
        if let Some(value) = receipt.get("source_event_id").cloned() {
            args.insert("source_event_id".into(), value);
        }
        let _ = remote_send(
            home,
            &DaemonRequest {
                v: 1,
                op: "remote_send".into(),
                args,
            },
        );
    }
}

fn receipt_retry_due(receipt: &Value, now: chrono::DateTime<chrono::Utc>, due_only: bool) -> bool {
    let attempt = receipt["attempt"].as_u64().unwrap_or(0);
    if attempt >= receipt["max_attempts"].as_u64().unwrap_or(5) {
        return false;
    }
    match receipt["status"].as_str().unwrap_or("") {
        "queued" | "retrying" if !due_only => true,
        "queued" | "retrying" => {
            timestamp(receipt, "next_attempt_at").is_none_or(|next_attempt| next_attempt <= now)
        }
        "sending" => timestamp(receipt, "last_attempt_at")
            .is_none_or(|last_attempt| (now - last_attempt).num_seconds() >= SENDING_STALE_SECONDS),
        _ => false,
    }
}

fn timestamp(receipt: &Value, field: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(receipt[field].as_str()?.trim())
        .ok()
        .map(|value| value.with_timezone(&chrono::Utc))
}

fn next_retry_at(attempt: u64, attempted_at: &str) -> String {
    const BACKOFF_SECONDS: [i64; 5] = [2, 5, 15, 30, 60];
    let base = chrono::DateTime::parse_from_rfc3339(attempted_at)
        .map(|value| value.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let index = attempt.saturating_sub(1) as usize;
    let seconds = BACKOFF_SECONDS[index.min(BACKOFF_SECONDS.len() - 1)];
    (base + chrono::Duration::seconds(seconds)).to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

pub(super) fn prepare_reply(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    target: &cccc_contracts::Event,
    request: &DaemonRequest,
    message_mode: &str,
) -> Result<Option<reply::PreparedReply>, OpError> {
    reply::prepare(home, group, target, request, message_mode)
}

fn remote_send_without_source_record(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    remote_send_inner(home, request, false)
}

fn remote_send_inner(home: &HomeLayout, request: &DaemonRequest, record_source: bool) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let registration_id = required_arg(request, "registration_id")?;
    let idempotency_key = required_arg(request, "idempotency_key")?;
    let mut payload = request
        .args
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| OpError::new("invalid_payload", "payload must be an object"))?;
    normalize_outbound_payload(request, &mut payload)?;
    let state = bridge_state(home)?;
    let route = route(&state, &registration_id, &group_id)?;
    if !matches!(
        route["remote_access_level"].as_str().unwrap_or("messages"),
        "messages" | "read" | "full"
    ) {
        return Err(OpError::new(
            "permission_denied",
            "remote trust does not allow messages",
        ));
    }
    let endpoint = nonempty(&route, &["remote_endpoint", "endpoint", "url"]);
    let credential = nonempty(&route, &["credential", "token"]);
    let remote_group_id =
        nonempty(&route, &["remote_group_id", "target_group_id"]).unwrap_or_default();
    let existing = find_delivery(&state, &registration_id, &idempotency_key);
    if existing
        .as_ref()
        .is_some_and(|receipt| receipt["operation"] != "remote_send")
    {
        return Err(OpError::new(
            "contract_version_mismatch",
            "Group Bridge delivery does not use the current operation contract",
        ));
    }
    if let Some(result) = result_projection::deduped(home, &group_id, &remote_group_id, &existing) {
        return result;
    }
    let attempt = existing
        .as_ref()
        .and_then(|receipt| receipt["attempt"].as_u64())
        .unwrap_or(0)
        + 1;
    let title = GroupStore::new(home.clone())
        .and_then(|store| store.load(&group_id))
        .map(|group| group.title)
        .unwrap_or_default();
    let stored_body = existing
        .as_ref()
        .and_then(|receipt| receipt["payload"].as_object())
        .cloned();
    let stored_record = existing
        .as_ref()
        .and_then(|receipt| receipt["source_record_payload"].as_object())
        .cloned();
    let mut record_payload = stored_record.unwrap_or_else(|| payload.clone());
    let mut body = if let Some(mut stored_body) = stored_body {
        validate_remote_payload(&mut stored_body)?;
        stored_body
    } else {
        if let Some(insight) = request.args.get("insight").cloned() {
            record_payload.insert("insight".into(), insight);
        }
        if let Some(required) = request.args.get("require_peer_insight").cloned() {
            record_payload.insert("require_peer_insight".into(), required);
        }
        let mut body = payload;
        payload::encode_outbound_attachments(home, &group_id, &mut body)?;
        let insight = cccc_core::peer_insight::normalize(request.args.get("insight"))
            .map_err(|message| OpError::new("invalid_insight", message))?;
        if insight.is_some() {
            let text = body.get("text").and_then(Value::as_str).unwrap_or("");
            let insight_value = insight.as_ref().map(|value| Value::String(value.clone()));
            body.insert(
                "text".into(),
                json!(cccc_core::peer_insight::append_to_delivery(
                    text,
                    insight_value.as_ref()
                )),
            );
        }
        body.insert("source_group_id".into(), json!(group_id));
        body.insert("src_group_id".into(), json!(group_id));
        body.insert("source_group_title".into(), json!(title));
        body.insert(
            "source_by".into(),
            request
                .args
                .get("by")
                .cloned()
                .unwrap_or_else(|| json!("user")),
        );
        body.insert("idempotency_key".into(), json!(idempotency_key));
        body.insert(
            "message_contract_version".into(),
            json!(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION),
        );
        if let Some(value) = request.args.get("source_event_id").cloned() {
            body.insert("src_event_id".into(), value);
        }
        if let Some(value) = request.args.get("reply_to_remote_event_id").cloned() {
            body.insert("reply_to".into(), value);
        }
        body
    };
    if !body.contains_key("source_group_id") {
        body.insert("source_group_id".into(), json!(group_id));
    }
    if !body.contains_key("src_group_id") {
        body.insert("src_group_id".into(), json!(group_id));
    }
    if !body.contains_key("source_group_title") {
        body.insert("source_group_title".into(), json!(title));
    }
    if !body.contains_key("source_by") {
        body.insert(
            "source_by".into(),
            record_payload
                .get("source_by")
                .cloned()
                .or_else(|| request.args.get("by").cloned())
                .unwrap_or_else(|| json!("user")),
        );
    }
    if !body.contains_key("idempotency_key") {
        body.insert("idempotency_key".into(), json!(idempotency_key));
    }
    body.insert(
        "message_contract_version".into(),
        json!(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION),
    );
    let persisted_source_event_id = existing
        .as_ref()
        .and_then(|receipt| receipt["source_event_id"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_source_event_id = request
        .args
        .get("source_event_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let source_event =
        if let Some(event_id) = persisted_source_event_id.or(requested_source_event_id) {
            validated_source_event(home, &group_id, &remote_group_id, event_id, &record_payload)?
        } else if record_source {
            ensure_source_event(home, &remote_group_id, &record_payload, &body, request)?
        } else {
            Value::Null
        };
    let source_event_id = source_event["id"]
        .as_str()
        .or_else(|| request.args.get("source_event_id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if let Some(source_event_id) = source_event_id.as_ref() {
        body.insert("src_event_id".into(), json!(source_event_id));
    }
    let attempted_at = utc_now();
    let mut sending_receipt = json!({
        "operation":"remote_send","ok":false,"status":"sending",
        "registration_id":registration_id,
        "idempotency_key":idempotency_key,"remote_group_id":remote_group_id,
        "src_group_id":group_id,"dst_group_id":remote_group_id,
        "transport":"group_bridge_session","attempt":attempt,"max_attempts":5,
        "payload":body,"source_record_payload":record_payload,
        "last_attempt_at":attempted_at,"next_attempt_at":"","error":null
    });
    if let Some(source_event_id) = source_event_id.as_ref() {
        sending_receipt["source_event_id"] = json!(source_event_id);
    }
    store_delivery(home, sending_receipt.clone())?;
    let session_request = json!({
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "op":"remote_send",
        "src_group_id":group_id,
        "target_group_id":remote_group_id,
        "remote_peer_id":route["remote_peer_id"],
        "idempotency_key":idempotency_key,
        "payload":body.clone()
    });
    let daemon_session_remote = session_runtime::deliver(
        home,
        &DaemonRequest {
            v: 1,
            op: "group_bridge_session_deliver".into(),
            args: json!({
                "group_id":group_id,
                "remote_group_id":remote_group_id,
                "remote_peer_id":route["remote_peer_id"],
                "operation":"remote_send",
                "idempotency_key":idempotency_key,
                "payload":body,
                "timeout_ms":5_000
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        },
    )
    .ok()
    .map(Value::Object)
    .filter(delivery_succeeded);
    let outgoing_session_remote = daemon_session_remote.or_else(|| {
        crate::group_bridge_sessions::send(
            &group_id,
            &remote_group_id,
            route["remote_peer_id"].as_str().unwrap_or(""),
            session_request,
        )
        .filter(delivery_succeeded)
    });
    let remote_result = outgoing_session_remote.map(Ok).unwrap_or_else(|| {
        match (endpoint.as_deref(), credential.as_deref()) {
            (Some(endpoint), Some(credential)) => {
                post_delivery(endpoint, credential, "remote_send", &idempotency_key, &body)
            }
            _ => Err(OpError::new(
                "peer_session_unavailable",
                "no live Group Bridge session and no authenticated HTTP fallback",
            )),
        }
    });
    let remote = match remote_result {
        Ok(remote) => remote,
        Err(error) => {
            let terminal = attempt >= 5;
            let failed_at = utc_now();
            let mut receipt = json!({
                "operation":"remote_send",
                "ok":false,"status":if terminal{"failed"}else{"retrying"},
                "registration_id":registration_id,"idempotency_key":idempotency_key,
                "remote_group_id":remote_group_id,"src_group_id":group_id,
                "dst_group_id":remote_group_id,"transport":"group_bridge_session",
                "attempt":attempt,"max_attempts":5,"payload":body,
                "source_record_payload":record_payload,
                "last_attempt_at":failed_at,"updated_at":failed_at,
                "next_attempt_at":if terminal { String::new() } else { next_retry_at(attempt, &failed_at) },
                "error":{"code":error.code,"message":error.message,
                    "retriable":!terminal,"transport":"group_bridge_session"}
            });
            if let Some(source_event_id) = source_event_id.as_ref() {
                receipt["source_event_id"] = json!(source_event_id);
            }
            store_delivery(home, receipt.clone())?;
            return object(json!({
                "queued":!terminal,"receipt":receipt,"source_event":source_event,
                "deduped":false
            }));
        }
    };
    let peer_receipt = remote
        .pointer("/result/receipt")
        .or_else(|| remote.get("receipt"))
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "status":"sent",
                "remote_event_id":remote
                    .pointer("/result/event_id")
                    .or_else(|| remote.get("event_id"))
                    .cloned()
                    .unwrap_or(Value::Null)
            })
        });
    let remote_event_id = peer_receipt
        .get("remote_event_id")
        .or_else(|| peer_receipt.get("event_id"))
        .cloned()
        .or_else(|| {
            remote
                .pointer("/result/event/id")
                .or_else(|| remote.pointer("/result/event_id"))
                .or_else(|| remote.get("event_id"))
                .cloned()
        })
        .unwrap_or(Value::Null);
    let mut receipt = sending_receipt;
    receipt["status"] = json!("sent");
    receipt["next_attempt_at"] = json!("");
    receipt["remote_event_id"] = remote_event_id;
    if let Some(transport) = peer_receipt["transport"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        receipt["transport"] = json!(transport);
    }
    receipt["ok"] = json!(true);
    receipt["attempt"] = json!(attempt);
    receipt["max_attempts"] = json!(5);
    receipt["updated_at"] = json!(utc_now());
    store_delivery(home, receipt.clone())?;
    if let Err(error) =
        result_projection::project_success(home, &group_id, &remote_group_id, &mut receipt)
    {
        tracing::warn!(
            %group_id,
            destination_group_id = %remote_group_id,
            error = %error.message,
            "failed to project a successful Group Bridge receipt"
        );
    }
    object(json!({
        "queued":false,"receipt":receipt,"source_event":source_event,
        "transport":"group_bridge_session","deduped":false
    }))
}

fn validated_source_event(
    home: &HomeLayout,
    source_group_id: &str,
    destination_group_id: &str,
    source_event_id: &str,
    remote_payload: &Map<String, Value>,
) -> Result<Value, OpError> {
    let event = result_projection::find_source_event(home, source_group_id, source_event_id);
    if event["id"].as_str() != Some(source_event_id) {
        return Err(OpError::new(
            "source_event_not_found",
            "Group Bridge source event was not found in the source group ledger",
        ));
    }
    if event["kind"] != "chat.message"
        || event["data"]["dst_group_id"].as_str() != Some(destination_group_id)
        || event["data"]["to"] != json!(["user"])
        || event["data"]["message_mode"] != "send"
        || event["data"]["dst_to"] != remote_payload["to"]
        || event["data"]["dst_message_mode"] != remote_payload["message_mode"]
    {
        return Err(OpError::new(
            "source_event_mismatch",
            "Group Bridge source event does not match the canonical remote destination record",
        ));
    }
    Ok(event)
}

fn ensure_source_event(
    home: &HomeLayout,
    destination_group_id: &str,
    record_payload: &Map<String, Value>,
    outbound_payload: &Map<String, Value>,
    request: &DaemonRequest,
) -> Result<Value, OpError> {
    let source_group_id = required_arg(request, "group_id")?;
    let registration_id = required_arg(request, "registration_id")?;
    let idempotency_key = required_arg(request, "idempotency_key")?;
    let mut record = record_payload.clone();
    record.remove("source_by");
    record.insert("group_id".into(), json!(source_group_id));
    record.insert("dst_group_id".into(), json!(destination_group_id));
    record.insert("by".into(), outbound_payload["source_by"].clone());
    record.insert(
        "client_id".into(),
        json!(source_client_id(&registration_id, &idempotency_key)),
    );
    let event = dispatch_message(home, "send_cross_group_remote_record", record)?
        .get("source_event")
        .filter(|event| event.is_object())
        .cloned()
        .ok_or_else(|| {
            OpError::new(
                "source_event_missing",
                "Group Bridge source message was not persisted",
            )
        })?;
    let event_id = event["id"].as_str().ok_or_else(|| {
        OpError::new(
            "source_event_missing",
            "Group Bridge source message has no id",
        )
    })?;
    validated_source_event(
        home,
        &source_group_id,
        destination_group_id,
        event_id,
        record_payload,
    )
}

fn source_client_id(registration_id: &str, idempotency_key: &str) -> String {
    let digest = Sha256::digest(format!("{registration_id}\0{idempotency_key}"));
    format!("group-bridge-source:{digest:.32x}")
}

fn delivery_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let registration_id = required_arg(request, "registration_id")?;
    let idempotency_key = required_arg(request, "idempotency_key")?;
    let state = bridge_state(home)?;
    let _ = route(&state, &registration_id, &group_id)?;
    object(json!({"receipt":find_delivery(&state, &registration_id, &idempotency_key)}))
}

fn receive(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let target_group_id = required_arg(request, "target_group_id")?;
    let src_group_id = required_arg(request, "src_group_id")?;
    let remote_peer_id = required_arg(request, "remote_peer_id")?;
    let idempotency_key = required_arg(request, "idempotency_key")?;
    let mut payload = request
        .args
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| OpError::new("invalid_payload", "payload must be an object"))?;
    if payload
        .get("message_contract_version")
        .and_then(Value::as_u64)
        != Some(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION)
    {
        return Err(OpError::new(
            "contract_version_mismatch",
            "Group Bridge message contract version does not match",
        ));
    }
    payload.remove("message_contract_version");
    payload.remove("op");
    validate_remote_payload(&mut payload)?;
    let state = bridge_state(home)?;
    let registration = items(&state, "registrations")
        .iter()
        .chain(items(&state, "trusts").iter())
        .find(|item| {
            item["status"] == "active"
                && item["group_id"] == target_group_id
                && item["remote_group_id"] == src_group_id
                && item["remote_peer_id"] == remote_peer_id
        })
        .ok_or_else(|| {
            OpError::new(
                "registration_not_found",
                "active Group Bridge registration not found",
            )
        })?;
    let registration_id = registration["registration_id"].as_str().unwrap_or("");
    if let Some(receipt) = find_delivery(&state, registration_id, &idempotency_key) {
        return object(json!({"ok":true,"receipt":receipt,"deduped":true}));
    }
    let trust = items(&state, "trusts").iter().find(|item| {
        item["status"] == "active"
            && item["group_id"] == target_group_id
            && item["remote_group_id"] == src_group_id
            && item["remote_peer_id"] == remote_peer_id
    });
    let source_by = payload
        .get("source_by")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    let src_event_id = payload
        .get("src_event_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    store_remote_attachments(home, &target_group_id, &mut payload)?;
    let mut args = payload;
    args.insert("group_id".into(), json!(target_group_id));
    args.insert("by".into(), json!(format!("group_bridge:{remote_peer_id}")));
    args.insert("source_group_id".into(), json!(src_group_id));
    args.insert("src_group_id".into(), json!(src_group_id));
    args.insert("src_event_id".into(), json!(src_event_id));
    args.insert("src_by".into(), json!(source_by));
    args.insert("source_platform".into(), json!("group_bridge_session"));
    args.insert(
        "source_user_name".into(),
        trust
            .and_then(|item| item["remote_group_title"].as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or(&src_group_id)
            .into(),
    );
    args.insert("source_user_id".into(), json!(remote_peer_id));
    let remote_reply_to = remote_reply_recipients(&source_by);
    if !remote_reply_to.is_empty() {
        args.insert("remote_reply_to".into(), json!(remote_reply_to));
    }
    args.insert("client_id".into(), json!(idempotency_key));
    args.remove("source_by");
    args.remove("idempotency_key");
    let target_group = GroupStore::new(home.clone())
        .and_then(|store| store.load(&target_group_id))
        .map_err(OpError::io)?;
    super::messaging_recipients::apply_cross_group_recipient(&target_group, &mut args)?;
    let result = dispatch_message(home, "send", args)?;
    let receipt = json!({
        "registration_id":registration_id,"idempotency_key":idempotency_key,
        "status":"sent","event_id":result["event"]["id"],"delivered_at":utc_now(),
        "transport":"group_bridge_session"
    });
    store_delivery(home, receipt.clone())?;
    object(json!({"ok":true,"receipt":receipt,"event":result["event"],"deduped":false}))
}

fn post_delivery(
    endpoint: &str,
    credential: &str,
    operation: &str,
    idempotency_key: &str,
    payload: &Map<String, Value>,
) -> Result<Value, OpError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(OpError::invalid)?;
    let url = format!(
        "{}/api/group-bridge/session/send",
        endpoint.trim_end_matches('/')
    );
    let body = if operation == "remote_send" {
        let mut body = payload.clone();
        body.insert("op".into(), json!(operation));
        body.insert(
            "message_contract_version".into(),
            json!(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION),
        );
        Value::Object(body)
    } else {
        json!({
            "op":operation,
            "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
            "source_group_id":payload.get("source_group_id").cloned().unwrap_or(Value::Null),
            "src_group_id":payload.get("source_group_id").cloned().unwrap_or(Value::Null),
            "idempotency_key":idempotency_key,
            "payload":payload,
        })
    };
    let response = client
        .post(url)
        .bearer_auth(credential)
        .json(&body)
        .send()
        .map_err(|error| OpError::new("remote_transport_error", error.to_string()))?;
    let status = response.status();
    let value = response
        .json::<Value>()
        .map_err(|error| OpError::new("remote_transport_error", error.to_string()))?;
    if status.is_success() && value.get("error").is_none() && value["result"]["isError"] != true {
        return Ok(value);
    }
    Err(OpError::new("remote_delivery_failed", value.to_string()))
}

fn delivery_succeeded(value: &Value) -> bool {
    value.get("error").is_none()
        && value.get("detail").is_none()
        && value.get("ok").and_then(Value::as_bool) != Some(false)
        && value["result"]["isError"] != true
}
