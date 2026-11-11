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

    fn captured_tasks() -> Vec<Map<String, Value>> {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/open-blocked-by/tasks.json"
        ))
        .expect("captured tasks")
    }

    fn set_status(tasks: &mut [Map<String, Value>], index: usize, status: &str) {
        tasks[index].insert("status".into(), json!(status));
    }

    #[test]
    fn a_planned_blocker_still_blocks() {
        let tasks = captured_tasks();
        assert_eq!(open_blocked_by(&tasks, &tasks[0]), ["blocker-task"]);
    }

    #[test]
    fn done_and_archived_blockers_stop_counting() {
        let mut tasks = captured_tasks();
        let blocked = tasks[0].clone();
        for status in ["planned", "active"] {
            set_status(&mut tasks, 1, status);
            assert_eq!(
                open_blocked_by(&tasks, &blocked),
                ["blocker-task"],
                "{status}"
            );
        }
        for status in ["done", "archived"] {
            set_status(&mut tasks, 1, status);
            assert!(
                open_blocked_by(&tasks, &blocked).is_empty(),
                "{status} should not block"
            );
        }
    }

    #[test]
    fn an_id_that_names_no_task_keeps_blocking() {
        let mut tasks = captured_tasks();
        tasks[0].insert("blocked_by".into(), json!(["missing-blocker"]));
        let blocked = tasks[0].clone();
        assert_eq!(open_blocked_by(&tasks, &blocked), ["missing-blocker"]);
    }

    #[test]
    fn a_blocker_restored_to_unfinished_blocks_again() {
        let mut tasks = captured_tasks();
        let blocked = tasks[0].clone();
        set_status(&mut tasks, 1, "done");
        assert!(open_blocked_by(&tasks, &blocked).is_empty());
        set_status(&mut tasks, 1, "planned");
        assert_eq!(open_blocked_by(&tasks, &blocked), ["blocker-task"]);
        set_status(&mut tasks, 1, "done");
        assert!(open_blocked_by(&tasks, &blocked).is_empty());
    }

    #[test]
    fn no_blocked_by_means_nothing_open() {
        let mut tasks = captured_tasks();
        tasks[0].remove("blocked_by");
        let blocked = tasks[0].clone();
        assert!(open_blocked_by(&tasks, &blocked).is_empty());
    }
}
