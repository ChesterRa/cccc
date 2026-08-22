use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult};

use super::payload::normalize_outbound_payload;
use super::state::{bridge_state, items, nonempty};

const SESSION_TRANSPORT: &str = "group_bridge_session";

pub(in crate::ops) struct PreparedReply {
    registration_id: String,
    remote_event_id: String,
    remote_group_id: String,
    remote_to: Vec<String>,
    remote_message_mode: String,
    local_to: Vec<String>,
    payload: Map<String, Value>,
}

pub(super) fn prepare(
    home: &HomeLayout,
    group: &GroupDoc,
    target: &Event,
    request: &DaemonRequest,
    message_mode: &str,
) -> Result<Option<PreparedReply>, OpError> {
    let inbound_bridge_sender = target.by.starts_with("group_bridge:");
    if !inbound_bridge_sender
        || target.data.get("source_platform").and_then(Value::as_str) != Some(SESSION_TRANSPORT)
    {
        return Ok(None);
    }

    let remote_group_id = target
        .data
        .get("src_group_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| route_not_found(group, target, "remote source group is missing"))?
        .to_owned();
    let remote_peer_id = target
        .data
        .get("source_user_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| route_not_found(group, target, "remote source peer is missing"))?;
    let state = bridge_state(home)?;
    let trust = items(&state, "trusts")
        .iter()
        .find(|item| {
            item["status"] == "active"
                && item["transport"] == SESSION_TRANSPORT
                && item["group_id"] == group.group_id
                && item["remote_group_id"] == remote_group_id
                && item["remote_peer_id"] == remote_peer_id
        })
        .ok_or_else(|| route_not_found(group, target, "no active Group Bridge route found"))?;
    let registration_id = nonempty(trust, &["registration_id", "trust_id"])
        .ok_or_else(|| route_not_found(group, target, "remote route has no registration"))?;
    if !matches!(
        trust["remote_access_level"].as_str().unwrap_or("messages"),
        "messages" | "read" | "full"
    ) {
        return Err(OpError::new(
            "permission_denied",
            "remote trust does not allow messages",
        ));
    }

    let explicit_to = recipients(request.args.get("to"));
    let remote_to = if explicit_to.is_empty() {
        let remote_to = default_remote_recipients(target);
        if remote_to.is_empty() {
            return Err(OpError::new(
                "missing_remote_recipient",
                "Group Bridge replies require an explicit recipient when the remote sender did not provide a return recipient",
            ));
        }
        remote_to
    } else {
        explicit_to
    };

    let mut payload = Map::new();
    for key in ["text", "format", "refs", "attachments"] {
        if let Some(value) = request.args.get(key).cloned() {
            payload.insert(key.into(), value);
        }
    }
    payload.insert("message_mode".into(), json!(message_mode));
    payload.insert("to".into(), json!(remote_to));
    normalize_outbound_payload(request, &mut payload)?;

    Ok(Some(PreparedReply {
        registration_id,
        remote_event_id: target
            .data
            .get("src_event_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .to_owned(),
        remote_group_id,
        remote_to,
        remote_message_mode: message_mode.into(),
        local_to: vec!["user".into()],
        payload,
    }))
}

impl PreparedReply {
    pub(in crate::ops) fn apply_local_metadata(
        &self,
        target: &Event,
        args: &mut Map<String, Value>,
    ) {
        args.insert("to".into(), json!(self.local_to));
        args.insert("message_mode".into(), json!("send"));
        args.insert("dst_group_id".into(), json!(self.remote_group_id));
        args.insert("dst_to".into(), json!(self.remote_to));
        args.insert("dst_message_mode".into(), json!(self.remote_message_mode));
        for key in [
            "source_platform",
            "source_user_name",
            "source_user_id",
            "mention_user_ids",
        ] {
            if let Some(value) = target.data.get(key).cloned() {
                args.insert(key.into(), value);
            }
        }
    }

    pub(in crate::ops) fn relay(
        self,
        home: &HomeLayout,
        request: &DaemonRequest,
        source_event_id: &str,
    ) -> OpResult {
        let mut args = Map::from_iter([
            (
                "group_id".into(),
                request.args.get("group_id").cloned().unwrap_or(Value::Null),
            ),
            (
                "by".into(),
                request
                    .args
                    .get("by")
                    .cloned()
                    .unwrap_or_else(|| json!("user")),
            ),
            ("registration_id".into(), json!(self.registration_id)),
            (
                "idempotency_key".into(),
                json!(format!("reply:{source_event_id}:{}", self.registration_id)),
            ),
            ("source_event_id".into(), json!(source_event_id)),
            (
                "reply_to_remote_event_id".into(),
                json!(self.remote_event_id),
            ),
            ("payload".into(), Value::Object(self.payload)),
        ]);
        for key in ["insight", "require_peer_insight"] {
            if let Some(value) = request.args.get(key).cloned() {
                args.insert(key.into(), value);
            }
        }
        super::remote_send_without_source_record(
            home,
            &DaemonRequest {
                v: 1,
                op: "remote_send".into(),
                args,
            },
        )
    }
}

fn default_remote_recipients(target: &Event) -> Vec<String> {
    let stored = recipients(target.data.get("remote_reply_to"));
    if !stored.is_empty() {
        return stored;
    }
    let sender = target
        .data
        .get("src_by")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if matches!(sender, "user" | "@user") {
        return vec!["user".into()];
    }
    if sender.is_empty() || sender.starts_with(['@', '#']) || sender.starts_with("group_bridge:") {
        return Vec::new();
    }
    vec![sender.into()]
}

fn recipients(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn route_not_found(group: &GroupDoc, target: &Event, message: &str) -> OpError {
    let mut error = OpError::new("group_bridge_reply_route_not_found", message);
    error
        .details
        .insert("group_id".into(), json!(group.group_id));
    for key in ["src_group_id", "source_user_id", "source_platform"] {
        if let Some(value) = target.data.get(key).cloned() {
            error.details.insert(key.into(), value);
        }
    }
    error
}
