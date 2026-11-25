use serde_json::{Map, Value};

/// The outline lists open entries and a count for each Closed section.
/// A specific entry can be read in full. Writes return what changed and
/// the file's new hash.
pub(crate) fn apply(
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) -> Result<(), String> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("get");
    if action != "get" {
        result.remove("sections");
        return Ok(());
    }
    match arguments.get("entry").filter(|value| !value.is_null()) {
        None => {
            project_outline(result);
            Ok(())
        }
        Some(selector) => keep_one_entry(result, selector),
    }
}

fn project_outline(result: &mut Map<String, Value>) {
    for section in result
        .get_mut("sections")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
    {
        if section.get("closed").and_then(Value::as_bool) == Some(true) {
            let entry_count = section
                .remove("entries")
                .and_then(|entries| entries.as_array().map(Vec::len))
                .unwrap_or_default();
            section.insert("entry_count".into(), Value::from(entry_count));
            continue;
        }
        for entry in section
            .get_mut("entries")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object_mut)
        {
            entry.remove("body");
        }
    }
}

/// The same selector rule as the daemon's writes, so an entry read here can be written with
/// the same arguments: an ID, or the heading line of an entry that has none yet.
fn keep_one_entry(result: &mut Map<String, Value>, selector: &Value) -> Result<(), String> {
    let fields = selector.as_object().filter(|fields| fields.len() == 1);
    let id = fields
        .and_then(|fields| fields.get("id"))
        .and_then(Value::as_str)
        .filter(|id| cccc_contracts::backlog::valid_entry_id(id));
    let line = fields
        .and_then(|fields| fields.get("line"))
        .and_then(Value::as_u64)
        .filter(|line| *line > 0);
    let described = match (id, line) {
        (Some(id), None) => format!("id {id}"),
        (None, Some(line)) => format!("line {line}"),
        _ => {
            return Err(
                "invalid_args: entry must be {\"id\": 12 lowercase hex} or {\"line\": heading line of an entry without an id}"
                    .into(),
            );
        }
    };
    let mut found = Vec::new();
    for section in result
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = section.get("name").cloned().unwrap_or(Value::Null);
        for entry in section
            .get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let matches = match (id, line) {
                (Some(id), _) => entry.get("id").and_then(Value::as_str) == Some(id),
                (_, Some(line)) => {
                    entry.get("id").is_none_or(Value::is_null)
                        && entry.get("line").and_then(Value::as_u64) == Some(line)
                }
                _ => false,
            };
            if matches {
                let mut entry = entry.as_object().cloned().unwrap_or_default();
                entry.insert("section".into(), name.clone());
                found.push(entry);
            }
        }
    }
    let path = result.get("path").and_then(Value::as_str).unwrap_or("");
    match found.len() {
        0 => Err(format!(
            "backlog_not_found: no entry with {described} in {path}; read the outline again"
        )),
        1 => {
            result.remove("sections");
            result.insert("entry".into(), Value::Object(found.remove(0)));
            Ok(())
        }
        _ => Err(format!(
            "backlog_ambiguous: {} entries have {described} in {path}; fix the file first",
            found.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::apply;
    use serde_json::{Map, Value, json};

    fn snapshot() -> Map<String, Value> {
        json!({
            "configured": true,
            "scope_key": "s_1",
            "scope_url": "file:///repo",
            "path": "BACKLOG.md",
            "exists": true,
            "sha256": "a".repeat(64),
            "problems": [],
            "sections": [
                {"name": "", "line": 0, "closed": false, "entries": [
                    {"id": "111111111111", "title": "Before the sections", "line": 1,
                     "body": "A preamble entry.\n", "problems": [], "tasks": []}
                ]},
                {"name": "Open", "line": 3, "closed": false, "entries": [
                    {"id": "0123456789ab", "title": "Faster startup", "line": 5,
                     "body": "Measure first.\n", "problems": [], "tasks": []},
                    {"id": null, "title": "Old idea", "line": 9,
                     "body": "No id yet.\n", "problems": [], "tasks": []}
                ]},
                {"name": "Closed", "line": 12, "closed": true, "entries": [
                    {"id": "ba9876543210", "title": "Done topic", "line": 14,
                     "body": "Outcome (2026-11-11): shipped.\n", "problems": [], "tasks": []}
                ]}
            ]
        })
        .as_object()
        .cloned()
        .expect("object")
    }

    fn arguments(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object")
    }

    #[test]
    fn the_outline_keeps_open_entries_and_counts_closed_entries() {
        let mut result = snapshot();
        let problems =
            json!([{"code": "missing_id", "line": 14, "message": "Entry has no stable ID"}]);
        result["sections"][2]["entries"][0]["id"] = Value::Null;
        result.insert("problems".into(), problems.clone());
        apply(&mut result, &arguments(json!({}))).expect("outline");
        let sections = result["sections"].as_array().expect("sections");
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0]["name"], "");
        let titles: Vec<&str> = sections[..2]
            .iter()
            .flat_map(|section| section["entries"].as_array().expect("entries"))
            .map(|entry| {
                assert!(entry.get("body").is_none(), "{entry}");
                entry["title"].as_str().expect("title")
            })
            .collect();
        assert_eq!(
            titles,
            ["Before the sections", "Faster startup", "Old idea"]
        );
        assert_eq!(
            sections[2],
            json!({"name": "Closed", "line": 12, "closed": true, "entry_count": 1})
        );
        assert_eq!(result["problems"], problems);
        assert_eq!(result["sha256"], "a".repeat(64));
    }

    #[test]
    fn the_outline_counts_each_closed_section_including_an_empty_one() {
        let mut result = snapshot();
        result["sections"]
            .as_array_mut()
            .expect("sections")
            .push(json!({"name": "Closed", "line": 17, "closed": true, "entries": []}));
        apply(&mut result, &arguments(json!({}))).expect("outline");
        assert_eq!(
            result["sections"][2],
            json!({"name": "Closed", "line": 12, "closed": true, "entry_count": 1})
        );
        assert_eq!(
            result["sections"][3],
            json!({"name": "Closed", "line": 17, "closed": true, "entry_count": 0})
        );
    }

    #[test]
    fn one_entry_is_read_in_full_by_id_or_by_heading_line() {
        let mut by_id = snapshot();
        apply(
            &mut by_id,
            &arguments(json!({"action": "get", "entry": {"id": "ba9876543210"}})),
        )
        .expect("by id");
        assert!(by_id.get("sections").is_none());
        assert_eq!(by_id["entry"]["section"], "Closed");
        assert_eq!(by_id["entry"]["body"], "Outcome (2026-11-11): shipped.\n");
        assert_eq!(by_id["path"], "BACKLOG.md");

        let mut by_line = snapshot();
        apply(&mut by_line, &arguments(json!({"entry": {"line": 9}}))).expect("by line");
        assert_eq!(by_line["entry"]["title"], "Old idea");
        assert_eq!(by_line["entry"]["section"], "Open");

        let mut closed_by_line = snapshot();
        closed_by_line["sections"][2]["entries"][0]["id"] = Value::Null;
        apply(
            &mut closed_by_line,
            &arguments(json!({"entry": {"line": 14}})),
        )
        .expect("closed entry by line");
        assert_eq!(closed_by_line["entry"]["section"], "Closed");
        assert_eq!(
            closed_by_line["entry"]["body"],
            "Outcome (2026-11-11): shipped.\n"
        );
    }

    #[test]
    fn a_missing_duplicated_or_malformed_selector_is_an_error() {
        let mut result = snapshot();
        let error = apply(
            &mut result,
            &arguments(json!({"entry": {"id": "ffffffffffff"}})),
        )
        .expect_err("missing");
        assert!(error.starts_with("backlog_not_found:"), "{error}");
        assert!(
            result.contains_key("sections"),
            "a failed read keeps the snapshot"
        );

        let mut duplicated = snapshot();
        duplicated["sections"][1]["entries"][1]["id"] = json!("0123456789ab");
        let error = apply(
            &mut duplicated,
            &arguments(json!({"entry": {"id": "0123456789ab"}})),
        )
        .expect_err("duplicated");
        assert!(error.starts_with("backlog_ambiguous:"), "{error}");

        // An entry with an ID is read by it, not by its heading line, as writes require.
        let mut by_line = snapshot();
        let error = apply(&mut by_line, &arguments(json!({"entry": {"line": 5}})))
            .expect_err("identified entry by line");
        assert!(error.starts_with("backlog_not_found:"), "{error}");

        for selector in [
            json!({}),
            json!({"id": "0123456789ab", "line": 5}),
            json!({"id": "0123456789AB"}),
            json!({"id": "0123456789ab", "extra": 1}),
            json!({"line": 0}),
            json!("x"),
        ] {
            let mut result = snapshot();
            let error =
                apply(&mut result, &arguments(json!({"entry": selector}))).expect_err("malformed");
            assert!(error.starts_with("invalid_args:"), "{error}");
        }
    }

    #[test]
    fn a_write_answers_with_what_changed_not_the_whole_file() {
        let mut result = snapshot();
        result.insert("entry_id".into(), json!("0123456789ab"));
        result.insert("task_id".into(), json!("T9"));
        result.insert("reused".into(), json!(false));
        apply(
            &mut result,
            &arguments(json!({"action": "promote", "entry": {"id": "0123456789ab"}})),
        )
        .expect("write");
        assert!(result.get("sections").is_none());
        assert_eq!(result["entry_id"], "0123456789ab");
        assert_eq!(result["task_id"], "T9");
        assert_eq!(result["reused"], false);
        assert_eq!(result["sha256"], "a".repeat(64));
    }
}
