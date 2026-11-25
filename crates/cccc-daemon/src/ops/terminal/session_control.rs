use super::*;
use crate::session_owner::runtime as cccc_runtime;

pub(super) fn ready(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if request.args.get("by").and_then(Value::as_str) != Some("user") {
        return Err(OpError::new(
            "permission_denied",
            "terminal startup input can only be released explicitly by the user",
        ));
    }
    let (group, actor) = ids(request)?;
    let generation = required_arg(request, "generation")?;
    if let Err(error) = load_pty_target(home, &group, &actor) {
        return Err(if error.code == "not_pty_actor" {
            stale_startup_input()
        } else {
            error
        });
    }
    match cccc_runtime::release_startup_input(&group, &actor, &generation) {
        Ok(true) => object(json!({"released":true})),
        Ok(false)
        | Err(
            cccc_runtime::RuntimeError::NotFound(..) | cccc_runtime::RuntimeError::NotRunning(..),
        ) => Err(stale_startup_input()),
        Err(error) => Err(runtime_error(error)),
    }
}

fn stale_startup_input() -> OpError {
    OpError::new(
        "runtime_session_changed",
        "the terminal session changed; inspect its current startup prompts before allowing input",
    )
}

pub(super) fn status(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let status = cccc_runtime::status(&group_id, &actor_id).map_err(runtime_error)?;
    object(json!({"session": status}))
}

pub(super) fn attachment_status(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let attachment_id = request
        .args
        .get("attachment_id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let writable = cccc_runtime::attachment_writable(&group_id, &actor_id, attachment_id)
        .map_err(runtime_error)?;
    let mut status = json!({"terminal_writable": writable});
    // Every attachment shows the PTY at the writer's size, so viewers read it here.
    if let Ok((cols, rows)) = cccc_runtime::terminal_size(&group_id, &actor_id) {
        status["cols"] = json!(cols);
        status["rows"] = json!(rows);
    }
    object(status)
}

pub(super) fn write(_home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let data = string_arg(request, "data")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "data is required"))?;
    cccc_runtime::write(&group_id, &actor_id, data.as_bytes()).map_err(runtime_error)?;
    object(json!({"written": data.len()}))
}

pub(super) fn resize(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let cols = terminal_size_arg(request, "cols");
    let rows = terminal_size_arg(request, "rows");
    if !(10..=u16::MAX as usize).contains(&cols) || !(2..=u16::MAX as usize).contains(&rows) {
        return Err(OpError::new(
            "invalid_size",
            format!("invalid terminal size: cols={cols} rows={rows}"),
        ));
    }
    let attachment_id = match request.args.get("attachment_id") {
        None => None,
        Some(value) => Some(value.as_u64().filter(|value| *value > 0).ok_or_else(|| {
            OpError::new(
                "invalid_args",
                "attachment_id must be a positive integer when provided",
            )
        })?),
    };
    load_pty_target(home, &group_id, &actor_id)?;
    require_active_session(&group_id, &actor_id)?;
    let cols = cols as u16;
    let rows = rows as u16;
    let resized = match attachment_id {
        Some(attachment_id) => {
            cccc_runtime::resize_from_attachment(&group_id, &actor_id, attachment_id, cols, rows)
                .map_err(active_session_error)?
        }
        None => {
            cccc_runtime::resize(&group_id, &actor_id, cols, rows).map_err(active_session_error)?;
            true
        }
    };
    if !resized {
        return Err(OpError::new(
            "terminal_not_writer",
            "terminal attachment is not the current writer",
        ));
    }
    object(json!({
        "group_id": group_id,
        "actor_id": actor_id,
        "cols": cols,
        "rows": rows,
    }))
}

pub(super) fn clear(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    authorize_transcript(home, request, &group_id, &actor_id)?;
    require_active_session(&group_id, &actor_id)?;
    cccc_runtime::clear(&group_id, &actor_id).map_err(active_session_error)?;
    object(json!({"group_id": group_id, "actor_id": actor_id, "cleared": true}))
}
