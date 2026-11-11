use serde_json::{Map, Value};

/// An agent reads the backlog in two steps: the outline of every section and
/// entry, then one entry in full. A write answers with what it changed and the
/// file's new hash, not with the whole file again.
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
            for entry in entries_mut(result) {
                entry.remove("body");
            }
            Ok(())
        }
        Some(selector) => keep_one_entry(result, selector),
    }
}

fn entries_mut(result: &mut Map<String, Value>) -> impl Iterator<Item = &mut Map<String, Value>> {
    result
        .get_mut("sections")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(|section| section.get_mut("entries").and_then(Value::as_array_mut))
        .flatten()
        .filter_map(Value::as_object_mut)
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
                {"name": "", "line": 1, "closed": false, "entries": []},
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
    fn the_outline_keeps_every_entry_without_its_body() {
        let mut result = snapshot();
        apply(&mut result, &arguments(json!({}))).expect("outline");
        let sections = result["sections"].as_array().expect("sections");
        assert_eq!(sections.len(), 3);
        let titles: Vec<&str> = sections
            .iter()
            .flat_map(|section| section["entries"].as_array().expect("entries"))
            .map(|entry| {
                assert!(entry.get("body").is_none(), "{entry}");
                entry["title"].as_str().expect("title")
            })
            .collect();
        assert_eq!(titles, ["Faster startup", "Old idea", "Done topic"]);
        assert_eq!(result["sha256"], "a".repeat(64));
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
