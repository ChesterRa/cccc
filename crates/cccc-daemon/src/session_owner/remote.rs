//! Local facts for cloud-backed members. Routes and unfinished work survive
//! maintenance; none of these records proves a cloud process is running.
use super::receipt::AgentIdentity;
use cccc_contracts::{Actor, ActorRuntime, GroupState};
use cccc_core::update_capabilities::KeepRefusal;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, Registry, web_model_connectors};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io;
use std::time::Instant;

#[derive(Default)]
pub(super) struct Snapshot {
    pub groups: Vec<GroupDoc>,
    pub covered: BTreeSet<(String, String, String)>,
    pub active: Vec<AgentIdentity>,
    pub refusals: Vec<KeepRefusal>,
}

pub(super) type MemberKey = (String, String, String);

fn within_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "member check deadline elapsed",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn read(
    home: &HomeLayout,
    deadline: Instant,
    held: &BTreeSet<MemberKey>,
) -> io::Result<Snapshot> {
    within_deadline(deadline)?;
    // Registry::load would create a document during an observation.
    let registry: Registry = match cccc_core::fs::read_json(&home.registry_path()) {
        Ok(registry) => registry,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Snapshot::default()),
        Err(error) => return Err(error),
    };
    let store = GroupStore::new(home.clone())?;
    let mut result = Snapshot::default();
    for (id, meta) in registry.groups {
        within_deadline(deadline)?;
        let group = store.load(&id)?;
        if meta.group_id != id || group.group_id != id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registered group identity changed",
            ));
        }
        if group.running && group.state != GroupState::Stopped {
            result.groups.push(group);
        }
    }
    let is_unheld_remote = |group: &GroupDoc, actor: &Actor| {
        is_remote(actor)
            && !held.contains(&(
                group.group_id.clone(),
                actor.id.clone(),
                actor.created_at.clone(),
            ))
    };
    let has_remote_members = result.groups.iter().any(|group| {
        group
            .actors
            .iter()
            .any(|actor| is_unheld_remote(group, actor))
    });
    let connectors = if has_remote_members {
        match web_model_connectors::snapshot(home) {
            Ok(connectors) => connectors,
            Err(_) => {
                result.refusals.push(KeepRefusal {
                    code: "remote_resource_unconfirmed".into(),
                    group_id: None,
                    actor_id: None,
                    owner_id: None,
                    display_name: None,
                    message: "CCCC could not read the saved remote connections; inspect their current state before retrying.".into(),
                });
                return Ok(result);
            }
        }
    } else {
        Vec::new()
    };
    for group in &result.groups {
        for actor in group
            .actors
            .iter()
            .filter(|actor| is_unheld_remote(group, actor))
        {
            within_deadline(deadline)?;
            result.covered.insert((
                group.group_id.clone(),
                actor.id.clone(),
                actor.created_at.clone(),
            ));
            match activity(group, actor, &connectors) {
                Ok(true) => result.active.push(AgentIdentity {
                    group_id: group.group_id.clone(),
                    actor_id: actor.id.clone(),
                    actor_created_at: actor.created_at.clone(),
                    // A turn or a route revision is not a process generation.
                    generation: None,
                    owner_id: None,
                }),
                Ok(false) => {}
                Err(message) => result.refusals.push(KeepRefusal {
                    code: "remote_resource_unconfirmed".into(),
                    group_id: Some(group.group_id.clone()),
                    actor_id: Some(actor.id.clone()),
                    owner_id: None,
                    display_name: (!actor.title.trim().is_empty())
                        .then(|| actor.title.trim().into()),
                    message: message.into(),
                }),
            }
        }
    }
    within_deadline(deadline)?;
    Ok(result)
}

fn is_remote(actor: &Actor) -> bool {
    actor.enabled
        && actor.external_session_id.is_empty()
        && matches!(
            actor.runtime,
            ActorRuntime::ChatgptDots | ActorRuntime::GrokWebModel
        )
}

fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| !value.is_empty())
}

fn activity(group: &GroupDoc, actor: &Actor, connectors: &[Value]) -> Result<bool, &'static str> {
    let active_turn = group
        .extra
        .get("runtime_states")
        .and_then(|states| states.get(&actor.id))
        .and_then(|state| state.get("active_turn_id"))
        .and_then(text)
        .is_some();
    let provider = actor.runtime.web_model_provider();
    let routes = connectors
        .iter()
        .filter(|connector| {
            connector["revoked"] != true && connector["provider"].as_str() == provider
        })
        .flat_map(|connector| {
            connector["bindings"]
                .as_object()
                .into_iter()
                .flat_map(|bindings| bindings.values())
                .filter(|binding| {
                    binding["group_id"] == group.group_id
                        && binding["actor_id"] == actor.id
                        && binding["state"] == "bound"
                })
                .map(move |binding| (connector, binding))
        })
        .collect::<Vec<_>>();
    if routes.is_empty() {
        return if active_turn {
            Err(
                "This remote member has unfinished work but no matching saved route; inspect its connection and current turn before retrying.",
            )
        } else {
            Ok(false)
        };
    }
    if routes.len() != 1 {
        return Err(
            "More than one saved route claims this remote member; inspect its connection before retrying.",
        );
    }
    let (connector, binding) = routes[0];
    let generation = cccc_core::actors::generation_identity(actor);
    let transport = if actor.runtime == ActorRuntime::ChatgptDots {
        "mcp_events"
    } else {
        "grok_webhook"
    };
    if binding["generation"] != generation
        || binding["transport"] != transport
        || text(&binding["revision"]).is_none()
        || web_model_connectors::binding_for_actor(
            connector,
            &group.group_id,
            &actor.id,
            &generation,
        )
        .as_ref()
            != Some(binding)
    {
        return Err(
            "The saved remote route does not match this member's current identity; inspect its connection before retrying.",
        );
    }
    let private = if actor.runtime == ActorRuntime::ChatgptDots {
        &connector["dots"]
    } else {
        web_model_connectors::grok_webhook::state(connector, &group.group_id, &actor.id)
    };
    if private.is_null() && !active_turn {
        // An Actor may have a route but no configured event subscription or
        // Routine yet. There is no launched work to put in the result cohort.
        return Ok(false);
    }
    if !private.is_object()
        || text(&private["epoch"]).is_none()
        || private["group_id"] != group.group_id
        || private["actor_id"] != actor.id
        || (actor.runtime == ActorRuntime::ChatgptDots
            && (private["generation"] != generation || private["revision"] != binding["revision"]))
    {
        return Err(
            "CCCC could not confirm this member's saved remote work state; inspect its connection and current turn before retrying.",
        );
    }
    if active_turn
        || matches!(
            private["execution"]["state"].as_str(),
            Some("active" | "interrupted")
        )
    {
        // Canonical work remains unfinished even if a crash lost its private
        // execution projection. Existing explicit turn resolution still owns it.
        return Ok(true);
    }
    // Failed is a terminal delivery result, not proof of cloud exit. Grok may
    // still accept its late claim; Dots will not. Maintenance changes neither
    // authorization rule and does not turn failure into an endless activity row.
    let wake_active = matches!(
        private["wake"]["state"].as_str(),
        Some("pending" | "sending" | "delivered" | "claimed")
    );
    if actor.runtime == ActorRuntime::ChatgptDots && wake_active {
        let subscription = &private["subscription"];
        return Ok(subscription["enabled"] == true
            && (subscription["expires_at_ms"].is_null()
                || subscription["expires_at_ms"]
                    .as_i64()
                    .is_some_and(|expires| expires > chrono::Utc::now().timestamp_millis())));
    }
    Ok(wake_active)
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
