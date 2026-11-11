use super::*;
use cccc_core::{backlog::Document, context::ContextStore};

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = state::required_group(home, request)?;
    let path = selected_path(&group, request)?;
    let file = state::read(&group, &path)?;
    let expected = mutations::expected(request, false, &file.sha256)?;
    let identified = Document::parse(&file.content)
        .identify(&mutations::selector(request)?)
        .map_err(mutations::entry_error)?;
    let source =
        json!({"scope_key":group.active_scope_key,"path":path,"entry_id":identified.entry_id});
    let mut operation = request
        .args
        .get("task")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| OpError::invalid("task must contain task.create fields"))?;
    if operation.contains_key("op") || operation.contains_key("source") {
        return Err(OpError::invalid(
            "task op and source are supplied by backlog_promote",
        ));
    }
    operation.insert("op".into(), json!("task.create"));
    operation.insert("source".into(), source.clone());
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let before = contexts.load(&group.group_id).map_err(OpError::io)?;
    let replay = operation
        .get("client_request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .and_then(|id| {
            before.tasks.iter().find(|task| {
                task.get("created_by").and_then(Value::as_str) == Some(by(request).as_str())
                    && task.get("client_request_id").and_then(Value::as_str) == Some(id)
            })
        });
    if replay.is_some_and(|task| task.get("source") != Some(&source)) {
        return Err(OpError::invalid(
            "client_request_id already belongs to a different task source",
        ));
    }
    let mut context_request = request.clone();
    context_request.op = "context_sync".into();
    context_request
        .args
        .insert("ops".into(), json!([operation]));
    context_request.args.insert("dry_run".into(), json!(true));
    // Reuse Context's validation and caller authorization before assigning an ID.
    super::super::context::sync(home, &context_request)?;
    let sha = if identified.content != file.content {
        let sha = mutations::persist(&group, &path, &identified.content, &expected)?;
        event(
            home,
            &group,
            request,
            "backlog.identify",
            &path,
            &sha,
            Some(&identified.entry_id),
        )
        .map_err(|e| partial(e, file_applied(&path, &sha, Some(&identified.entry_id))))?;
        sha
    } else {
        file.sha256
    };
    let applied = file_applied(&path, &sha, Some(&identified.entry_id));
    let existing = replay.or_else(|| {
        before.tasks.iter().find(|task| {
            task.get("source") == Some(&source)
                && matches!(
                    task.get("status").and_then(Value::as_str),
                    Some("planned" | "active")
                )
        })
    });
    let (task_id, reused) = if let Some(task) = existing {
        (
            task.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            true,
        )
    } else {
        context_request.args.insert("dry_run".into(), json!(false));
        let result = super::super::context::sync(home, &context_request)
            .map_err(|error| partial(error, applied.clone()))?;
        let id = result
            .get("changes")
            .and_then(Value::as_array)
            .and_then(|changes| changes.first())
            .and_then(|change| change["task_id"].as_str())
            .ok_or_else(|| {
                partial(
                    OpError::new("io_error", "Task creation result unconfirmed"),
                    applied.clone(),
                )
            })?;
        (id.to_owned(), false)
    };
    let mut result = state::snapshot(home, &group).map_err(|e| partial(e, applied))?;
    result.insert("entry_id".into(), json!(identified.entry_id));
    result.insert("task_id".into(), json!(task_id));
    result.insert("reused".into(), json!(reused));
    Ok(result)
}
