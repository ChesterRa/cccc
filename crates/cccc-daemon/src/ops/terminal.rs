use cccc_contracts::{ActorRole, DaemonRequest, RunnerKind};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::terminal_text;

mod history_page;
mod session_control;

#[cfg(all(test, unix))]
use session_control::write;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "terminal_status" => session_control::status(request),
        "term_attachment_status" => session_control::attachment_status(request),
        "terminal_tail" => tail(home, request),
        "terminal_snapshot" => snapshot(home, request),
        "terminal_replay" => replay(home, request),
        "terminal_history" => history(home, request),
        "terminal_since" => since(home, request),
        "terminal_write" => session_control::write(home, request),
        "term_resize" | "terminal_resize" => session_control::resize(home, request),
        "terminal_clear" => session_control::clear(home, request),
        _ => return None,
    })
}

fn ids(request: &DaemonRequest) -> Result<(String, String), OpError> {
    Ok((
        required_arg(request, "group_id")?,
        required_arg(request, "actor_id")?,
    ))
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
    require_active_session(&group_id, &actor_id)?;
    let after = request
        .args
        .get("after")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let end_cursor = request.args.get("end_cursor").and_then(Value::as_u64);
    let limit = integer(request, "limit_bytes", 512 * 1024).clamp(1, 2_000_000);
    let (page, replay_end_cursor) =
        cccc_runtime::active_history_replay(&group_id, &actor_id, after, end_cursor, limit)
            .map_err(active_session_error)?;
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
    let render_before = request.args.get("render_before").and_then(Value::as_u64);
    let page = history_page::read(home, &group_id, &actor_id, before, limit, render_before)?;
    let strip_ansi = bool_arg(request, "strip_ansi", false);
    let text = if strip_ansi {
        terminal_text::render_history(&page.data, bool_arg(request, "compact", false))
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
        .ok_or_else(|| {
            OpError::new(
                "invalid_args",
                "after is required and must be a non-negative integer",
            )
        })?;
    let limit = integer(request, "limit_bytes", 64_000).clamp(1, 2_000_000);
    let page = super::terminal_history_source::since(home, &group_id, &actor_id, after, limit)
        .map_err(runtime_error)?;
    object(json!({"history": page}))
}

fn authorize_transcript(
    home: &HomeLayout,
    request: &DaemonRequest,
    group_id: &str,
    target_actor_id: &str,
) -> Result<(), OpError> {
    let group = load_pty_target(home, group_id, target_actor_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if by.is_empty() || by == "user" {
        return Ok(());
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

fn load_pty_target(home: &HomeLayout, group_id: &str, actor_id: &str) -> Result<GroupDoc, OpError> {
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(group_id)
        .map_err(|_| OpError::new("group_not_found", format!("group not found: {group_id}")))?;
    let actor = cccc_core::actors::find(&group, actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", format!("actor not found: {actor_id}")))?;
    if actor.runner != RunnerKind::Pty {
        let mut error = OpError::new(
            "not_pty_actor",
            "terminal operation is only available for PTY actors",
        );
        error.details.insert("runner".into(), json!(actor.runner));
        return Err(error);
    }
    Ok(group)
}

fn require_active_session(group_id: &str, actor_id: &str) -> Result<(), OpError> {
    match cccc_runtime::status(group_id, actor_id) {
        Ok(status) if status.running => Ok(()),
        Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(_, _)) => {
            Err(OpError::new("actor_not_running", "actor is not running"))
        }
        Err(error) => Err(runtime_error(error)),
    }
}

fn transcript_visibility(group: &GroupDoc) -> &str {
    let configured = group
        .extra
        .get("terminal_transcript")
        .and_then(Value::as_object)
        .and_then(|value| value.get("visibility"))
        .or_else(|| {
            group
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

fn terminal_size_arg(request: &DaemonRequest, name: &str) -> usize {
    request.args.get(name).map_or(0, |value| match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0),
        Value::String(text) => text.parse().unwrap_or(0),
        _ => 0,
    })
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

fn active_session_error(error: cccc_runtime::RuntimeError) -> OpError {
    match error {
        cccc_runtime::RuntimeError::NotFound(_, _) => {
            OpError::new("actor_not_running", "actor is not running")
        }
        error => runtime_error(error),
    }
}

#[cfg(all(test, unix))]
#[path = "terminal_io_tests.rs"]
mod io_tests;
