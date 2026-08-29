use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use super::yaml_storage::ContextPaths;
use crate::fs::{read_yaml, write_yaml};

pub(super) fn load_tasks(tasks_dir: &Path) -> io::Result<Vec<Map<String, Value>>> {
    if !tasks_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(tasks_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_task_path(path))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths
        .iter()
        .filter_map(|path| {
            read_yaml::<Value>(path)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .filter(|task| {
                    task.get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| !id.is_empty())
                })
        })
        .collect())
}

pub(super) fn load_agents(path: &Path) -> BTreeMap<String, Map<String, Value>> {
    let source = read_yaml_map(path);
    source
        .get("agent_states")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let mut state = value.as_object()?.clone();
            let actor_id = state
                .remove("actor_id")
                .or_else(|| state.remove("id"))?
                .as_str()?
                .trim()
                .to_owned();
            (!actor_id.is_empty()).then_some((actor_id, state))
        })
        .collect()
}

pub(super) fn write_agents(
    paths: &ContextPaths,
    states: &BTreeMap<String, Map<String, Value>>,
) -> io::Result<()> {
    let agents = states
        .iter()
        .map(|(actor_id, state)| {
            let mut state = state.clone();
            state.remove("id");
            state.insert("actor_id".into(), Value::String(actor_id.clone()));
            Value::Object(state)
        })
        .collect::<Vec<_>>();
    write_yaml(&paths.agents_file, &json!({"agent_states":agents}))
}

pub(super) fn write_task_diff(
    paths: &ContextPaths,
    before: &[Map<String, Value>],
    after: &[Map<String, Value>],
) -> io::Result<()> {
    let before = task_map(before);
    let after = task_map(after);
    for (id, task) in &after {
        if before.get(id) != Some(task) {
            write_task(paths, task)?;
        }
    }
    for id in before.keys().filter(|id| !after.contains_key(*id)) {
        let path = paths.tasks_dir.join(format!("{id}.yaml"));
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn has_tasks(path: &Path) -> bool {
    fs::read_dir(path).ok().is_some_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|entry| is_task_path(&entry.path()))
    })
}

pub(super) fn is_task_id(id: &str) -> bool {
    id.strip_prefix('T').is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

pub(super) fn read_yaml_map(path: &Path) -> Map<String, Value> {
    read_yaml::<Value>(path)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn write_task(paths: &ContextPaths, task: &Map<String, Value>) -> io::Result<()> {
    let id = task
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_task_id(id))
        .ok_or_else(|| io::Error::other("task id must match T<number> format"))?;
    write_yaml(&paths.tasks_dir.join(format!("{id}.yaml")), task)
}

fn task_map(tasks: &[Map<String, Value>]) -> BTreeMap<String, Map<String, Value>> {
    tasks
        .iter()
        .filter_map(|task| {
            task.get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_owned(), task.clone()))
        })
        .collect()
}

fn is_task_path(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("yaml")
        && path
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(is_task_id)
}
