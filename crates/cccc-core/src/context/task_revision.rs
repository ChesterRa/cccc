use serde_json::{Map, Value};
use std::{fmt, io};

#[derive(Debug)]
pub struct TaskConflict {
    pub task_id: String,
    pub expected: u64,
    pub actual: u64,
}
impl fmt::Display for TaskConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "task {} revision conflict: expected {}, actual {}",
            self.task_id, self.expected, self.actual
        )
    }
}
impl std::error::Error for TaskConflict {}

pub(super) fn check(tasks: &[Map<String, Value>], op: &Map<String, Value>) -> io::Result<()> {
    if op.contains_key("rev") {
        return Err(io::Error::other("rev is daemon-owned"));
    }
    let Some(expected) = op.get("if_task_rev") else {
        return Ok(());
    };
    if op.get("op").and_then(Value::as_str) == Some("task.create") {
        return Err(io::Error::other("task.create has no revision to compare"));
    }
    let expected = expected
        .as_u64()
        .ok_or_else(|| io::Error::other("if_task_rev must be an unsigned integer"))?;
    let id = op
        .get("task_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| io::Error::other("task_id is required"))?;
    let task = tasks
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| io::Error::other(format!("task not found: {id}")))?;
    let actual = task.get("rev").and_then(Value::as_u64).unwrap_or(0);
    if expected != actual {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            TaskConflict {
                task_id: id.into(),
                expected,
                actual,
            },
        ));
    }
    Ok(())
}

// Timestamps and the revision are derived metadata, not editable task content.
// An identical patch is a no-op even if the clock advanced during application.
pub(super) fn advance(
    before: &[Map<String, Value>],
    after: &mut [Map<String, Value>],
) -> io::Result<()> {
    let previous = before
        .iter()
        .filter_map(|task| task.get("id").and_then(Value::as_str).map(|id| (id, task)))
        .collect::<std::collections::BTreeMap<_, _>>();
    for task in after {
        let old = task
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| previous.get(id).copied());
        let unchanged = old.is_some_and(|old| content(old).eq(content(task)));
        let rev = old
            .and_then(|old| old.get("rev"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let next = if unchanged {
            rev
        } else {
            rev.checked_add(1)
                .ok_or_else(|| io::Error::other("task revision exhausted"))?
        };
        if unchanged {
            if let Some(value) = old.and_then(|old| old.get("updated_at")) {
                task.insert("updated_at".into(), value.clone());
            } else {
                task.remove("updated_at");
            }
        }
        task.insert("rev".into(), Value::from(next));
    }
    Ok(())
}

fn content(task: &Map<String, Value>) -> impl Iterator<Item = (&String, &Value)> {
    task.iter()
        .filter(|(key, _)| !matches!(key.as_str(), "rev" | "updated_at"))
}
