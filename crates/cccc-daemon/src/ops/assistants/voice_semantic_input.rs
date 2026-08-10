use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::{GroupStore, HomeLayout, integration_state};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io;
use uuid::Uuid;

use super::{voice_input, voice_input_delivery};
use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

const KEY: &str = "assistants";

pub(super) fn append(
    home: &HomeLayout,
    request: &DaemonRequest,
    kind: &str,
    text: String,
) -> OpResult {
    append_with_state(home, request, kind, text, |_| Ok(()))
}

pub(super) fn append_with_state(
    home: &HomeLayout,
    request: &DaemonRequest,
    kind: &str,
    text: String,
    prepare_state: impl FnOnce(&mut serde_json::Map<String, Value>) -> io::Result<()>,
) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(OpError::new("empty_voice_input", "text cannot be empty"));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let state = integration_state::group_get(&store, &group_id, KEY).map_err(OpError::io)?;
    let assistant = state
        .get("assistant")
        .cloned()
        .or_else(|| state.get("voice_secretary").cloned())
        .unwrap_or_else(voice_input::default_assistant);
    if !assistant["enabled"].as_bool().unwrap_or(false) {
        return Err(OpError::new(
            "assistant_disabled",
            "voice_secretary is disabled",
        ));
    }

    let request_id = string_arg(request, "request_id").unwrap_or_default();
    let input_append_id = requested_input_append_id(request);
    let session_id = semantic_session_id(kind);
    let segment_id = input_append_id
        .as_deref()
        .map(|key| idempotent_segment_id(kind, &request_id, key))
        .unwrap_or_else(|| format!("input-{}", Uuid::new_v4().simple()));
    let document_path = string_arg(request, "document_path")
        .map(|value| value.trim().to_owned())
        .unwrap_or_default();
    if !document_path.is_empty() {
        voice_input::validate_document_path(&document_path)?;
    }
    let language = string_arg(request, "language").unwrap_or_default();
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let trigger = request.args.get("trigger").cloned().unwrap_or_else(|| {
        json!({
            "trigger_kind":if kind == "prompt_refine" { "prompt_refine" } else { "user_instruction" },
            "capture_mode":if kind == "prompt_refine" { "prompt" } else { "instruction" },
            "source":"user"
        })
    });
    let now = utc_now();
    let input_path = voice_input::input_log_path(home, &group_id);
    let (candidate_input, input_created) = integration_state::group_update(
        &store,
        &group_id,
        KEY,
        |value| {
            let root = voice_input::state_root(value);
            prepare_state(root)?;
            if let Some(existing) =
                voice_input::find_segment_io(&input_path, &session_id, &segment_id)?
            {
                let seq = existing["seq"].as_u64().unwrap_or(0);
                let latest = root
                    .get("input_latest_seq")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                root.insert("input_latest_seq".into(), json!(latest.max(seq)));
                return Ok((Some(existing), false));
            }
            let next_seq = root
                .get("input_latest_seq")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                + 1;
            let record = json!({
                "schema":1,
                "seq":next_seq,
                "input_id":format!("vin_{}",Uuid::new_v4().simple()),
                "kind":kind,
                "text":text,
                "language":language,
                "document_path":document_path,
                "session_id":session_id,
                "segment_id":segment_id,
                "by":by,
                "trigger":trigger,
                "request_id":request_id,
                "input_append_id":input_append_id,
                "operation":string_arg(request,"operation").unwrap_or_default(),
                "composer_snapshot_hash":string_arg(request,"composer_snapshot_hash").unwrap_or_default(),
                "metadata":request.args.get("metadata").cloned().unwrap_or_else(||json!({})),
                "created_at":now
            });
            voice_input::append_jsonl_io(&input_path, &record)?;
            root.insert("input_latest_seq".into(), json!(next_seq));
            root.insert("input_updated_at".into(), json!(now));
            Ok((Some(record), true))
        },
    )
    .map_err(OpError::io)?;
    let delivery = voice_input_delivery::deliver(
        home,
        &store,
        &group_id,
        &session_id,
        &segment_id,
        &by,
        candidate_input.as_ref(),
    )?;
    let current = integration_state::group_get(&store, &group_id, KEY).map_err(OpError::io)?;
    object(json!({
        "group_id":group_id,
        "assistant":current.get("assistant").cloned().unwrap_or_else(voice_input::default_assistant),
        "session_id":session_id,
        "input_append_id":input_append_id,
        "segment":Value::Null,
        "segment_path":Value::Null,
        "document":Value::Null,
        "document_updated":false,
        "input_event":candidate_input,
        "input_event_created":input_created,
        "event":delivery.event,
        "input_notify_event":delivery.notify,
        "input_notify_emitted":delivery.notify.is_some(),
        "actor_woken":delivery.actor_woken,
        "actor_wake_error":delivery.wake_error,
        "actor_notify_delivered":delivery.delivery.as_ref().and_then(|item|item["queued"].as_u64()).unwrap_or(0)>0,
        "actor_notify_delivery":delivery.delivery
    }))
}

pub(super) fn requested_input_append_id(request: &DaemonRequest) -> Option<String> {
    string_arg(request, "input_append_id")
        .or_else(|| string_arg(request, "idempotency_key"))
        .map(|value| value.trim().chars().take(256).collect::<String>())
        .filter(|value| !value.is_empty())
}

fn idempotent_segment_id(kind: &str, request_id: &str, key: &str) -> String {
    let digest = Sha256::digest(format!("{kind}\0{request_id}\0{key}").as_bytes());
    format!("semantic-{digest:x}")
}

fn semantic_session_id(kind: &str) -> String {
    match kind {
        "prompt_refine" => "voice-secretary-prompt-refine".into(),
        "voice_instruction" => "voice-secretary-user-instruction".into(),
        _ => format!("voice-secretary-{kind}"),
    }
}
