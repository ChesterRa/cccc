use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::model::ContextDoc;
use super::yaml_files;
use crate::fs::{read_json, write_json, write_yaml};

pub(super) use super::yaml_files::is_task_id as is_canonical_task_id;

#[derive(Debug, Clone)]
pub(super) struct ContextPaths {
    pub context_file: PathBuf,
    pub tasks_dir: PathBuf,
    pub agents_file: PathBuf,
    pub version_file: PathBuf,
    pub legacy_file: PathBuf,
    pub migration_file: PathBuf,
    pub lock_file: PathBuf,
}

impl ContextPaths {
    pub fn new(group_dir: &Path) -> Self {
        let context_dir = group_dir.join("context");
        Self {
            context_file: context_dir.join("context.yaml"),
            tasks_dir: context_dir.join("tasks"),
            agents_file: context_dir.join("agents.yaml"),
            version_file: context_dir.join("version_state.json"),
            migration_file: context_dir.join(".rust-state-migrated-v1.json"),
            lock_file: context_dir.join(".rust-context.lock"),
            legacy_file: group_dir.join("state/context.json"),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(super) struct VersionState {
    pub global_rev: u64,
    #[serde(default)]
    pub context_rev: u64,
    #[serde(default)]
    pub tasks_rev: u64,
    #[serde(default)]
    pub agents_rev: u64,
    #[serde(default)]
    pub actors_rev: u64,
}

impl VersionState {
    pub fn load(paths: &ContextPaths) -> io::Result<Self> {
        match read_json(&paths.version_file) {
            Ok(version) => Ok(version),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let has_context = paths.context_file.try_exists()?;
                let has_tasks = yaml_files::has_tasks(&paths.tasks_dir)?;
                let has_agents = paths.agents_file.try_exists()?;
                let baseline = u64::from(has_context || has_tasks || has_agents);
                Ok(Self {
                    global_rev: baseline,
                    context_rev: u64::from(has_context),
                    tasks_rev: u64::from(has_tasks),
                    agents_rev: u64::from(has_agents),
                    actors_rev: 0,
                })
            }
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!(
                    "cannot read context version {}: {error}",
                    paths.version_file.display()
                ),
            )),
        }
    }

    pub fn bump(&mut self, context: bool, tasks: bool, agents: bool) -> io::Result<()> {
        if !(context || tasks || agents) {
            return Ok(());
        }
        for (revision, changed) in [
            (&mut self.global_rev, true),
            (&mut self.context_rev, context),
            (&mut self.tasks_rev, tasks),
            (&mut self.agents_rev, agents),
        ] {
            *revision = revision.checked_add(u64::from(changed)).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "context revision exhausted")
            })?;
        }
        Ok(())
    }
}

pub(super) fn load(paths: &ContextPaths) -> io::Result<ContextDoc> {
    load_with_tasks(paths, true)
}

pub(super) fn load_overview(paths: &ContextPaths) -> io::Result<ContextDoc> {
    load_with_tasks(paths, false)
}

fn load_with_tasks(paths: &ContextPaths, include_tasks: bool) -> io::Result<ContextDoc> {
    let context = yaml_files::read_yaml_map(&paths.context_file)?;
    let object_field = |key: &str| -> io::Result<serde_json::Map<String, Value>> {
        match context.get(key) {
            None => Ok(serde_json::Map::new()),
            Some(value) => value.as_object().cloned().ok_or_else(|| {
                yaml_files::invalid_data(&paths.context_file, &format!("{key} must be an object"))
            }),
        }
    };
    let coordination = object_field("coordination")?;
    let meta = object_field("meta")?;
    let version = VersionState::load(paths)?;
    Ok(ContextDoc {
        v: 3,
        revision: version.global_rev,
        tasks_revision: version.tasks_rev,
        updated_at: String::new(),
        coordination,
        tasks: if include_tasks {
            yaml_files::load_tasks(&paths.tasks_dir)?
        } else {
            Vec::new()
        },
        agent_states: yaml_files::load_agents(&paths.agents_file)?,
        meta,
    })
}

pub(super) fn persist_diff(
    paths: &ContextPaths,
    before: &ContextDoc,
    after: &ContextDoc,
) -> io::Result<VersionState> {
    fs::create_dir_all(&paths.tasks_dir)?;
    let context_changed = before.coordination != after.coordination || before.meta != after.meta;
    let tasks_changed = before.tasks != after.tasks;
    let agents_changed = before.agent_states != after.agent_states;
    let mut version = VersionState::load(paths)?;
    // Reserve the revision before the first payload write. An I/O failure can
    // leave part of this file batch visible; no reader may accept an old CAS
    // token for that changed state. This is invalidation, not a success receipt.
    version.bump(context_changed, tasks_changed, agents_changed)?;
    if context_changed || tasks_changed || agents_changed {
        write_json(&paths.version_file, &version)?;
    }

    if context_changed {
        write_context(paths, after)?;
    }
    if tasks_changed {
        yaml_files::write_task_diff(paths, &before.tasks, &after.tasks)?;
    }
    if agents_changed {
        yaml_files::write_agents(paths, &after.agent_states)?;
    }
    Ok(version)
}

pub(super) fn write_context(paths: &ContextPaths, document: &ContextDoc) -> io::Result<()> {
    write_yaml(
        &paths.context_file,
        &json!({"coordination":document.coordination,"meta":document.meta}),
    )
}

pub(super) fn touch_updated_at(document: &mut ContextDoc) {
    document.updated_at = utc_now();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_persistence_cannot_reuse_the_previous_revision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = ContextPaths::new(temp.path());
        let mut before = ContextDoc::default();
        before
            .coordination
            .insert("brief".into(), json!({"objective":"before"}));
        persist_diff(&paths, &ContextDoc::default(), &before).expect("initial state");
        let previous = load(&paths).expect("initial revision");
        let mut after = previous.clone();
        after
            .coordination
            .insert("brief".into(), json!({"objective":"after"}));
        after.tasks.push(
            json!({"id":"T001", "title":"new task", "status":"planned"})
                .as_object()
                .cloned()
                .expect("task"),
        );
        // Simulate a destination becoming unavailable after the snapshot read.
        std::fs::create_dir(paths.tasks_dir.join("T001.yaml")).expect("block task destination");
        assert!(persist_diff(&paths, &previous, &after).is_err());
        std::fs::remove_dir(paths.tasks_dir.join("T001.yaml")).expect("restore destination");
        let actual = load(&paths).expect("read after failure");
        assert_eq!(actual.coordination["brief"]["objective"], "after");
        assert!(
            actual.revision > previous.revision,
            "partially changed state still accepts the stale version"
        );
    }
}
