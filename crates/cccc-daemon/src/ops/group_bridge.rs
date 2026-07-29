use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use std::time::Duration;

use crate::dispatch::{OpError, OpResult, object, required_arg};

mod payload;
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

const STORE_KEY: &str = "group_bridge";

pub(super) fn route_ready(home: &HomeLayout, trust: &Value) -> bool {
    session_runtime::route_ready(home, trust)
}

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "remote_send" => remote_send(home, request),
        "remote_delivery_status" => delivery_status(home, request),
        "group_bridge_receive_remote_send" => receive(home, request),
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
    let existing = find_delivery(&state, &registration_id, &idempotency_key);
    if existing.as_ref().is_some_and(|receipt| {
        matches!(
            receipt["status"].as_str().unwrap_or(""),
            "delivered" | "sent" | "failed"
        )
    }) {
        return object(json!({
            "queued":false,"receipt":existing,"deduped":true
        }));
    }
    let attempt = existing
        .as_ref()
        .and_then(|receipt| receipt["attempt"].as_u64())
        .unwrap_or(0)
        + 1;
    let endpoint = nonempty(&route, &["remote_endpoint", "endpoint", "url"]).ok_or_else(|| {
        OpError::new(
            "registration_invalid",
            "registration has no remote endpoint",
        )
    })?;
    let credential = nonempty(&route, &["credential", "token"]).ok_or_else(|| {
        OpError::new(
            "credential_unresolved",
            "registration credential is unavailable",
        )
    })?;
    let remote_group_id =
        nonempty(&route, &["remote_group_id", "target_group_id"]).unwrap_or_default();
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
    let mut body = if let Some(stored_body) = stored_body {
        stored_body
    } else {
        if let Some(insight) = request.args.get("insight").cloned() {
            record_payload.insert("insight".into(), insight);
        }
        if let Some(required) = request.args.get("require_peer_insight").cloned() {
            record_payload.insert("require_peer_insight".into(), required);
        }
        let mut body = payload;
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
        if let Some(value) = request.args.get("source_event_id").cloned() {
            body.insert("src_event_id".into(), value);
        }
        if let Some(value) = request.args.get("reply_to_remote_event_id").cloned() {
            body.insert("reply_to".into(), value);
        }
        body
    };
    if !body.contains_key("source_by") {
        body.insert("source_by".into(), json!("user"));
    }
    store_delivery(
        home,
        json!({
            "ok":false,"status":"sending","registration_id":registration_id,
            "idempotency_key":idempotency_key,"remote_group_id":remote_group_id,
            "transport":"group_bridge_session","attempt":attempt,"max_attempts":5,
            "payload":body,"source_record_payload":record_payload,
            "last_attempt_at":utc_now(),"error":null
        }),
    )?;
    let session_request = json!({
        "op":"remote_send",
        "src_group_id":group_id,
        "target_group_id":remote_group_id,
        "remote_peer_id":route["remote_peer_id"],
        "idempotency_key":idempotency_key,
        "payload":body.clone()
    });
    let session_remote = crate::group_bridge_sessions::send(
        &group_id,
        &remote_group_id,
        route["remote_peer_id"].as_str().unwrap_or(""),
        session_request,
    )
    .filter(delivery_succeeded);
    let remote_result = session_remote
        .map(Ok)
        .unwrap_or_else(|| post_delivery(&endpoint, &credential, &idempotency_key, &body));
    let remote = match remote_result {
        Ok(remote) => remote,
        Err(error) => {
            let terminal = attempt >= 5;
            let receipt = json!({
                "ok":false,"status":if terminal{"failed"}else{"retrying"},
                "registration_id":registration_id,"idempotency_key":idempotency_key,
                "remote_group_id":remote_group_id,"transport":"group_bridge_session",
                "attempt":attempt,"max_attempts":5,"payload":body,
                "source_record_payload":record_payload,
                "updated_at":utc_now(),
                "error":{"code":error.code,"message":error.message,
                    "retriable":!terminal,"transport":"group_bridge_session"}
            });
            store_delivery(home, receipt.clone())?;
            return object(json!({
                "queued":!terminal,"receipt":receipt,"deduped":false
            }));
        }
    };
    let mut receipt = remote
        .pointer("/result/receipt")
        .or_else(|| remote.get("receipt"))
        .cloned()
        .unwrap_or_else(|| json!({"status":"delivered"}));
    receipt["registration_id"] = json!(registration_id);
    receipt["idempotency_key"] = json!(idempotency_key);
    receipt["remote_group_id"] = json!(remote_group_id);
    receipt["transport"] = json!("group_bridge_session");
    receipt["ok"] = json!(true);
    receipt["attempt"] = json!(attempt);
    receipt["max_attempts"] = json!(5);
    receipt["updated_at"] = json!(utc_now());
    store_delivery(home, receipt.clone())?;
    let mut record = record_payload;
    record.insert("group_id".into(), json!(group_id));
    record.insert("dst_group_id".into(), json!(remote_group_id));
    record.insert("by".into(), body["source_by"].clone());
    record.insert("source_by".into(), body["source_by"].clone());
    record.insert("src_group_id".into(), json!(group_id));
    if let Some(value) = request.args.get("source_event_id").cloned() {
        record.insert("src_event_id".into(), value);
    }
    record.insert("delivery_receipt".into(), receipt.clone());
    let local = dispatch_message(home, "send_cross_group_remote_record", record)?;
    object(json!({
        "queued":false,"receipt":receipt,"source_event":local.get("source_event"),
        "transport":"group_bridge_session","deduped":false
    }))
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
    let result = dispatch_message(home, "send", args)?;
    let receipt = json!({
        "registration_id":registration_id,"idempotency_key":idempotency_key,
        "status":"delivered","event_id":result["event"]["id"],"delivered_at":utc_now(),
        "transport":"group_bridge_session"
    });
    store_delivery(home, receipt.clone())?;
    object(json!({"ok":true,"receipt":receipt,"event":result["event"],"deduped":false}))
}

fn post_delivery(
    endpoint: &str,
    credential: &str,
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
    let response = client
        .post(url)
        .bearer_auth(credential)
        .json(&Value::Object(payload.clone()))
        .send()
        .map_err(|error| OpError::new("remote_transport_error", error.to_string()))?;
    let status = response.status();
    let value = response
        .json::<Value>()
        .map_err(|error| OpError::new("remote_transport_error", error.to_string()))?;
    if status.is_success() && value.get("error").is_none() && value["result"]["isError"] != true {
        return Ok(value);
    }
    let mut arguments = payload.clone();
    for key in [
        "source_group_id",
        "src_group_id",
        "source_group_title",
        "source_by",
        "idempotency_key",
        "dst_group_id",
        "group_id",
        "by",
    ] {
        arguments.remove(key);
    }
    arguments.insert("client_id".into(), json!(idempotency_key));
    let fallback = client
        .post(format!(
            "{}/mcp/group-bridge",
            endpoint.trim_end_matches('/')
        ))
        .bearer_auth(credential)
        .json(&json!({
            "jsonrpc":"2.0","id":idempotency_key,"method":"tools/call",
            "params":{"name":"cccc_message_send","arguments":arguments}
        }))
        .send()
        .map_err(|error| OpError::new("remote_transport_error", error.to_string()))?;
    let fallback_status = fallback.status();
    let fallback_value = fallback
        .json::<Value>()
        .map_err(|error| OpError::new("remote_transport_error", error.to_string()))?;
    if fallback_status.is_success()
        && fallback_value.get("error").is_none()
        && fallback_value["result"]["isError"] != true
    {
        Ok(json!({"receipt":{
            "status":"delivered","idempotency_key":idempotency_key,
            "transport":"group_bridge_mcp"
        }}))
    } else {
        Err(OpError::new(
            "remote_delivery_failed",
            format!("session={value}; mcp={fallback_value}"),
        ))
    }
}

fn delivery_succeeded(value: &Value) -> bool {
    value.get("error").is_none()
        && value.get("detail").is_none()
        && value.get("ok").and_then(Value::as_bool) != Some(false)
        && value["result"]["isError"] != true
}
