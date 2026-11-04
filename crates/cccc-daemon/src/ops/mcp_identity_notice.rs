//! Per-request diagnostics for native sessions reading a message through an
//! MCP connection. Self-reported identity never grants or changes authority.
use cccc_contracts::Actor;
use cccc_core::{HomeLayout, actors, local_sessions};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Key {
    home: PathBuf,
    group: String,
    actor: String,
    actor_generation: String,
    session_revision: String,
}

#[derive(Default)]
struct Notices(HashMap<Key, Instant>);

impl Notices {
    fn allow(&mut self, key: Key, now: Instant) -> bool {
        self.0
            .retain(|_, last| now.saturating_duration_since(*last) < INTERVAL);
        if self.0.contains_key(&key) {
            return false;
        }
        self.0.insert(key, now);
        true
    }
}

fn is_reported(identity: Option<&Value>) -> bool {
    ["version", "source_id"].iter().all(|field| {
        identity
            .and_then(|identity| identity.get(field))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    })
}

/// Call only for the Actor's own successful exact history read. A missing
/// lifecycle record skips the notice rather than guessing its session identity.
pub(super) fn for_read(
    home: &HomeLayout,
    group_id: &str,
    actor: &Actor,
    identity: Option<&Value>,
) -> Option<&'static str> {
    static NOTICES: OnceLock<Mutex<Notices>> = OnceLock::new();
    if actor.external_session_id.is_empty() {
        return None;
    }
    if is_reported(identity) {
        return None;
    }
    let session = local_sessions::read(home, &actor.external_session_id).ok()?;
    let key = Key {
        home: home.root().to_owned(),
        group: group_id.to_owned(),
        actor: actor.id.clone(),
        actor_generation: actors::generation_identity(actor),
        session_revision: session.revision,
    };
    if !NOTICES
        .get_or_init(Mutex::default)
        .lock()
        .ok()?
        .allow(key, Instant::now())
    {
        return None;
    }
    Some(
        "[cccc] This request's client did not report a CCCC source identity. If you use a CCCC MCP server, its tools may be out of date. Ask the user to check and reconnect it if needed: https://chesterra.github.io/cccc/guide/local-sessions#upgrade-an-existing-integration",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn any_complete_identity_suppresses_the_missing_identity_notice() {
        assert!(!is_reported(None));
        assert!(!is_reported(Some(&json!({"version":"1"}))));
        assert!(!is_reported(Some(&json!({"version":"1","source_id":""}))));
        assert!(is_reported(Some(
            &json!({"version":"1","source_id":"source"})
        )));
        assert!(is_reported(Some(
            &json!({"version":"2","source_id":"other"})
        )));
    }

    #[test]
    fn throttle_only_suppresses_repeats_for_the_same_session() {
        let now = Instant::now();
        let mut notices = Notices::default();
        let key = Key {
            home: PathBuf::from("home"),
            group: "group".into(),
            actor: "actor".into(),
            actor_generation: "generation".into(),
            session_revision: "one".into(),
        };
        assert!(notices.allow(key.clone(), now));
        assert!(!notices.allow(key.clone(), now + Duration::from_secs(1)));
        assert!(notices.allow(
            Key {
                session_revision: "two".into(),
                ..key.clone()
            },
            now
        ));
        assert!(notices.allow(
            Key {
                actor_generation: "new".into(),
                ..key.clone()
            },
            now
        ));
        assert!(notices.allow(key, now + INTERVAL));
    }
}
