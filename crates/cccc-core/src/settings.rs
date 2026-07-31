use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io;

use crate::HomeLayout;
use crate::fs::{read_json, read_yaml, with_exclusive_lock, write_yaml};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalSettings {
    #[serde(default)]
    pub observability: Map<String, Value>,
    #[serde(default)]
    #[serde(rename = "web_branding", alias = "branding")]
    pub branding: Map<String, Value>,
    #[serde(default)]
    pub remote_access: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub fn load(home: &HomeLayout) -> io::Result<GlobalSettings> {
    migrate_legacy_json(home)?;
    let path = home.root().join("settings.yaml");
    let mut settings = if path.exists() {
        read_yaml(&path)
    } else {
        Ok(GlobalSettings::default())
    }?;
    migrate_flat_observability(&mut settings.observability);
    Ok(settings)
}

pub fn save(home: &HomeLayout, settings: &GlobalSettings) -> io::Result<()> {
    write_yaml(&home.root().join("settings.yaml"), settings)
}

fn migrate_legacy_json(home: &HomeLayout) -> io::Result<()> {
    let legacy_path = home.root().join("settings.json");
    let canonical_path = home.root().join("settings.yaml");
    let marker_path = home.root().join(".rust-settings-migrated-v1");
    if marker_path.exists() || !legacy_path.exists() {
        return Ok(());
    }
    with_exclusive_lock(&home.root().join("settings.yaml.lock"), || {
        if marker_path.exists() {
            return Ok(());
        }
        let mut legacy: Value = read_json(&legacy_path)?;
        if let Some(object) = legacy.as_object_mut()
            && let Some(branding) = object.remove("branding")
        {
            object.entry("web_branding").or_insert(branding);
        }
        let mut canonical = if canonical_path.exists() {
            read_yaml::<Value>(&canonical_path)?
        } else {
            Value::Object(Map::new())
        };
        merge_missing(&mut canonical, &legacy);
        write_yaml(&canonical_path, &canonical)?;
        std::fs::write(&marker_path, b"migrated from settings.json\n")
    })
}

fn merge_missing(target: &mut Value, source: &Value) {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        match target.get_mut(key) {
            Some(existing) if existing.is_object() && value.is_object() => {
                merge_missing(existing, value);
            }
            Some(_) => {}
            None => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
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

fn migrate_flat_observability(observability: &mut Map<String, Value>) {
    for (legacy_key, section, nested_key) in [
        (
            "terminal_transcript_per_actor_bytes",
            "terminal_transcript",
            "per_actor_bytes",
        ),
        (
            "terminal_ui_scrollback_lines",
            "terminal_ui",
            "scrollback_lines",
        ),
        (
            "peer_runtime_visibility",
            "runtime_visibility",
            "peer_runtime",
        ),
        (
            "assistant_runtime_visibility",
            "runtime_visibility",
            "assistant_runtime",
        ),
    ] {
        let Some(value) = observability.remove(legacy_key) else {
            continue;
        };
        let section = observability
            .entry(section)
            .or_insert_with(|| Value::Object(Map::new()));
        if !section.is_object() {
            *section = Value::Object(Map::new());
        }
        section
            .as_object_mut()
            .expect("observability section is an object")
            .entry(nested_key)
            .or_insert(value);
    }
    observability.remove("by");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn loads_canonical_python_yaml() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  web_host: 0.0.0.0\n  web_port: 9000\n",
        )
        .expect("canonical settings");

        let settings = load(&home).expect("load canonical settings");
        assert_eq!(settings.remote_access["web_host"], json!("0.0.0.0"));
        assert_eq!(settings.remote_access["web_port"], json!(9000));
    }

    #[test]
    fn saved_settings_replace_existing_canonical_yaml() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  web_host: 0.0.0.0\n",
        )
        .expect("existing settings");
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
        .expect("save settings");

        let settings = load(&home).expect("load saved settings");
        assert_eq!(settings.remote_access["web_host"], json!("127.0.0.2"));
    }

    #[test]
    fn migrates_flat_observability_fields_from_native_web_updates() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.json"),
            serde_json::to_vec(&json!({
                "observability": {
                    "by": "user",
                    "terminal_transcript_per_actor_bytes": 10485760,
                    "terminal_ui_scrollback_lines": 8000,
                    "peer_runtime_visibility": "visible",
                    "assistant_runtime_visibility": "visible"
                }
            }))
            .expect("settings json"),
        )
        .expect("write settings");

        let settings = load(&home).expect("load settings");
        assert_eq!(
            settings.observability["terminal_transcript"]["per_actor_bytes"],
            json!(10485760)
        );
        assert_eq!(
            settings.observability["terminal_ui"]["scrollback_lines"],
            json!(8000)
        );
        assert_eq!(
            settings.observability["runtime_visibility"],
            json!({"peer_runtime":"visible","assistant_runtime":"visible"})
        );
        assert!(!settings.observability.contains_key("by"));
        assert!(
            !settings
                .observability
                .contains_key("assistant_runtime_visibility")
        );
    }
}
