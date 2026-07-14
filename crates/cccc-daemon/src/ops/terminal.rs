use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg};

pub fn handle(_home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "terminal_status" => status(request),
        "terminal_tail" => tail(request),
        "terminal_history" => history(request),
        "terminal_write" => write(request),
        "terminal_resize" => resize(request),
        "terminal_clear" => clear(request),
        _ => return None,
    })
}

fn ids(request: &DaemonRequest) -> Result<(String, String), OpError> {
    Ok((
        required_arg(request, "group_id")?,
        required_arg(request, "actor_id")?,
    ))
}

fn status(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let status = cccc_runtime::status(&group_id, &actor_id).map_err(runtime_error)?;
    object(json!({"session": status}))
}

fn tail(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let max_chars = integer(request, "max_chars", 8_000).clamp(1, 2_000_000);
    let page =
        cccc_runtime::history(&group_id, &actor_id, None, max_chars).map_err(runtime_error)?;
    object(json!({"text": page.data, "end_cursor": page.end_cursor}))
}

fn history(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let before = request.args.get("before").and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    });
    let limit = integer(request, "limit_bytes", 64_000).clamp(1, 2_000_000);
    let page = cccc_runtime::history(&group_id, &actor_id, before, limit).map_err(runtime_error)?;
    object(json!({"history": page}))
}

fn write(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let data = required_arg(request, "data")?;
    cccc_runtime::write(&group_id, &actor_id, data.as_bytes()).map_err(runtime_error)?;
    object(json!({"written": data.len()}))
}

fn resize(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let cols = integer(request, "cols", 120).clamp(1, u16::MAX as usize) as u16;
    let rows = integer(request, "rows", 40).clamp(1, u16::MAX as usize) as u16;
    cccc_runtime::resize(&group_id, &actor_id, cols, rows).map_err(runtime_error)?;
    object(json!({"resized": true, "cols": cols, "rows": rows}))
}

fn clear(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    cccc_runtime::clear(&group_id, &actor_id).map_err(runtime_error)?;
    object(json!({"cleared": true}))
}

fn integer(request: &DaemonRequest, name: &str, default: usize) -> usize {
    request
        .args
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}

fn runtime_error(error: cccc_runtime::RuntimeError) -> OpError {
    OpError::new("runtime_error", error.to_string())
}
