use super::*;

pub fn persist_lifecycle(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    enabled: bool,
    target_status: Option<&SessionStatus>,
) -> Result<Actor, OpError> {
    let running = group.actors.iter().any(|actor| {
        if actor.id == actor_id {
            enabled
                && (if super::super::local_headless::supports(actor) {
                    super::super::local_headless::running(&group.group_id, &actor.id)
                } else {
                    is_structured(actor) || target_status.is_some_and(|status| status.running)
                })
        } else {
            actor.enabled
                && (if super::super::local_headless::supports(actor) {
                    super::super::local_headless::running(&group.group_id, &actor.id)
                } else {
                    is_structured(actor)
                        || status(&group.group_id, &actor.id).is_some_and(|status| status.running)
                })
        }
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
