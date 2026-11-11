use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// Completion remains true when a done task is archived, not when unfinished
/// work is put aside. Reopening clears the retained archive state.
pub fn task_is_completed(task: &Map<String, Value>) -> bool {
    task.get("status").and_then(Value::as_str) == Some("done")
        || (task.get("status").and_then(Value::as_str) == Some("archived")
            && task.get("archived_from").and_then(Value::as_str) == Some("done"))
}

/// Add the current unresolved dependency subset to read-only task snapshots.
/// Call before filtering or paging, and never persist these projected objects.
pub fn project_task_dependencies(tasks: &mut [Map<String, Value>]) {
    let completed = tasks
        .iter()
        .filter(|task| task_is_completed(task))
        .filter_map(|task| task.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for task in tasks {
        if task
            .get("blocked_by")
            .and_then(Value::as_array)
            .is_none_or(|ids| ids.is_empty())
        {
            task.remove("open_blocked_by");
            continue;
        }
        let open = task
            .get("blocked_by")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|id| !id.as_str().is_some_and(|id| completed.contains(id)))
            .cloned()
            .collect();
        task.insert("open_blocked_by".into(), Value::Array(open));
    }
}

/// Unresolved dependencies or an explicit external/Actor wait need attention.
/// An unprojected task has no evidence that its dependencies are complete.
pub fn task_is_blocked(task: &Map<String, Value>) -> bool {
    task.get("open_blocked_by")
        .or_else(|| task.get("blocked_by"))
        .and_then(Value::as_array)
        .is_some_and(|ids| !ids.is_empty())
        || matches!(
            task.get("waiting_on").and_then(Value::as_str),
            Some("actor" | "external")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_completed_dependencies_stop_blocking_and_reopen_is_observed() {
        let mut tasks = [
            json!({"id":"T1","status":"active","rev":8,"blocked_by":["T2","T3","T4","T5","T6","missing"],"open_blocked_by":[]}),
            json!({"id":"T2","status":"done"}),
            json!({"id":"T3","status":"archived","archived_from":"active"}),
            json!({"id":"T4","status":"planned"}),
            json!({"id":"T5","status":"archived","archived_from":"done"}),
            json!({"id":"T6","status":"archived"}),
        ].map(|task| task.as_object().expect("task").clone());
        let before = tasks[0].clone();
        project_task_dependencies(&mut tasks);
        assert!(!tasks[1].contains_key("open_blocked_by"));
        assert_eq!(
            tasks[0]["open_blocked_by"],
            json!(["T3", "T4", "T6", "missing"])
        );
        assert_eq!(tasks[0]["blocked_by"], before["blocked_by"]);
        assert_eq!(tasks[0]["rev"], 8);
        assert!(task_is_blocked(&tasks[0]));
        tasks[1].insert("status".into(), json!("active"));
        project_task_dependencies(&mut tasks);
        assert_eq!(
            tasks[0]["open_blocked_by"],
            json!(["T2", "T3", "T4", "T6", "missing"])
        );
    }

    #[test]
    fn completed_dependencies_preserve_explicit_waits() {
        let mut tasks = [
            json!({"id":"T1","blocked_by":["T2"]}),
            json!({"id":"T2","status":"done"}),
        ]
        .map(|task| task.as_object().expect("task").clone());
        project_task_dependencies(&mut tasks);
        assert_eq!(tasks[0]["open_blocked_by"], json!([]));
        assert!(!task_is_blocked(&tasks[0]));
        for waiting in ["actor", "external"] {
            tasks[0].insert("waiting_on".into(), json!(waiting));
            assert!(task_is_blocked(&tasks[0]));
        }
    }
}
