use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::terminal_text;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "terminal_status" => status(request),
        "terminal_tail" => tail(home, request),
        "terminal_snapshot" => snapshot(home, request),
        "terminal_history" => history(home, request),
        "terminal_since" => since(home, request),
        "terminal_write" => write(home, request),
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

fn tail(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let max_chars = integer(request, "max_chars", 8_000).clamp(1, 2_000_000);
    let page = super::terminal_history_source::retained(
        home,
        &group_id,
        &actor_id,
        max_chars.max(512 * 1024),
    )
    .map_err(runtime_error)?;
    let (strip_ansi, compact) = tail_render_options(request);
    let text = render_tail(&page.data, max_chars, strip_ansi, compact);
    object(json!({"text": text, "hint": "", "end_cursor": page.end_cursor}))
}

fn snapshot(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let limit = integer(request, "limit_bytes", 512 * 1024).clamp(1, 2_000_000);
    let page = super::terminal_history_source::retained(home, &group_id, &actor_id, limit)
        .map_err(runtime_error)?;
    let rendered = terminal_text::render(&page.data, false);
    let data = if rendered.is_empty() {
        String::new()
    } else {
        format!("\u{1b}[2J\u{1b}[H{rendered}")
    };
    object(json!({
        "data": data,
        "start_cursor": page.start_cursor,
        "end_cursor": page.end_cursor,
    }))
}

fn render_tail(text: &str, max_chars: usize, strip_ansi: bool, compact: bool) -> String {
    let rendered = if strip_ansi {
        terminal_text::render(text, compact)
    } else {
        text.to_owned()
    };
    trailing_chars(&rendered, max_chars)
}

fn trailing_chars(text: &str, max_chars: usize) -> String {
    let start = text
        .char_indices()
        .rev()
        .nth(max_chars.saturating_sub(1))
        .map(|(index, _)| index)
        .unwrap_or(0);
    text[start..].to_owned()
}

fn history(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let before = request.args.get("before").and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    });
    let limit = integer(request, "limit_bytes", 64_000).clamp(1, 2_000_000);
    let page = super::terminal_history_source::page(home, &group_id, &actor_id, before, limit)
        .map_err(runtime_error)?;
    let strip_ansi = bool_arg(request, "strip_ansi", false);
    let text = if strip_ansi {
        terminal_text::render(&page.data, bool_arg(request, "compact", false))
    } else {
        page.data.clone()
    };
    object(json!({
        "text": text,
        "hint": "",
        "start_cursor": page.start_cursor,
        "end_cursor": page.end_cursor,
        "has_more": page.has_more,
        "cursor_expired": page.cursor_expired,
        "history": page,
    }))
}

fn since(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let after = request
        .args
        .get("after")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    let limit = integer(request, "limit_bytes", 64_000).clamp(1, 2_000_000);
    let page = super::terminal_history_source::since(home, &group_id, &actor_id, after, limit)
        .map_err(runtime_error)?;
    object(json!({"history": page}))
}

fn write(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let data = string_arg(request, "data")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "data is required"))?;
    cccc_runtime::write(&group_id, &actor_id, data.as_bytes()).map_err(runtime_error)?;
    super::runtime_hook_input::observe(home, &group_id, &actor_id, data.as_bytes());
    object(json!({"written": data.len()}))
}

#[cfg(test)]
fn is_interrupt_input(data: &str) -> bool {
    data.as_bytes().contains(&0x03) || data == "\u{1b}"
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

fn tail_render_options(request: &DaemonRequest) -> (bool, bool) {
    (
        bool_arg(request, "strip_ansi", true),
        bool_arg(request, "compact", true),
    )
}

fn runtime_error(error: cccc_runtime::RuntimeError) -> OpError {
    OpError::new("runtime_error", error.to_string())
}

#[cfg(all(test, unix))]
#[path = "terminal_io_tests.rs"]
mod io_tests;

#[cfg(all(test, unix))]
#[path = "terminal_hook_tests.rs"]
mod hook_tests;
