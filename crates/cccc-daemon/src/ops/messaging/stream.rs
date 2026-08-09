use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

pub(super) fn emit(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = super::load(home, request)?;
    let by = required_arg(request, "by")?;
    let operation = required_arg(request, "op")?;
    if !matches!(operation.as_str(), "start" | "update" | "end") {
        return Err(OpError::new(
            "invalid_op",
            "op must be 'start', 'update', or 'end'",
        ));
    }
    let stream_id = if operation == "start" {
        Uuid::new_v4().simple().to_string()
    } else {
        required_arg(request, "stream_id").map_err(|_| {
            OpError::new("missing_stream_id", "stream_id is required for update/end")
        })?
    };
    let to = request
        .args
        .get("to")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut data = json!({
        "stream_id":stream_id,
        "op":operation,
        "mode":string_arg(request, "mode").unwrap_or_else(|| "snapshot".into()),
        "text":string_arg(request, "text").unwrap_or_default(),
        "format":string_arg(request, "format").unwrap_or_else(|| "plain".into()),
        "seq":request.args.get("seq").and_then(Value::as_u64).unwrap_or(0),
        "to":to,
        "reply_to":string_arg(request, "reply_to"),
        "client_id":string_arg(request, "client_id"),
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    super::super::message_metadata::add_sender_title_snapshot(&group, &by, &mut data);
    let event = super::append(home, &group.group_id, "chat.stream", &by, data)?;
    object(json!({"event":event,"stream_id":stream_id}))
}
