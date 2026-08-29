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
    #[serde(default)]
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
    pub fn load(paths: &ContextPaths) -> Self {
        read_json(&paths.version_file).unwrap_or_else(|_| {
            let has_context = paths.context_file.is_file();
            let has_tasks = yaml_files::has_tasks(&paths.tasks_dir);
            let has_agents = paths.agents_file.is_file();
            let baseline = u64::from(has_context || has_tasks || has_agents);
            Self {
                global_rev: baseline,
                context_rev: u64::from(has_context),
                tasks_rev: u64::from(has_tasks),
                agents_rev: u64::from(has_agents),
                actors_rev: 0,
            }
        })
    }

    pub fn bump(&mut self, context: bool, tasks: bool, agents: bool) {
        if !(context || tasks || agents) {
            return;
        }
        self.global_rev += 1;
        self.context_rev += u64::from(context);
        self.tasks_rev += u64::from(tasks);
        self.agents_rev += u64::from(agents);
    }
}

pub(super) fn load(paths: &ContextPaths) -> io::Result<ContextDoc> {
    load_with_tasks(paths, true)
}

pub(super) fn load_overview(paths: &ContextPaths) -> io::Result<ContextDoc> {
    load_with_tasks(paths, false)
}

fn load_with_tasks(paths: &ContextPaths, include_tasks: bool) -> io::Result<ContextDoc> {
    let context = yaml_files::read_yaml_map(&paths.context_file);
    let coordination = context
        .get("coordination")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let meta = context
        .get("meta")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let version = VersionState::load(paths);
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
        agent_states: yaml_files::load_agents(&paths.agents_file),
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
    let mut version = VersionState::load(paths);
    if context_changed {
        write_context(paths, after)?;
    }
    if tasks_changed {
        yaml_files::write_task_diff(paths, &before.tasks, &after.tasks)?;
    }
    if agents_changed {
        yaml_files::write_agents(paths, &after.agent_states)?;
    }
    version.bump(context_changed, tasks_changed, agents_changed);
    if context_changed || tasks_changed || agents_changed {
        write_json(&paths.version_file, &version)?;
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
