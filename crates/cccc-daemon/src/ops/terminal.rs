use cccc_contracts::{ActorRole, DaemonRequest};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::terminal_text;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "terminal_status" => status(request),
        "terminal_tail" => tail(home, request),
        "terminal_snapshot" => snapshot(home, request),
        "terminal_replay" => replay(home, request),
        "terminal_history" => history(home, request),
        "terminal_since" => since(home, request),
        "terminal_write" => write(home, request),
        "terminal_resize" => resize(request),
        "terminal_clear" => clear(home, request),
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
    authorize_transcript(home, request, &group_id, &actor_id)?;
    let max_chars = integer(request, "max_chars", 8_000).clamp(1, 2_000_000);
    let page = super::terminal_history_source::retained_full(home, &group_id, &actor_id)
        .map_err(runtime_error)?;
    let (strip_ansi, compact) = tail_render_options(request);
    let text = render_tail(&page.data, max_chars, strip_ansi, compact);
    object(json!({
        "group_id": group_id,
        "actor_id": actor_id,
        "warning": "Terminal transcript may include sensitive stdout/stderr.",
        "text": text,
        "hint": "",
        "end_cursor": page.end_cursor,
    }))
}

fn snapshot(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    authorize_transcript(home, request, &group_id, &actor_id)?;
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

fn replay(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    authorize_transcript(home, request, &group_id, &actor_id)?;
    let after = request
        .args
        .get("after")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let end_cursor = request.args.get("end_cursor").and_then(Value::as_u64);
    let limit = integer(request, "limit_bytes", 512 * 1024).clamp(1, 2_000_000);
    let (page, replay_end_cursor) =
        cccc_runtime::active_history_replay(&group_id, &actor_id, after, end_cursor, limit)
            .map_err(runtime_error)?;
    object(json!({"history": page, "replay_end_cursor": replay_end_cursor}))
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
    authorize_transcript(home, request, &group_id, &actor_id)?;
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
        "group_id": group_id,
        "actor_id": actor_id,
        "warning": "Terminal transcript may include sensitive stdout/stderr.",
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
    authorize_transcript(home, request, &group_id, &actor_id)?;
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

fn clear(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    authorize_transcript(home, request, &group_id, &actor_id)?;
    cccc_runtime::clear(&group_id, &actor_id).map_err(runtime_error)?;
    object(json!({"group_id": group_id, "actor_id": actor_id, "cleared": true}))
}

fn authorize_transcript(
    home: &HomeLayout,
    request: &DaemonRequest,
    group_id: &str,
    target_actor_id: &str,
) -> Result<(), OpError> {
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if by.is_empty() || by == "user" {
        return Ok(());
    }
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(group_id)
        .map_err(OpError::not_found)?;
    if cccc_core::actors::find(&group, target_actor_id).is_none() {
        return Err(OpError::new(
            "actor_not_found",
            format!("actor not found: {target_actor_id}"),
        ));
    }
    if by == target_actor_id {
        return Ok(());
    }
    let visibility = transcript_visibility(&group);
    if cccc_core::actors::find(&group, &by).is_some()
        && (visibility == "all"
            || (visibility == "foreman"
                && cccc_core::actors::effective_role(&group, &by) == Some(ActorRole::Foreman)))
    {
        return Ok(());
    }
    let mut error = OpError::new(
        "permission_denied",
        "terminal transcript is restricted by group settings",
    );
    error.details.insert("visibility".into(), json!(visibility));
    error.details.insert("by".into(), json!(by));
    error
        .details
        .insert("target_actor_id".into(), json!(target_actor_id));
    Err(error)
}

fn transcript_visibility(group: &GroupDoc) -> &str {
    let configured = group
        .extra
        .get("settings")
        .and_then(Value::as_object)
        .and_then(|settings| {
            settings.get("terminal_transcript_visibility").or_else(|| {
                settings
                    .get("terminal_transcript")
                    .and_then(Value::as_object)
                    .and_then(|value| value.get("visibility"))
            })
        })
        .or_else(|| {
            group
                .extra
                .get("terminal_transcript")
                .and_then(Value::as_object)
                .and_then(|value| value.get("visibility"))
        })
        .and_then(Value::as_str);
    match configured {
        Some(value @ ("off" | "foreman" | "all")) => value,
        _ => "foreman",
    }
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
