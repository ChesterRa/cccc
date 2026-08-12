use chrono::DateTime;
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
    load_canonical(home)
}

fn load_canonical(home: &HomeLayout) -> io::Result<GlobalSettings> {
    let path = home.root().join("settings.yaml");
    let mut settings = if path.exists() {
        read_yaml(&path)
    } else {
        Ok(GlobalSettings::default())
    }?;
    migrate_flat_observability(&mut settings.observability);
    Ok(settings)
}

pub fn update<T>(
    home: &HomeLayout,
    change: impl FnOnce(&mut GlobalSettings) -> io::Result<T>,
) -> io::Result<T> {
    // Legacy migration owns this same lock, so finish it before entering the
    // canonical read/modify/write section.
    migrate_legacy_json(home)?;
    with_exclusive_lock(&home.root().join("settings.yaml.lock"), || {
        let mut settings = load_canonical(home)?;
        let result = change(&mut settings)?;
        write_yaml(&home.root().join("settings.yaml"), &settings)?;
        Ok(result)
    })
}

pub fn save(home: &HomeLayout, settings: &GlobalSettings) -> io::Result<()> {
    write_yaml(&home.root().join("settings.yaml"), settings)
}

fn migrate_legacy_json(home: &HomeLayout) -> io::Result<()> {
    let legacy_path = home.root().join("settings.json");
    let canonical_path = home.root().join("settings.yaml");
    let marker_path = home.root().join(".rust-settings-migrated-v2");
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
        replace_section_when_source_is_newer(&mut canonical, &legacy, "remote_access");
        merge_missing(&mut canonical, &legacy);
        write_yaml(&canonical_path, &canonical)?;
        std::fs::write(&marker_path, b"migrated from settings.json\n")
    })
}

fn replace_section_when_source_is_newer(target: &mut Value, source: &Value, section: &str) {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    let Some(source_section) = source.get(section).and_then(Value::as_object) else {
        return;
    };
    let source_updated_at = source_section
        .get("updated_at")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
    let target_updated_at = target
        .get(section)
        .and_then(Value::as_object)
        .and_then(|value| value.get("updated_at"))
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
    let source_is_newer = source_updated_at
        .zip(target_updated_at)
        .is_some_and(|(source, target)| source > target);
    if source_is_newer {
        target.insert(section.to_owned(), Value::Object(source_section.clone()));
    }
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
    use std::sync::{Arc, Barrier};

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
    fn concurrent_updates_preserve_disjoint_sections() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let barrier = Arc::new(Barrier::new(2));

        let first_home = home.clone();
        let first_barrier = Arc::clone(&barrier);
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            update(&first_home, |settings| {
                settings
                    .observability
                    .insert("developer_mode".into(), json!(true));
                Ok(())
            })
        });
        let second_home = home.clone();
        let second = std::thread::spawn(move || {
            barrier.wait();
            update(&second_home, |settings| {
                settings
                    .branding
                    .insert("product_name".into(), json!("Concurrent"));
                Ok(())
            })
        });

        first.join().expect("first thread").expect("first update");
        second
            .join()
            .expect("second thread")
            .expect("second update");
        let settings = load(&home).expect("settings");
        assert_eq!(settings.observability["developer_mode"], json!(true));
        assert_eq!(settings.branding["product_name"], json!("Concurrent"));
    }

    #[test]
    fn update_finishes_legacy_migration_before_locking() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.json"),
            serde_json::to_vec(&json!({"legacy_section":{"kept":true}})).expect("json"),
        )
        .expect("legacy settings");

        update(&home, |settings| {
            settings
                .branding
                .insert("product_name".into(), json!("Migrated"));
            Ok(())
        })
        .expect("update");

        let settings = load(&home).expect("settings");
        assert_eq!(settings.extra["legacy_section"]["kept"], json!(true));
        assert_eq!(settings.branding["product_name"], json!("Migrated"));
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

    #[test]
    fn newer_legacy_remote_access_replaces_older_empty_canonical_values() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  updated_at: '2026-07-08T08:06:03Z'\n  web_public_url: ''\n",
        )
        .expect("canonical settings");
        std::fs::write(
            home.root().join("settings.json"),
            serde_json::to_vec(&json!({
                "remote_access": {
                    "updated_at": "2026-07-26T15:25:45Z",
                    "web_public_url": "https://cccc.example/",
                    "require_access_token": true
                }
            }))
            .expect("legacy settings json"),
        )
        .expect("legacy settings");
        std::fs::write(
            home.root().join(".rust-settings-migrated-v1"),
            "previous migration\n",
        )
        .expect("old marker");

        let settings = load(&home).expect("load settings");

        assert_eq!(
            settings.remote_access["web_public_url"],
            json!("https://cccc.example/")
        );
        assert!(home.root().join(".rust-settings-migrated-v2").exists());
    }

    #[test]
    fn older_legacy_remote_access_does_not_replace_newer_canonical_values() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  updated_at: '2026-07-30T08:06:03Z'\n  web_public_url: ''\n",
        )
        .expect("canonical settings");
        std::fs::write(
            home.root().join("settings.json"),
            serde_json::to_vec(&json!({
                "remote_access": {
                    "updated_at": "2026-07-26T15:25:45Z",
                    "web_public_url": "https://stale.example/"
                }
            }))
            .expect("legacy settings json"),
        )
        .expect("legacy settings");

        let settings = load(&home).expect("load settings");

        assert_eq!(settings.remote_access["web_public_url"], json!(""));
    }
}
