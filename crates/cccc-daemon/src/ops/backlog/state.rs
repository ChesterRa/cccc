use super::*;
use cccc_core::{backlog::Document, context::ContextStore};

pub(super) fn scoped(
    home: &HomeLayout,
    request: &DaemonRequest,
    writing: bool,
) -> Result<Option<GroupDoc>, OpError> {
    let id = required_arg(request, "group_id")?;
    let mut group = super::super::context::load_group(home, &id)?;
    permissions::require_group_member(&group, &by(request))
        .map_err(|e| OpError::new("permission_denied", e.to_string()))?;
    let key = match string_arg(request, "scope_key").filter(|key| !key.is_empty()) {
        Some(key) => key,
        None if writing => return Err(OpError::invalid("scope_key is required")),
        None => match super::super::actor_runtime::caller_scope(&group, &by(request))? {
            Some(scope) => {
                if !group.scopes.iter().any(|item| {
                    item.scope_key == scope.scope_key
                        && std::path::Path::new(&item.url).canonicalize().ok().as_ref()
                            == Some(&scope.path)
                }) {
                    return Err(OpError::new(
                        "scope_conflict",
                        "The caller's running workspace differs from the attached scope",
                    ));
                }
                scope.scope_key
            }
            None => return Ok(None),
        },
    };
    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == key)
        .ok_or_else(|| OpError::new("scope_conflict", "Scope is no longer attached"))?;
    if (writing || request.args.contains_key("scope_url"))
        && string_arg(request, "scope_url").as_deref() != Some(scope.url.as_str())
    {
        return Err(OpError::new(
            "scope_conflict",
            "Workspace changed; reload before writing",
        ));
    }
    group.active_scope_key = key;
    Ok(Some(group))
}

pub(super) fn empty(key: &str, url: &str) -> Value {
    json!({"configured":false,"scope_key":key,"scope_url":url,"path":"","exists":false,"sha256":"","git":null,"problems":[],"sections":[]})
}

pub(super) fn read(group: &GroupDoc, path: &str) -> Result<workspace::FileContent, OpError> {
    let file = workspace::read_file(group, path).map_err(file_error)?;
    if file.binary || file.truncated {
        return Err(OpError::new(
            "backlog_invalid_file",
            "Backlog must be UTF-8 text no larger than 1 MiB",
        ));
    }
    Ok(file)
}

pub(super) fn snapshot(home: &HomeLayout, group: &GroupDoc) -> OpResult {
    let url = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .map(|scope| scope.url.as_str())
        .unwrap_or_default();
    let mut result = empty(&group.active_scope_key, url);
    result["git"] = git::facts(group, setting(group).unwrap_or_default()).unwrap_or(Value::Null);
    let Some(path) = setting(group) else {
        return object(result);
    };
    result["configured"] = json!(true);
    result["path"] = json!(path);
    let file = match read(group, path) {
        Ok(file) => file,
        Err(error) if error.code == "backlog_not_found" => return object(result),
        Err(error) => return Err(error),
    };
    let doc = Document::parse(&file.content);
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let context = contexts.load(&group.group_id).map_err(OpError::io)?;
    let mut sections = serde_json::to_value(&doc.sections).map_err(OpError::invalid)?;
    for section in sections.as_array_mut().into_iter().flatten() {
        for entry in section["entries"].as_array_mut().into_iter().flatten() {
            let tasks = context.tasks.iter().filter(|task| {
                entry["id"].as_str().is_some() && task.get("source").is_some_and(|source| source["scope_key"] == group.active_scope_key && source["path"] == path && source["entry_id"] == entry["id"])
            }).map(|task| json!({"id":task.get("id"),"title":task.get("title"),"status":task.get("status"),"assignee":task.get("assignee")})).collect::<Vec<_>>();
            entry["tasks"] = json!(tasks);
        }
    }
    result["exists"] = json!(true);
    result["sha256"] = json!(file.sha256);
    result["problems"] = serde_json::to_value(doc.problems).map_err(OpError::invalid)?;
    result["sections"] = sections;
    object(result)
}

pub(super) fn required_group(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<GroupDoc, OpError> {
    scoped(home, request, true)?
        .ok_or_else(|| OpError::new("group_has_no_scope", "Attach a project folder first"))
}
