use super::*;

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
    object(json!({"terminal_writable": writable}))
}

pub(super) fn write(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let data = string_arg(request, "data")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "data is required"))?;
    cccc_runtime::write(&group_id, &actor_id, data.as_bytes()).map_err(runtime_error)?;
    super::super::runtime_hook_input::observe(home, &group_id, &actor_id, data.as_bytes());
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
