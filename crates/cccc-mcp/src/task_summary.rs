//! Task lists as agents read them: one short row per task, open work first.
//!
//! Notes, outcome and checklist text stay behind a `task_id` read, so a list grows with the
//! number of tasks shown, not with their history. Rows are summaries and must not be written
//! back as a task.

use serde_json::{Map, Value, json};

/// Characters kept from a title; longer titles end in `...`.
const TITLE_CHARS: usize = 120;
/// Blocking task ids kept on a row; a longer list adds `blocked_by_total`.
const BLOCKED_BY_ITEMS: usize = 10;
const STATUSES: [&str; 4] = ["planned", "active", "done", "archived"];
const DEFAULT_LIMIT: u64 = 50;
const MAX_LIMIT: u64 = 100;
const NOTE: &str = "Rows are summaries; read the full task by task_id before updating its fields.";

/// One row: what the task is, its state, who has it and how far its checklist got.
pub(crate) fn summary_row(task: &Map<String, Value>) -> Value {
    let mut row = Map::new();
    for key in ["id", "status"] {
        if let Some(value) = text(task, key) {
            row.insert(key.into(), Value::String(value));
        }
    }
    if let Some(title) = text(task, "title") {
        row.insert("title".into(), Value::String(trim(&title, TITLE_CHARS)));
    }
    for key in [
        "assignee",
        "priority",
        "task_type",
        "parent_id",
        "handoff_to",
    ] {
        if let Some(value) = text(task, key) {
            row.insert(key.into(), Value::String(value));
        }
    }
    if let Some(waiting_on) = text(task, "waiting_on").filter(|value| value != "none") {
        row.insert("waiting_on".into(), Value::String(waiting_on));
    }
    let blocked_by = task
        .get("blocked_by")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::trim).filter(|id| !id.is_empty()))
        .collect::<Vec<_>>();
    if !blocked_by.is_empty() {
        row.insert(
            "blocked_by".into(),
            json!(blocked_by.iter().take(BLOCKED_BY_ITEMS).collect::<Vec<_>>()),
        );
        if blocked_by.len() > BLOCKED_BY_ITEMS {
            row.insert("blocked_by_total".into(), json!(blocked_by.len()));
        }
    }
    if let Some(items) = task
        .get("checklist")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
    {
        let done = items
            .iter()
            .filter(|item| item.get("status").and_then(Value::as_str) == Some("done"))
            .count();
        row.insert(
            "checklist".into(),
            Value::String(format!("{done}/{}", items.len())),
        );
    }
    for key in ["rev", "updated_at"] {
        if let Some(value) = task.get(key).filter(|value| !value.is_null()) {
            row.insert(key.into(), value.clone());
        }
    }
    Value::Object(row)
}

/// Replace the daemon's full task list with one page of summary rows.
///
/// Without `status`, the page shows planned and active tasks; with `query`, also done ones, so
/// finished work is found before it is redone, and `include_archived` adds archived tasks.
/// An explicit `status` shows that status only.
/// `counts` covers every status of the tasks that matched the daemon's filters.
pub(crate) fn list_page(
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) -> Result<(), String> {
    let tasks = match result.remove("tasks") {
        Some(Value::Array(tasks)) => tasks,
        Some(_) => return Err("task_list returned tasks that are not a list".into()),
        None => return Ok(()),
    };
    let status = arguments
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|status| !status.is_empty());
    if let Some(status) = status
        && !STATUSES.contains(&status)
    {
        return Err(format!("invalid status: {status}"));
    }
    let searching = arguments
        .get("query")
        .and_then(Value::as_str)
        .is_some_and(|query| !query.trim().is_empty());
    let include_archived = arguments
        .get("include_archived")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut shown: Vec<&str> = match status {
        Some(status) => vec![status],
        None if searching => vec!["planned", "active", "done"],
        None => vec!["planned", "active"],
    };
    if include_archived && status.is_none() {
        shown.push("archived");
    }
    let limit = bounded(arguments, "limit", DEFAULT_LIMIT, 1, MAX_LIMIT)?;
    let offset = bounded(arguments, "offset", 0, 0, u64::MAX)?;

    let mut counts = Map::from_iter(STATUSES.iter().map(|name| ((*name).into(), json!(0))));
    let mut matching = Vec::new();
    for task in tasks.iter().filter_map(Value::as_object) {
        let status = text(task, "status").unwrap_or_else(|| "planned".into());
        if let Some(count) = counts.get_mut(&status) {
            *count = json!(count.as_u64().unwrap_or(0) + 1);
        }
        if shown.contains(&status.as_str()) {
            matching.push(task);
        }
    }
    let total = matching.len();
    let skip = usize::try_from(offset).unwrap_or(usize::MAX);
    let rows = matching
        .into_iter()
        .skip(skip)
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(summary_row)
        .collect::<Vec<_>>();
    let has_more = skip.saturating_add(rows.len()) < total;
    result.insert("tasks".into(), Value::Array(rows));
    result.insert("statuses".into(), json!(shown));
    result.insert("counts".into(), Value::Object(counts));
    result.insert("total".into(), json!(total));
    result.insert("offset".into(), json!(offset));
    result.insert("limit".into(), json!(limit));
    result.insert("has_more".into(), json!(has_more));
    result.insert("note".into(), json!(NOTE));
    Ok(())
}

fn bounded(
    arguments: &Map<String, Value>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|number| (min..=max).contains(number))
            .ok_or_else(|| {
                if max == u64::MAX {
                    format!("{key} must be an integer of at least {min}")
                } else {
                    format!("{key} must be between {min} and {max}")
                }
            }),
    }
}

fn text(task: &Map<String, Value>, key: &str) -> Option<String> {
    task.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn trim(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    format!(
        "{}...",
        value
            .chars()
            .take(max_chars - 3)
            .collect::<String>()
            .trim_end()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object")
    }

    fn board() -> Map<String, Value> {
        object(json!({"tasks": [
            {"id": "T5", "title": "Ship the release notes", "status": "active", "assignee": "lead",
             "priority": "high", "task_type": "standard", "waiting_on": "none", "rev": 4,
             "updated_at": "2026-11-03T01:00:00Z", "notes": "long history", "outcome": "",
             "checklist": [{"text": "draft", "status": "done"}, {"text": "review", "status": "pending"}]},
            {"id": "T4", "title": "Fix the checkout rounding", "status": "planned", "waiting_on": "user",
             "blocked_by": ["T2", " "], "rev": 1},
            {"id": "T3", "title": "Coupon threshold", "status": "done", "outcome": "merged", "rev": 7},
            {"id": "T2", "title": "Old spike", "status": "archived", "rev": 2},
            {"id": "T1", "title": "First task", "status": "done", "rev": 3}
        ]}))
    }

    #[test]
    fn default_list_shows_open_tasks_as_rows_and_counts_the_rest() {
        let mut result = board();
        list_page(&mut result, &Map::new()).expect("page");
        let ids = result["tasks"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["id"].as_str().expect("id"))
            .collect::<Vec<_>>();
        assert_eq!(ids, ["T5", "T4"]);
        assert_eq!(result["statuses"], json!(["planned", "active"]));
        assert_eq!(
            result["counts"],
            json!({"planned": 1, "active": 1, "done": 2, "archived": 1})
        );
        assert_eq!(result["total"], 2);
        assert_eq!(result["has_more"], false);
        let row = &result["tasks"][0];
        assert_eq!(row["checklist"], "1/2");
        assert_eq!(row["rev"], 4);
        assert!(row.get("notes").is_none() && row.get("outcome").is_none());
        assert!(row.get("waiting_on").is_none(), "none is not shown");
        assert_eq!(result["tasks"][1]["waiting_on"], "user");
        assert_eq!(result["tasks"][1]["blocked_by"], json!(["T2"]));
        assert!(result["note"].as_str().expect("note").contains("task_id"));
    }

    #[test]
    fn status_query_and_archived_choose_what_is_shown() {
        let mut done = board();
        list_page(&mut done, &object(json!({"status": "done"}))).expect("done");
        assert_eq!(done["total"], 2);
        assert_eq!(done["tasks"][0]["id"], "T3");

        let mut search = board();
        list_page(&mut search, &object(json!({"query": "coupon"}))).expect("search");
        assert_eq!(search["statuses"], json!(["planned", "active", "done"]));
        assert_eq!(search["total"], 4);

        let mut archived = board();
        list_page(&mut archived, &object(json!({"include_archived": true}))).expect("archived");
        assert_eq!(
            archived["statuses"],
            json!(["planned", "active", "archived"])
        );
        assert_eq!(archived["total"], 3);

        let mut explicit = board();
        list_page(&mut explicit, &object(json!({"status": "archived"}))).expect("explicit");
        assert_eq!(explicit["tasks"][0]["id"], "T2");

        // An explicit status wins over include_archived.
        let mut done_only = board();
        list_page(
            &mut done_only,
            &object(json!({"status": "done", "include_archived": true})),
        )
        .expect("done only");
        assert_eq!(done_only["statuses"], json!(["done"]));
        assert_eq!(done_only["total"], 2);

        let mut invalid = board();
        assert_eq!(
            list_page(&mut invalid, &object(json!({"status": "closed"}))),
            Err("invalid status: closed".into())
        );
    }

    #[test]
    fn pages_after_filtering_and_reports_what_is_left() {
        let mut first = board();
        list_page(&mut first, &object(json!({"status": "done", "limit": 1}))).expect("first page");
        assert_eq!(first["tasks"].as_array().expect("rows").len(), 1);
        assert_eq!(first["has_more"], true);
        let mut second = board();
        list_page(
            &mut second,
            &object(json!({"status": "done", "limit": 1, "offset": 1})),
        )
        .expect("second page");
        assert_eq!(second["tasks"][0]["id"], "T1");
        assert_eq!(second["has_more"], false);
        let mut past_end = board();
        list_page(&mut past_end, &object(json!({"offset": 9}))).expect("past end");
        assert_eq!(past_end["tasks"], json!([]));
        assert_eq!(past_end["total"], 2);
        assert_eq!(past_end["has_more"], false);
        for bad in [
            json!({"limit": 0}),
            json!({"limit": 101}),
            json!({"offset": -1}),
            json!({"limit": "5"}),
        ] {
            assert!(list_page(&mut board(), &object(bad)).is_err());
        }
    }

    #[test]
    fn long_blocker_lists_say_how_many_there_are() {
        let ids = (1..=12).map(|n| format!("T{n}")).collect::<Vec<_>>();
        let row = summary_row(&object(json!({"id": "T20", "blocked_by": ids})));
        assert_eq!(row["blocked_by"].as_array().expect("ids").len(), 10);
        assert_eq!(row["blocked_by_total"], 12);
        let short = summary_row(&object(json!({"id": "T21", "blocked_by": ["T1"]})));
        assert!(short.get("blocked_by_total").is_none());
    }

    #[test]
    fn long_titles_end_with_a_marker() {
        let long = "长".repeat(130);
        let row = summary_row(&object(
            json!({"id": "T9", "title": long, "status": "active"}),
        ));
        let title = row["title"].as_str().expect("title");
        assert_eq!(title.chars().count(), 120);
        assert!(title.ends_with("..."));
    }
}
