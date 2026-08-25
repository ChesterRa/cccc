use serde_json::{Map, Value, json};

use super::task_list_query::{matches_task, sort_tasks, task_number, task_status};

pub(super) struct PageFilter<'a> {
    pub query: &'a str,
    pub assignee: &'a str,
    pub attention: Option<&'a str>,
}

pub(super) fn page(
    tasks: &[Map<String, Value>],
    status: Option<&str>,
    offset: usize,
    limit: usize,
    filter: &PageFilter<'_>,
) -> Value {
    let mut matching = tasks
        .iter()
        .filter(|task| {
            matches_task(
                task,
                status,
                filter.attention,
                filter.query,
                filter.assignee,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    sort_tasks(&mut matching, status);
    let total_count = matching.len();
    let items = matching
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let count = items.len();
    json!({
        "tasks":items,
        "count":count,
        "total_count":total_count,
        "offset":offset,
        "limit":limit,
        "has_more":offset.saturating_add(count) < total_count,
    })
}

pub(super) fn pages(
    tasks: &[Map<String, Value>],
    statuses: &[String],
    offset: usize,
    limit: usize,
    filter: &PageFilter<'_>,
) -> Value {
    Value::Object(
        statuses
            .iter()
            .map(|status| {
                (
                    status.clone(),
                    page(tasks, Some(status), offset, limit, filter),
                )
            })
            .collect(),
    )
}

pub(super) fn index(tasks: &[Map<String, Value>]) -> Value {
    const FIELDS: [&str; 5] = ["id", "title", "status", "assignee", "parent_id"];
    let mut items = tasks
        .iter()
        .filter(|task| task_status(task) != "archived")
        .map(|task| {
            let mut item = Map::new();
            for field in FIELDS {
                if let Some(value) = task.get(field) {
                    item.insert(field.into(), value.clone());
                }
            }
            item
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|task| std::cmp::Reverse(task_number(task)));
    Value::Array(items.into_iter().map(Value::Object).collect())
}
