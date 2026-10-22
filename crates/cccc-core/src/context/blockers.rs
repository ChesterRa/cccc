//! Which of a task's `blocked_by` entries still hold it back.

use serde_json::{Map, Value};
use std::collections::HashMap;

/// Each task's status by id, taken from one task list, to tell open blockers
/// from resolved ones.
pub struct Blockers {
    status: HashMap<String, String>,
}

impl Blockers {
    pub fn new<'a>(tasks: impl IntoIterator<Item = &'a Map<String, Value>>) -> Self {
        let status = tasks
            .into_iter()
            .filter_map(|task| {
                let id = task.get("id")?.as_str()?;
                Some((id.to_owned(), status(task).to_owned()))
            })
            .collect();
        Self { status }
    }

    /// The entries of `task`'s `blocked_by` that are not done or archived. An
    /// entry that names no task stays open: nothing can complete it.
    pub fn open(&self, task: &Map<String, Value>) -> Vec<String> {
        task.get("blocked_by")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| match entry.as_str() {
                Some(id) => (!matches!(
                    self.status.get(id).map(String::as_str),
                    Some("done" | "archived")
                ))
                .then(|| id.to_owned()),
                None => Some(entry.to_string()),
            })
            .collect()
    }

    /// Tasks in `after` that `before` held back by an open blocker and that
    /// have none left, at least one of the earlier blockers having entered
    /// done. Each comes with those blockers. Each list is read on its own: a
    /// task's `blocked_by` as it stood then, against the statuses of that same
    /// list. A sync that also edits a dependency list is therefore judged by
    /// the edges and statuses before it and after it, never a mix of the two.
    /// A task that did not exist before was not held back, and done or
    /// archived tasks are left out.
    pub fn unblocked<'t>(
        before: &[Map<String, Value>],
        after: &'t [Map<String, Value>],
    ) -> Vec<(&'t Map<String, Value>, Vec<String>)> {
        let earlier = Self::new(before);
        let later = Self::new(after);
        let rows = before
            .iter()
            .filter_map(|task| Some((task.get("id")?.as_str()?, task)))
            .collect::<HashMap<_, _>>();
        after
            .iter()
            .filter(|task| !matches!(status(task), "done" | "archived"))
            .filter_map(|task| {
                let prior = rows.get(task.get("id")?.as_str()?)?;
                let held = earlier.open(prior);
                if held.is_empty() || !later.open(task).is_empty() {
                    return None;
                }
                let done = held
                    .into_iter()
                    .filter(|id| later.status.get(id).map(String::as_str) == Some("done"))
                    .collect::<Vec<_>>();
                (!done.is_empty()).then_some((task, done))
            })
            .collect()
    }
}

fn status(task: &Map<String, Value>) -> &str {
    task.get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|status| !status.is_empty())
        .unwrap_or("planned")
}
