use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io;

use crate::HomeLayout;
use crate::fs::{read_json, read_yaml, write_json};

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
    } else if home.root().join("settings.yaml").exists() {
        read_yaml(&home.root().join("settings.yaml"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn loads_legacy_python_yaml_when_json_is_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  web_host: 0.0.0.0\n  web_port: 9000\n",
        )
        .expect("legacy settings");

        let settings = load(&home).expect("load legacy settings");
        assert_eq!(settings.remote_access["web_host"], json!("0.0.0.0"));
        assert_eq!(settings.remote_access["web_port"], json!(9000));
    }

    #[test]
    fn json_settings_take_precedence_over_legacy_yaml() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  web_host: 0.0.0.0\n",
        )
        .expect("legacy settings");
        save(
            &home,
            &GlobalSettings {
                remote_access: json!({"web_host":"127.0.0.2"})
                    .as_object()
                    .cloned()
                    .expect("object"),
                ..GlobalSettings::default()
            },
        )
        .expect("json settings");

        let settings = load(&home).expect("load json settings");
        assert_eq!(settings.remote_access["web_host"], json!("127.0.0.2"));
    }
}
