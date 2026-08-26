use cccc_contracts::{Event, GroupState};
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
        let target_ids = group
            .actors
            .iter()
            .filter(|actor| cccc_core::inbox::is_for_actor(&group, &event, &actor.id))
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
