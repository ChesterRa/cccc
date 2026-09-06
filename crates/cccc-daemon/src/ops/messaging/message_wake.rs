use cccc_contracts::{ActorRole, Event, GroupState};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use serde_json::{Map, Value};

use crate::dispatch::OpError;

pub(super) fn wake_message_targets(
    home: &HomeLayout,
    group: GroupDoc,
    by: &str,
    data: &Map<String, Value>,
) -> Result<GroupDoc, OpError> {
    if by == "user" {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = by.to_owned();
        event.data.clone_from(data);
        let recipients = event
            .data
            .get("to")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let target_ids = group
            .actors
            .iter()
            .filter(|actor| cccc_core::inbox::is_for_actor(&group, &event, &actor.id))
            // Broadcast membership is not consent to re-enable a stopped Actor.
            // Explicit IDs and the single-role selector keep their wake behavior.
            .filter(|actor| {
                actor.enabled
                    || recipients.contains(&actor.id.as_str())
                    || (recipients.contains(&"@foreman")
                        && cccc_core::actors::effective_role(&group, &actor.id)
                            == Some(ActorRole::Foreman))
            })
            .map(|actor| actor.id.clone())
            .collect::<Vec<_>>();
        return activate_message_targets(home, group, &target_ids);
    }

    let external_message =
        !by.is_empty() && by != "system" && !group.actors.iter().any(|actor| actor.id == by);
    if group.state != GroupState::Idle || !external_message {
        return Ok(group);
    }
    store(home)?
        .mutate(&group.group_id, |current| {
            if current.state == GroupState::Idle {
                current.state = GroupState::Active;
            }
            Ok(current.clone())
        })
        .map_err(OpError::io)
}

pub(super) fn activate_message_targets(
    home: &HomeLayout,
    group: GroupDoc,
    target_ids: &[String],
) -> Result<GroupDoc, OpError> {
    if target_ids.is_empty() {
        return Ok(group);
    }
    let needs_activation = group.state != GroupState::Active
        || !group.running
        || group
            .actors
            .iter()
            .any(|actor| target_ids.contains(&actor.id) && !actor.enabled);
    if !needs_activation {
        return Ok(group);
    }
    if matches!(
        group.state,
        GroupState::Idle | GroupState::Paused | GroupState::Stopped
    ) {
        cccc_core::automation::reset_rule_timers_on_resume(home, &group.group_id)
            .map_err(OpError::io)?;
    }
    store(home)?
        .mutate(&group.group_id, |current| {
            current.state = GroupState::Active;
            current.running = true;
            for actor in &mut current.actors {
                if target_ids.contains(&actor.id) {
                    actor.enabled = true;
                }
            }
            Ok(current.clone())
        })
        .map_err(OpError::io)
}

fn store(home: &HomeLayout) -> Result<GroupStore, OpError> {
    GroupStore::new(home.clone()).map_err(OpError::io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;
    use serde_json::json;

    #[test]
    fn only_explicit_recipients_reenable_disabled_actors() {
        for (to, expected) in [
            (json!(["@all"]), vec!["peer1"]),
            (json!(["@peers"]), vec!["peer1"]),
            (json!(["lead"]), vec!["lead", "peer1"]),
            (json!(["@foreman"]), vec!["lead", "peer1"]),
            (json!(["peer2"]), vec!["peer1", "peer2"]),
            (json!(["@all", "peer2"]), vec!["peer1", "peer2"]),
            (json!(["@peers", "@foreman"]), vec!["lead", "peer1"]),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            let home = HomeLayout::from_path(temp.path()).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let group = store.create("wake recipients", "").expect("create group");
            let group = store
                .mutate(&group.group_id, |group| {
                    group.state = GroupState::Paused;
                    group.running = true;
                    group.actors = ["lead", "peer1", "peer2"]
                        .into_iter()
                        .map(|id| {
                            let mut actor = Actor::new(id);
                            actor.enabled = id == "peer1";
                            actor
                        })
                        .collect();
                    Ok(group.clone())
                })
                .expect("paused group");
            let data = json!({"to":to}).as_object().expect("data").clone();
            let awakened = wake_message_targets(&home, group, "user", &data).expect("wake");
            let persisted = store.load(&awakened.group_id).expect("load group");
            assert_eq!(persisted.state, GroupState::Active, "to={to}");
            assert!(persisted.running, "to={to}");
            let enabled = persisted
                .actors
                .iter()
                .filter(|actor| actor.enabled)
                .map(|actor| actor.id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(enabled, expected, "to={to}");
        }
    }

    #[test]
    fn broadcast_without_enabled_recipients_does_not_resume_group() {
        for state in [GroupState::Paused, GroupState::Stopped] {
            let temp = tempfile::tempdir().expect("tempdir");
            let home = HomeLayout::from_path(temp.path()).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let group = store
                .create("disabled recipients", "")
                .expect("create group");
            let group = store
                .mutate(&group.group_id, |group| {
                    group.state = state;
                    group.running = false;
                    let mut actor = Actor::new("lead");
                    actor.enabled = false;
                    group.actors = vec![actor];
                    Ok(group.clone())
                })
                .expect("disabled group");
            let data = json!({"to":["@all"]}).as_object().expect("data").clone();
            let result = wake_message_targets(&home, group, "user", &data).expect("send");
            let persisted = store.load(&result.group_id).expect("load group");
            assert_eq!(persisted.state, state);
            assert!(!persisted.running);
            assert!(!persisted.actors[0].enabled);
        }
    }
}
