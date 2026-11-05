use super::*;

pub fn persist_lifecycle(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    enabled: bool,
    target_status: Option<&SessionStatus>,
) -> Result<Actor, OpError> {
    GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .mutate(&group.group_id, |doc| {
            // From the stored document: this launch may have adopted a Profile
            // snapshot, and with it a runtime that needs no process.
            let running = doc.actors.iter().any(|actor| {
                let enabled = if actor.id == actor_id {
                    enabled
                } else {
                    actor.enabled
                };
                enabled
                    && (actor_is_running(doc, actor)
                        || is_structured(actor)
                        || (actor.id == actor_id
                            && target_status.is_some_and(|status| status.running)))
            });
            let mut patch = serde_json::Map::new();
            patch.insert("enabled".into(), serde_json::Value::Bool(enabled));
            let actor = cccc_core::actors::update(doc, actor_id, &patch)?;
            if !actor.external_session_id.is_empty() {
                // Start/Stop manages native membership only, just like
                // actor_update(enabled). It cannot release Group Stop/Pause.
                if !enabled {
                    doc.running = doc.running && doc.actors.iter().any(|actor| actor.enabled);
                } else if matches!(
                    doc.state,
                    cccc_contracts::GroupState::Active | cccc_contracts::GroupState::Idle
                ) {
                    doc.running = true;
                }
            } else {
                doc.running = running;
                if enabled && doc.state == cccc_contracts::GroupState::Stopped {
                    doc.state = cccc_contracts::GroupState::Active;
                }
            }
            Ok(actor)
        })
        .map_err(OpError::invalid)
}

/// A linked Profile takes effect at the launch that replaces the owner. Its
/// non-private launch fields become the member's own here, so lists, pairing and
/// Mail notices describe what runs; saving a Profile changes no member by itself.
pub(super) fn adopt_profile_launch(
    home: &HomeLayout,
    group: &GroupDoc,
    resolved: &Actor,
) -> Result<(), OpError> {
    let Some(stored) = group.actors.iter().find(|actor| actor.id == resolved.id) else {
        return Ok(());
    };
    if resolved.profile_id.is_empty() || same_launch(stored, resolved) {
        return Ok(());
    }
    let route_changes = super::super::actors::web_model_route_changes(stored, resolved);
    let _browser_work = route_changes
        .then(|| super::hold_browser_work(home, &group.group_id, &resolved.id))
        .transpose()?;
    let retired = if route_changes {
        // Starting replaces the owner, so what it still held is settled.
        super::super::runtime_state::settle_active_turn(home, group, stored)?;
        cccc_core::web_model_connectors::retire_actor(home, &group.group_id, &resolved.id)
            .map_err(OpError::io)?
    } else {
        Vec::new()
    };
    let written = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .mutate(&group.group_id, |doc| {
            let Some(member) = doc.actors.iter_mut().find(|actor| {
                actor.id == resolved.id
                    && actor.profile_id == resolved.profile_id
                    && actor.profile_scope == resolved.profile_scope
                    && actor.profile_owner == resolved.profile_owner
            }) else {
                // Unlinked or relinked meanwhile: that change decides, not this launch.
                return Ok(false);
            };
            member.runtime = resolved.runtime;
            member.runtime_mode = resolved.runtime_mode;
            member.runner = resolved.runner;
            member.submit = resolved.submit;
            member.command = resolved.command.clone();
            member.profile_revision_applied = resolved.profile_revision_applied;
            member.updated_at = cccc_contracts::utc_now();
            Ok(true)
        })
        .map_err(OpError::invalid);
    if !matches!(written, Ok(true))
        && let Err(error) = cccc_core::web_model_connectors::restore(home, &retired)
    {
        tracing::warn!(%error, actor_id = %resolved.id, "failed to restore Web Model routes");
    }
    written.map(|_| ())
}

fn same_launch(stored: &Actor, resolved: &Actor) -> bool {
    stored.runtime == resolved.runtime
        && stored.runtime_mode == resolved.runtime_mode
        && stored.runner == resolved.runner
        && stored.submit == resolved.submit
        && stored.command == resolved.command
        && stored.profile_revision_applied == resolved.profile_revision_applied
}
