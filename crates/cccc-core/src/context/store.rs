use serde_json::{Map, Value};
use std::io;

use super::apply::apply_all;
use super::migration::migrate_legacy_json;
use super::model::{ContextDoc, ContextSyncResult};
use super::yaml_storage::{self, ContextPaths};
use crate::fs::with_exclusive_lock;
use crate::{GroupStore, HomeLayout};

#[derive(Debug, Clone)]
pub struct ContextStore {
    home: HomeLayout,
}

impl ContextStore {
    pub fn new(home: HomeLayout) -> io::Result<Self> {
        home.initialize().map_err(io::Error::other)?;
        Ok(Self { home })
    }

    pub fn load(&self, group_id: &str) -> io::Result<ContextDoc> {
        let paths = self.paths(group_id)?;
        with_exclusive_lock(&paths.lock_file, || {
            migrate_legacy_json(&paths)?;
            yaml_storage::load(&paths)
        })
    }

    pub fn load_overview(&self, group_id: &str) -> io::Result<ContextDoc> {
        let paths = self.paths(group_id)?;
        with_exclusive_lock(&paths.lock_file, || {
            migrate_legacy_json(&paths)?;
            yaml_storage::load_overview(&paths)
        })
    }

    pub fn version(&self, document: &ContextDoc) -> io::Result<String> {
        Ok(format!("ctxv:{}", document.revision))
    }

    pub fn tasks_version(&self, document: &ContextDoc) -> String {
        format!("tasksv:{}", document.tasks_revision)
    }

    pub fn sync(
        &self,
        group_id: &str,
        operations: &[Map<String, Value>],
        if_version: Option<&str>,
        by: &str,
        dry_run: bool,
    ) -> io::Result<ContextSyncResult> {
        let paths = self.paths(group_id)?;
        with_exclusive_lock(&paths.lock_file, || {
            migrate_legacy_json(&paths)?;
            let before = yaml_storage::load(&paths)?;
            let current_version = self.version(&before)?;
            if if_version.is_some_and(|expected| expected != current_version) {
                return Err(io::Error::other("version_conflict"));
            }
            let mut document = before.clone();
            let changes = apply_all(&mut document, operations, by)?;
            yaml_storage::touch_updated_at(&mut document);
            let version = if dry_run {
                current_version
            } else {
                let state = yaml_storage::persist_diff(&paths, &before, &document)?;
                document.revision = state.global_rev;
                document.tasks_revision = state.tasks_rev;
                self.version(&document)?
            };
            Ok(ContextSyncResult {
                context: document,
                version,
                changes,
                dry_run,
            })
        })
    }

    fn paths(&self, group_id: &str) -> io::Result<ContextPaths> {
        let groups = GroupStore::new(self.home.clone())?;
        Ok(ContextPaths::new(&groups.group_dir(group_id)?))
    }
}
