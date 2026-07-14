use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io;

use crate::HomeLayout;
use crate::fs::{read_json, write_json};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalSettings {
    #[serde(default)]
    pub observability: Map<String, Value>,
    #[serde(default)]
    pub branding: Map<String, Value>,
    #[serde(default)]
    pub remote_access: Map<String, Value>,
    #[serde(default)]
    pub capability_allowlist: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub fn load(home: &HomeLayout) -> io::Result<GlobalSettings> {
    let path = home.root().join("settings.json");
    if path.exists() {
        read_json(&path)
    } else {
        Ok(GlobalSettings::default())
    }
}

pub fn save(home: &HomeLayout, settings: &GlobalSettings) -> io::Result<()> {
    write_json(&home.root().join("settings.json"), settings)
}

pub fn merge(target: &mut Map<String, Value>, patch: &Map<String, Value>) {
    for (key, value) in patch {
        if value.is_null() {
            target.remove(key);
        } else if let (Some(existing), Some(nested)) = (
            target.get_mut(key).and_then(Value::as_object_mut),
            value.as_object(),
        ) {
            merge(existing, nested);
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}
