use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

use crate::HomeLayout;
use crate::capability_builtin;
use crate::fs::{read_json, write_json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub capsule_text: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityState {
    #[serde(default)]
    pub enabled: BTreeSet<String>,
    #[serde(default)]
    pub blocked: BTreeSet<String>,
    #[serde(default)]
    pub hidden: BTreeSet<String>,
    #[serde(default)]
    pub custom: BTreeMap<String, Capability>,
}

#[derive(Debug, Clone)]
pub struct CapabilityStore {
    home: HomeLayout,
}

impl CapabilityStore {
    #[must_use]
    pub fn new(home: HomeLayout) -> Self {
        Self { home }
    }

    pub fn load(&self) -> io::Result<CapabilityState> {
        let path = self.path();
        if path.exists() {
            read_json(&path)
        } else {
            Ok(CapabilityState::default())
        }
    }

    pub fn save(&self, state: &CapabilityState) -> io::Result<()> {
        write_json(&self.path(), state)
    }

    pub fn catalog(&self) -> io::Result<Vec<Capability>> {
        let mut items = capability_builtin::all();
        items.extend(self.load()?.custom.into_values());
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(items)
    }

    pub fn search(&self, query: &str) -> io::Result<Vec<Capability>> {
        let terms: Vec<_> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        Ok(self
            .catalog()?
            .into_iter()
            .filter(|item| {
                if terms.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {} {}",
                    item.id,
                    item.name,
                    item.description,
                    item.tags.join(" ")
                )
                .to_lowercase();
                terms.iter().all(|term| haystack.contains(term))
            })
            .collect())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> io::Result<CapabilityState> {
        self.require(id)?;
        let mut state = self.load()?;
        if enabled {
            if state.blocked.contains(id) {
                return Err(io::Error::other("capability is blocked"));
            }
            state.enabled.insert(id.into());
        } else {
            state.enabled.remove(id);
        }
        self.save(&state)?;
        Ok(state)
    }

    pub fn set_blocked(&self, id: &str, blocked: bool) -> io::Result<CapabilityState> {
        self.require(id)?;
        let mut state = self.load()?;
        if blocked {
            state.blocked.insert(id.into());
            state.enabled.remove(id);
        } else {
            state.blocked.remove(id);
        }
        self.save(&state)?;
        Ok(state)
    }

    pub fn set_hidden(&self, id: &str, hidden: bool) -> io::Result<CapabilityState> {
        self.require(id)?;
        let mut state = self.load()?;
        if hidden {
            state.hidden.insert(id.into());
        } else {
            state.hidden.remove(id);
        }
        self.save(&state)?;
        Ok(state)
    }

    pub fn import(&self, capability: Capability) -> io::Result<CapabilityState> {
        validate_id(&capability.id)?;
        let mut state = self.load()?;
        state.custom.insert(capability.id.clone(), capability);
        self.save(&state)?;
        Ok(state)
    }

    pub fn uninstall(&self, id: &str) -> io::Result<bool> {
        let mut state = self.load()?;
        let removed = state.custom.remove(id).is_some();
        state.enabled.remove(id);
        state.blocked.remove(id);
        state.hidden.remove(id);
        self.save(&state)?;
        Ok(removed)
    }

    pub fn delete_source(&self, source: &str) -> io::Result<Vec<String>> {
        let mut state = self.load()?;
        let removed = state
            .custom
            .iter()
            .filter(|(_, capability)| capability.source == source)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &removed {
            state.custom.remove(id);
            state.enabled.remove(id);
            state.blocked.remove(id);
            state.hidden.remove(id);
        }
        self.save(&state)?;
        Ok(removed)
    }

    pub fn require(&self, id: &str) -> io::Result<Capability> {
        self.catalog()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("capability not found: {id}"),
                )
            })
    }

    fn path(&self) -> std::path::PathBuf {
        self.home.root().join("capabilities.json")
    }
}

fn validate_id(id: &str) -> io::Result<()> {
    let valid = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(io::Error::other("invalid capability id"))
    }
}
