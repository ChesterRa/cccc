use super::*;

pub fn persist_lifecycle(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    enabled: bool,
    target_status: Option<&SessionStatus>,
) -> Result<Actor, OpError> {
    let running = group.actors.iter().any(|actor| {
        let enabled = if actor.id == actor_id {
            enabled
        } else {
            actor.enabled
        };
        enabled
            && (actor_is_running(group, actor)
                || is_structured(actor)
                || (actor.id == actor_id && target_status.is_some_and(|status| status.running)))
    });
    GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .mutate(&group.group_id, |doc| {
            let mut patch = serde_json::Map::new();
            patch.insert("enabled".into(), serde_json::Value::Bool(enabled));
            let actor = cccc_core::actors::update(doc, actor_id, &patch)?;
            doc.running = running;
            if enabled && doc.state == cccc_contracts::GroupState::Stopped {
                doc.state = cccc_contracts::GroupState::Active;
            }
            Ok(actor)
        })
        .map_err(OpError::invalid)
}
