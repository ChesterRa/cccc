use cccc_core::{GroupStore, HomeLayout, actors, settings};
use cccc_runtime::HistoryConfig;

const DEFAULT_TRANSCRIPT_BYTES: usize = 10 * 1024 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 50_000_000;

pub(super) fn config(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> std::io::Result<HistoryConfig> {
    let actor_id = actors::validate_actor_id(actor_id)?;
    let settings = settings::load(home)?;
    let requested_bytes = settings
        .observability
        .get("terminal_transcript")
        .and_then(|value| value.get("per_actor_bytes"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(DEFAULT_TRANSCRIPT_BYTES);
    let max_bytes = if requested_bytes == 0 {
        DEFAULT_TRANSCRIPT_BYTES
    } else {
        requested_bytes.min(MAX_TRANSCRIPT_BYTES)
    };
    let persist = settings
        .observability
        .get("terminal_transcript")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|value| {
            value.get("enabled").and_then(serde_json::Value::as_bool) == Some(true)
                && value.get("persist").and_then(serde_json::Value::as_bool) == Some(true)
        });
    let actor_dir = GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("terminal")
        .join(actor_id);
    let session_id = uuid::Uuid::new_v4().simple();
    Ok(HistoryConfig {
        path: actor_dir.join(format!("{session_id}.pty")),
        max_bytes,
        hot_bytes: max_bytes,
        persist,
    })
}

pub(crate) fn actor_dir(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> std::io::Result<std::path::PathBuf> {
    let actor_id = actors::validate_actor_id(actor_id)?;
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("terminal")
        .join(actor_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_existing_observability_limit_and_safe_actor_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("history", "").expect("group");
        let mut settings = settings::load(&home).expect("settings");
        settings.observability.insert(
            "terminal_transcript".into(),
            serde_json::json!({"per_actor_bytes": 2 * 1024 * 1024}),
        );
        settings::save(&home, &settings).expect("save");

        let config = config(&home, &group.group_id, "peer-1").expect("config");

        assert_eq!(config.max_bytes, 2 * 1024 * 1024);
        assert_eq!(config.hot_bytes, 2 * 1024 * 1024);
        assert!(!config.persist);
        assert!(config.path.starts_with(
            actor_dir(&home, &group.group_id, "peer-1").expect("valid actor history directory"),
        ));
        assert_eq!(
            config.path.extension().and_then(|value| value.to_str()),
            Some("pty")
        );
    }

    #[test]
    fn clamps_transcript_limit_like_the_python_pty_backlog() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("history-cap", "").expect("group");
        let mut settings = settings::load(&home).expect("settings");
        settings.observability.insert(
            "terminal_transcript".into(),
            serde_json::json!({"per_actor_bytes": 1024_u64 * 1024 * 1024}),
        );
        settings::save(&home, &settings).expect("save");

        let config = config(&home, &group.group_id, "peer-1").expect("config");

        assert_eq!(config.max_bytes, MAX_TRANSCRIPT_BYTES);
        assert_eq!(config.hot_bytes, MAX_TRANSCRIPT_BYTES);
    }

    #[test]
    fn zero_transcript_limit_uses_the_python_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("history-default", "").expect("group");
        let mut settings = settings::load(&home).expect("settings");
        settings.observability.insert(
            "terminal_transcript".into(),
            serde_json::json!({"per_actor_bytes": 0}),
        );
        settings::save(&home, &settings).expect("save");

        let config = config(&home, &group.group_id, "peer-1").expect("config");

        assert_eq!(config.max_bytes, DEFAULT_TRANSCRIPT_BYTES);
        assert_eq!(config.hot_bytes, DEFAULT_TRANSCRIPT_BYTES);
    }

    #[test]
    fn persistence_requires_both_enabled_and_persist() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("history-persist", "").expect("group");
        let mut settings = settings::load(&home).expect("settings");
        settings.observability.insert(
            "terminal_transcript".into(),
            serde_json::json!({"enabled": true, "persist": true}),
        );
        settings::save(&home, &settings).expect("save");

        let config = config(&home, &group.group_id, "peer-1").expect("config");

        assert!(config.persist);
    }
}
