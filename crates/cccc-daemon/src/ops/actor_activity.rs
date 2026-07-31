use cccc_contracts::{Actor, Event};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    state: String,
    runner: String,
}

#[derive(Default)]
pub struct Publisher {
    previous: BTreeMap<String, BTreeMap<String, Fingerprint>>,
}

impl Publisher {
    pub fn tick(&mut self, home: &HomeLayout) -> io::Result<()> {
        let store = GroupStore::new(home.clone())?;
        let groups = store.list()?;
        let known = groups
            .iter()
            .map(|group| group.group_id.clone())
            .collect::<BTreeSet<_>>();
        self.previous.retain(|group_id, _| known.contains(group_id));
        for meta in groups {
            let Ok(group) = store.load(&meta.group_id) else {
                continue;
            };
            self.publish_group(&store, &group);
        }
        Ok(())
    }

    fn publish_group(&mut self, store: &GroupStore, group: &GroupDoc) {
        let mut payloads = Vec::new();
        let mut snapshot = BTreeMap::new();
        for actor in &group.actors {
            if let Some(payload) = running_payload(store.home(), group, actor) {
                let runner = text(&payload, "runner_effective", "pty");
                let state = text(&payload, "effective_working_state", "waiting");
                snapshot.insert(actor.id.clone(), Fingerprint { state, runner });
                payloads.push(Value::Object(payload));
            }
        }
        let previous = self
            .previous
            .get(&group.group_id)
            .cloned()
            .unwrap_or_default();
        if previous == snapshot {
            return;
        }
        for (actor_id, fingerprint) in &previous {
            if !snapshot.contains_key(actor_id) {
                payloads.push(json!({
                    "id":actor_id,
                    "running":false,
                    "runner_effective":fingerprint.runner,
                    "idle_seconds":null,
                    "effective_working_state":"stopped",
                    "effective_working_reason":"runner_not_running",
                    "effective_working_updated_at":cccc_contracts::utc_now(),
                    "effective_active_task_id":null,
                }));
            }
        }
        self.previous.insert(group.group_id.clone(), snapshot);
        if payloads.is_empty() {
            return;
        }
        let mut event = Event::new("actor.activity", &group.group_id);
        event.by = "system".into();
        event.data = json!({"actors":payloads})
            .as_object()
            .cloned()
            .unwrap_or_default();
        let result = store
            .ledger_path(&group.group_id)
            .and_then(|path| ledger::append(&path, &event));
        if let Err(error) = result {
            tracing::warn!(
                %error,
                group_id = %group.group_id,
                "failed to append actor.activity"
            );
        }
    }
}

fn running_payload(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> Option<Map<String, Value>> {
    let status = super::actor_runtime::status(&group.group_id, &actor.id);
    let running = if super::actor_runtime::is_structured(actor) {
        if super::local_headless::supports(actor) {
            super::local_headless::running(&group.group_id, &actor.id)
        } else {
            actor.enabled && group.running && group.state != cccc_contracts::GroupState::Stopped
        }
    } else {
        status.is_some_and(|status| status.running)
    };
    if !running {
        return None;
    }
    let mut payload =
        super::working_state::runtime_actor_fields(home, actor, &group.group_id, true);
    payload.insert("id".into(), json!(actor.id));
    payload.insert("running".into(), Value::Bool(true));
    Some(payload)
}

fn text(payload: &Map<String, Value>, key: &str, default: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use cccc_contracts::{Actor, RunnerKind};
    use cccc_core::actors;
    use cccc_runtime::LaunchSpec;
    use std::collections::BTreeMap;

    #[test]
    fn publishes_initial_and_stopped_runtime_snapshots_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("activity", "").expect("group");
        store
            .mutate(&group.group_id, |doc| {
                let mut actor = Actor::new("peer");
                actor.runner = RunnerKind::Pty;
                actors::add(doc, actor)
            })
            .expect("actor");
        cccc_runtime::start(LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: "peer".into(),
            runner: RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), "sleep 5".into()],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("runtime");
        let mut publisher = Publisher::default();
        publisher.tick(&home).expect("initial tick");
        publisher.tick(&home).expect("stable tick");
        cccc_runtime::stop(&group.group_id, "peer").expect("stop");
        publisher.tick(&home).expect("stopped tick");

        let events = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("events")
            .into_iter()
            .filter(|event| event.kind == "actor.activity")
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data["actors"][0]["running"], true);
        assert_eq!(
            events[1].data["actors"][0]["effective_working_state"],
            "stopped"
        );
    }
}
