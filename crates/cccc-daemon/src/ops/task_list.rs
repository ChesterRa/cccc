use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, context::ContextStore};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use task_list_pages::{PageFilter, index, page, pages};
use task_list_query::{delete_info, facets, matches_task, sort_tasks, text, validated_filter};

const STATUSES: [&str; 4] = ["planned", "active", "done", "archived"];
const ATTENTION_FILTERS: [&str; 4] = ["blocked", "waiting_user", "handoff", "unassigned"];
const MAX_PAGE_SIZE: usize = 100;

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    super::context::load_group(home, &group_id)?;
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let document = contexts.load(&group_id).map_err(OpError::io)?;
    let version = contexts.tasks_version(&document);

    if let Some(task_id) = string_arg(request, "task_id").filter(|value| !value.trim().is_empty()) {
        return one(&document.tasks, &task_id, version);
    }
    if let Some(task_ids) = string_arg(request, "task_ids").filter(|value| !value.trim().is_empty())
    {
        return many(&document.tasks, &task_ids, version);
    }

    let status = validated_filter(request, "status", &STATUSES)?;
    let statuses = status_list(request)?;
    if status.is_some() && statuses.is_some() {
        return Err(OpError::new(
            "invalid_args",
            "status and statuses cannot be combined",
        ));
    }
    let attention = validated_filter(request, "attention", &ATTENTION_FILTERS)?;
    let query = string_arg(request, "query")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let assignee = string_arg(request, "assignee").unwrap_or_default();
    let pagination = pagination(request)?;
    let facet_value = facets(&document.tasks);
    let filter = PageFilter {
        query: &query,
        assignee: &assignee,
        attention: attention.as_deref(),
    };
    if let Some(statuses) = statuses {
        let (offset, limit) = pagination.unwrap_or((0, 30));
        let mut result = json!({
            "pages":pages(&document.tasks, &statuses, offset, limit, &filter),
            "tasks_version":version,
            "facets":facet_value,
        });
        if bool_arg(request, "include_index")? {
            result["task_index"] = index(&document.tasks);
        }
        return object(result);
    }

    if let Some((offset, limit)) = pagination {
        let mut result = page(&document.tasks, status.as_deref(), offset, limit, &filter);
        result["tasks_version"] = json!(version);
        result["facets"] = facet_value;
        if bool_arg(request, "include_index")? {
            result["task_index"] = index(&document.tasks);
        }
        return object(result);
    }

    let mut tasks = document
        .tasks
        .into_iter()
        .filter(|task| {
            matches_task(
                task,
                status.as_deref(),
                attention.as_deref(),
                &query,
                &assignee,
            )
        })
        .collect::<Vec<_>>();
    sort_tasks(&mut tasks, status.as_deref());
    object(json!({"tasks":tasks}))
}

fn one(tasks: &[Map<String, Value>], task_id: &str, version: String) -> OpResult {
    let mut task = tasks
        .iter()
        .find(|task| text(task, "id") == task_id)
        .cloned()
        .ok_or_else(|| OpError::new("task_not_found", format!("task not found: {task_id}")))?;
    task.insert(
        "children".into(),
        Value::Array(
            tasks
                .iter()
                .filter(|candidate| text(candidate, "parent_id") == task_id)
                .cloned()
                .map(Value::Object)
                .collect(),
        ),
    );
    object(json!({
        "task":task,
        "tasks_version":version,
        "delete_info":delete_info(tasks, task_id),
    }))
}

fn many(tasks: &[Map<String, Value>], raw_ids: &str, version: String) -> OpResult {
    let ids = comma_values(raw_ids, "task_ids", &[])?;
    if ids.len() > 100 {
        return Err(OpError::new(
            "invalid_args",
            "task_ids accepts at most 100 ids",
        ));
    }
    let selected = ids
        .iter()
        .filter_map(|id| tasks.iter().find(|task| text(task, "id") == id).cloned())
        .collect::<Vec<_>>();
    object(json!({"tasks":selected,"tasks_version":version}))
}

fn pagination(request: &DaemonRequest) -> Result<Option<(usize, usize)>, OpError> {
    let Some(limit) = request.args.get("limit") else {
        if request.args.contains_key("offset") {
            return Err(OpError::new("invalid_args", "offset requires limit"));
        }
        return Ok(None);
    };
    let limit = parse_usize(limit, "limit")?;
    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(OpError::new(
            "invalid_args",
            "limit must be between 1 and 100",
        ));
    }
    let offset = request
        .args
        .get("offset")
        .map(|value| parse_usize(value, "offset"))
        .transpose()?
        .unwrap_or(0);
    Ok(Some((offset, limit)))
}

fn parse_usize(value: &Value, name: &str) -> Result<usize, OpError> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| {
            OpError::new(
                "invalid_args",
                format!("{name} must be a non-negative integer"),
            )
        })
}

fn status_list(request: &DaemonRequest) -> Result<Option<Vec<String>>, OpError> {
    string_arg(request, "statuses")
        .filter(|value| !value.trim().is_empty())
        .map(|value| comma_values(&value, "statuses", &STATUSES))
        .transpose()
}

fn comma_values(raw: &str, name: &str, allowed: &[&str]) -> Result<Vec<String>, OpError> {
    let mut values = Vec::new();
    for value in raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !allowed.is_empty() && !allowed.contains(&value) {
            return Err(OpError::new(
                "invalid_args",
                format!("invalid {name}: {value}"),
            ));
        }
        if !values.iter().any(|existing| existing == value) {
            values.push(value.to_owned());
        }
    }
    if values.is_empty() {
        return Err(OpError::new(
            "invalid_args",
            format!("{name} must not be empty"),
        ));
    }
    Ok(values)
}

fn bool_arg(request: &DaemonRequest, name: &str) -> Result<bool, OpError> {
    let Some(value) = request.args.get(name) else {
        return Ok(false);
    };
    match value {
        Value::Bool(value) => Ok(*value),
        Value::String(value) if matches!(value.as_str(), "true" | "1") => Ok(true),
        Value::String(value) if matches!(value.as_str(), "false" | "0" | "") => Ok(false),
        _ => Err(OpError::new(
            "invalid_args",
            format!("{name} must be a boolean"),
        )),
    }
}

#[cfg(test)]
#[path = "task_list_tests.rs"]
mod tests;

#[path = "task_list_query.rs"]
mod task_list_query;

#[path = "task_list_pages.rs"]
mod task_list_pages;
