//! Which `blocked_by` entries still block, derived from live task status.

use serde_json::{Map, Value};
use std::collections::HashMap;

/// The subset of a task's `blocked_by` ids that still block. A blocker stops
/// counting once its task is `done` or `archived` — archiving drops planned
/// or active work too, and a dropped blocker can no longer be waited on —
/// while restoring that task to `planned` or `active` blocks again, because
/// the set is derived on every read and nothing is stored. An id that names
/// no task keeps blocking, so a typo or a removed task stays visible.
pub fn open_blocked_by(tasks: &[Map<String, Value>], task: &Map<String, Value>) -> Vec<String> {
    let blocked_by = task
        .get("blocked_by")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if blocked_by.is_empty() {
        return Vec::new();
    }
    let statuses: HashMap<&str, &str> = tasks
        .iter()
        .filter_map(|candidate| {
            Some((
                candidate.get("id")?.as_str()?,
                candidate.get("status")?.as_str()?,
            ))
        })
        .collect();
    blocked_by
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| {
            statuses
                .get(id)
                .is_none_or(|status| !matches!(*status, "done" | "archived"))
        })
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(id: &str, status: &str, blocked_by: &[&str]) -> Map<String, Value> {
        let mut task = Map::new();
        task.insert("id".into(), json!(id));
        task.insert("status".into(), json!(status));
        task.insert(
            "blocked_by".into(),
            json!(blocked_by.iter().map(|id| json!(id)).collect::<Vec<_>>()),
        );
        task
    }

    #[test]
    fn a_planned_blocker_still_blocks() {
        let blocker = task("blocker-a", "planned", &[]);
        let task = task("task-a", "active", &["blocker-a"]);
        assert_eq!(
            open_blocked_by(&[blocker, task.clone()], &task),
            ["blocker-a"]
        );
    }

    #[test]
    fn done_and_archived_blockers_stop_counting() {
        let done = task("blocker-a", "done", &[]);
        let archived = task("blocker-b", "archived", &[]);
        let planned = task("blocker-c", "planned", &[]);
        let task = task("task-a", "active", &["blocker-a", "blocker-b", "blocker-c"]);
        assert_eq!(
            open_blocked_by(&[done, archived, planned, task.clone()], &task),
            ["blocker-c"]
        );
    }

    #[test]
    fn an_id_that_names_no_task_keeps_blocking() {
        let task = task("task-a", "active", &["missing-blocker"]);
        assert_eq!(open_blocked_by(&[task.clone()], &task), ["missing-blocker"]);
    }

    #[test]
    fn a_blocker_restored_to_unfinished_blocks_again() {
        let mut blocker = task("blocker-a", "done", &[]);
        let task = task("task-a", "active", &["blocker-a"]);
        assert!(open_blocked_by(&[blocker.clone(), task.clone()], &task).is_empty());
        blocker.insert("status".into(), json!("planned"));
        assert_eq!(
            open_blocked_by(&[blocker.clone(), task.clone()], &task),
            ["blocker-a"]
        );
        blocker.insert("status".into(), json!("done"));
        assert!(open_blocked_by(&[blocker, task.clone()], &task).is_empty());
    }

    #[test]
    fn no_blocked_by_means_nothing_open() {
        let task = Map::new();
        assert!(open_blocked_by(&[task.clone()], &task).is_empty());
    }
}
