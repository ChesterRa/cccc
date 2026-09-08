use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use super::yaml_storage::ContextPaths;
use crate::fs::{read_yaml, write_yaml};

pub(super) fn load_tasks(tasks_dir: &Path) -> io::Result<Vec<Map<String, Value>>> {
    let entries = match fs::read_dir(tasks_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    paths.retain(|path| is_task_path(path));
    paths.sort();
    paths
        .iter()
        .map(|path| {
            // An enumerated task must exist and remain readable. Treating it as
            // empty could recycle its ID and overwrite the user's original file.
            let task = required_yaml_map(path)?;
            let id = task.get("id").and_then(Value::as_str);
            if id != path.file_stem().and_then(|stem| stem.to_str()) {
                return Err(invalid_data(path, "task id must match its filename"));
            }
            Ok(task)
        })
        .collect()
}

pub(super) fn load_agents(path: &Path) -> io::Result<BTreeMap<String, Map<String, Value>>> {
    let source = read_yaml_map(path)?;
    let Some(states) = source.get("agent_states") else {
        return Ok(BTreeMap::new());
    };
    let states = states
        .as_array()
        .ok_or_else(|| invalid_data(path, "agent_states must be an array"))?;
    let mut result = BTreeMap::new();
    for value in states {
        let mut state = value
            .as_object()
            .cloned()
            .ok_or_else(|| invalid_data(path, "agent state must be an object"))?;
        let actor_id = state
            .remove("actor_id")
            .or_else(|| state.remove("id"))
            .and_then(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
            })
            .ok_or_else(|| invalid_data(path, "agent state requires an actor id"))?;
        if result.insert(actor_id, state).is_some() {
            return Err(invalid_data(path, "duplicate agent state id"));
        }
    }
    Ok(result)
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

pub(super) fn has_tasks(path: &Path) -> io::Result<bool> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        if is_task_path(&entry?.path()) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn is_task_id(id: &str) -> bool {
    id.strip_prefix('T').is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

pub(super) fn read_yaml_map(path: &Path) -> io::Result<Map<String, Value>> {
    match required_yaml_map(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        result => result,
    }
}

fn required_yaml_map(path: &Path) -> io::Result<Map<String, Value>> {
    let value = read_yaml::<Value>(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot read context file {}: {error}", path.display()),
        )
    })?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| invalid_data(path, "expected an object"))
}

pub(super) fn invalid_data(path: &Path, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid context file {}: {detail}", path.display()),
    )
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
