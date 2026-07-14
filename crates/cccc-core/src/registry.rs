use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;

use crate::fs::{read_json, write_json};
use crate::home::HomeLayout;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupMeta {
    pub group_id: String,
    pub title: String,
    #[serde(default)]
    pub topic: String,
    pub path: String,
    #[serde(default)]
    pub default_scope_key: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registry {
    pub v: u8,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub groups: BTreeMap<String, GroupMeta>,
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    #[serde(skip)]
    home: Option<HomeLayout>,
}

impl Registry {
    #[must_use]
    pub fn empty(home: HomeLayout) -> Self {
        let now = utc_now();
        Self {
            v: 1,
            created_at: now.clone(),
            updated_at: now,
            groups: BTreeMap::new(),
            defaults: BTreeMap::new(),
            home: Some(home),
        }
    }

    pub fn load(home: &HomeLayout) -> io::Result<Self> {
        home.initialize().map_err(io::Error::other)?;
        let path = home.registry_path();
        if !path.exists() {
            let registry = Self::empty(home.clone());
            registry.save()?;
            return Ok(registry);
        }
        let mut registry: Self = read_json(&path)?;
        registry.home = Some(home.clone());
        Ok(registry)
    }

    pub fn save(&self) -> io::Result<()> {
        let home = self
            .home
            .as_ref()
            .ok_or_else(|| io::Error::other("registry has no home"))?;
        let mut stored = self.clone();
        stored.updated_at = utc_now();
        write_json(&home.registry_path(), &stored)
    }

    pub fn insert(&mut self, meta: GroupMeta) -> io::Result<()> {
        self.groups.insert(meta.group_id.clone(), meta);
        self.updated_at = utc_now();
        self.save()
    }

    pub fn remove(&mut self, group_id: &str) -> io::Result<Option<GroupMeta>> {
        let removed = self.groups.remove(group_id);
        self.defaults.retain(|_, value| value != group_id);
        self.updated_at = utc_now();
        self.save()?;
        Ok(removed)
    }
}
