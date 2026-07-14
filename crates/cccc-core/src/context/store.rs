use cccc_contracts::utc_now;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::io;
use std::path::PathBuf;

use super::apply::apply_all;
use super::model::{ContextDoc, ContextSyncResult};
use crate::fs::{read_json, write_json};
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
        let path = self.path(group_id)?;
        if path.exists() {
            read_json(&path)
        } else {
            Ok(ContextDoc::default())
        }
    }

    pub fn version(&self, document: &ContextDoc) -> io::Result<String> {
        let bytes = serde_json::to_vec(document).map_err(io::Error::other)?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }

    pub fn sync(
        &self,
        group_id: &str,
        operations: &[Map<String, Value>],
        if_version: Option<&str>,
        by: &str,
        dry_run: bool,
    ) -> io::Result<ContextSyncResult> {
        let mut document = self.load(group_id)?;
        let current_version = self.version(&document)?;
        if if_version.is_some_and(|expected| expected != current_version) {
            return Err(io::Error::other("version_conflict"));
        }
        let changes = apply_all(&mut document, operations, by)?;
        if !changes.is_empty() {
            document.revision += 1;
            document.updated_at = utc_now();
        }
        let version = self.version(&document)?;
        if !dry_run && !changes.is_empty() {
            write_json(&self.path(group_id)?, &document)?;
        }
        Ok(ContextSyncResult {
            context: document,
            version,
            changes,
            dry_run,
        })
    }

    fn path(&self, group_id: &str) -> io::Result<PathBuf> {
        let state = GroupStore::new(self.home.clone())?.state_dir(group_id)?;
        Ok(state.join("context.json"))
    }
}
