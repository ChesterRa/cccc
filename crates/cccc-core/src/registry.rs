use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;

use crate::fs::{read_json, with_exclusive_lock, write_json};
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
        if path.exists() {
            return Self::load_unlocked(home);
        }
        with_exclusive_lock(&registry_lock_path(home), || {
            if path.exists() {
                Self::load_unlocked(home)
            } else {
                let registry = Self::empty(home.clone());
                registry.save_unlocked()?;
                Ok(registry)
            }
        })
    }

    pub fn mutate<T>(
        home: &HomeLayout,
        change: impl FnOnce(&mut Self) -> io::Result<T>,
    ) -> io::Result<T> {
        home.initialize().map_err(io::Error::other)?;
        with_exclusive_lock(&registry_lock_path(home), || {
            let mut registry = if home.registry_path().exists() {
                Self::load_unlocked(home)?
            } else {
                Self::empty(home.clone())
            };
            let result = change(&mut registry)?;
            registry.updated_at = utc_now();
            registry.save_unlocked()?;
            Ok(result)
        })
    }

    fn load_unlocked(home: &HomeLayout) -> io::Result<Self> {
        let mut registry: Self = read_json(&home.registry_path())?;
        registry.home = Some(home.clone());
        Ok(registry)
    }

    fn save_unlocked(&self) -> io::Result<()> {
        let home = self
            .home
            .as_ref()
            .ok_or_else(|| io::Error::other("registry has no home"))?;
        let mut stored = self.clone();
        stored.updated_at = utc_now();
        write_json(&home.registry_path(), &stored)
    }
}

fn registry_lock_path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join("registry.json.lock")
}
