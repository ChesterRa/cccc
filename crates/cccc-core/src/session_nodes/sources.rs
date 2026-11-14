use super::store;
use crate::{GroupStore, HomeLayout, actors};
use cccc_contracts::{
    local_session::{LocalSessionBinding, LocalSessionRuntime},
    session_node::{RemoteSessionSource, SessionNodeMembership, SessionNodeSource},
};
use chrono::Utc;
use std::{collections::BTreeSet, io};

fn available(state: &store::State, source: &RemoteSessionSource) -> bool {
    state.hub.as_ref().is_some_and(|hub| hub.owner.alive())
        && state
            .peers
            .get(&source.node_id)
            .is_some_and(|peer| peer.enabled && peer.connected && peer.grant_id == source.grant_id)
        && source
            .updated_at
            .parse::<chrono::DateTime<Utc>>()
            .is_ok_and(|at| Utc::now().signed_duration_since(at).num_seconds() < 30)
}

pub fn update_sources(
    home: &HomeLayout,
    gateway: &str,
    node_id: &str,
    sources: Vec<SessionNodeSource>,
) -> io::Result<Vec<RemoteSessionSource>> {
    if sources.len() > 128 {
        return Err(io::Error::other("Too many exported sessions"));
    }
    let mut ids = BTreeSet::new();
    for item in &sources {
        let source = &item.source;
        if !matches!(
            source.runtime,
            LocalSessionRuntime::Codex | LocalSessionRuntime::Claude
        ) || !source
            .source_id
            .starts_with(&format!("{}:", source.runtime.as_str()))
            || (source.runtime == LocalSessionRuntime::Codex
                && source.source_id != source.runtime.source_id(&source.native_session_id))
            || source.source_id.len() > 512
            || source.native_session_id.is_empty()
            || source.cwd.len() > 4096
            || source.title.len() > 1024
            || item.session_revision.is_empty()
            || item.session_revision.len() > 256
            || !ids.insert(source.source_id.clone())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid exported native session",
            ));
        }
    }
    store::update(home, |state| {
        store::check_gateway(state, gateway)?;
        let peer = state
            .peers
            .get(node_id)
            .filter(|peer| peer.enabled && peer.connected)
            .ok_or_else(store::denied)?
            .clone();
        // Retain the last known identity for existing memberships, but never
        // present a disappeared or previous-grant source as usable.
        state.sources.retain(|_, source| source.node_id != node_id);
        let mut result = Vec::new();
        for item in sources {
            let native_source_id = item.source.source_id.clone();
            let mut source = item.source;
            source.source_id = format!(
                "remote:{}:{}:{}",
                peer.node_id,
                peer.grant_id,
                store::hash(&native_source_id)
            );
            source.origin = Some(format!("remote:{}", peer.name));
            source.connected &= item.alive;
            let record = RemoteSessionSource {
                source,
                native_source_id,
                session_revision: item.session_revision,
                node_id: node_id.into(),
                node_name: peer.name.clone(),
                grant_id: peer.grant_id.clone(),
                alive: item.alive,
                updated_at: Utc::now().to_rfc3339(),
            };
            state
                .sources
                .insert(record.source.source_id.clone(), record.clone());
            result.push(record);
        }
        Ok(result)
    })
}

pub fn list_sources(home: &HomeLayout) -> io::Result<Vec<RemoteSessionSource>> {
    let state = store::read(home)?;
    Ok(state
        .sources
        .values()
        .filter(|source| available(&state, source))
        .cloned()
        .collect())
}

pub fn source(home: &HomeLayout, source_id: &str) -> io::Result<RemoteSessionSource> {
    let state = store::read(home)?;
    state
        .sources
        .get(source_id)
        .filter(|source| source.alive && available(&state, source))
        .cloned()
        .ok_or_else(store::denied)
}

pub fn binding_valid(home: &HomeLayout, binding: &LocalSessionBinding) -> io::Result<()> {
    let source = source(home, &binding.source_id)?;
    let group = GroupStore::new(home.clone())?.load(&binding.group_id)?;
    let actor = actors::find(&group, &binding.actor_id).ok_or_else(store::denied)?;
    if source.session_revision != binding.session_revision
        || !actor.enabled
        || actor.external_session_id != binding.source_id
        || actors::generation_identity(actor) != binding.actor_generation
        || !matches!(
            (actor.runtime, source.source.runtime),
            (
                cccc_contracts::ActorRuntime::Codex,
                LocalSessionRuntime::Codex
            ) | (
                cccc_contracts::ActorRuntime::Claude,
                LocalSessionRuntime::Claude
            )
        )
    {
        return Err(store::denied());
    }
    Ok(())
}

pub fn memberships(home: &HomeLayout, node_id: &str) -> io::Result<Vec<SessionNodeMembership>> {
    let state = store::read(home)?;
    let hub = state
        .hub
        .as_ref()
        .filter(|hub| hub.owner.alive())
        .ok_or_else(store::denied)?;
    let sources = state
        .sources
        .values()
        .filter(|source| source.node_id == node_id && source.alive && available(&state, source))
        .collect::<Vec<_>>();
    let store = GroupStore::new(home.clone())?;
    let mut result = Vec::new();
    for meta in store.list()? {
        let group = store.load(&meta.group_id)?;
        for actor in &group.actors {
            if let Some(source) = sources
                .iter()
                .find(|source| source.source.source_id == actor.external_session_id)
            {
                result.push(SessionNodeMembership {
                    hub_id: hub.endpoint.endpoint_id.clone(),
                    source_id: source.native_source_id.clone(),
                    source_revision: source.session_revision.clone(),
                    group_id: group.group_id.clone(),
                    group_title: group.title.clone(),
                    actor_id: actor.id.clone(),
                    actor_generation: actors::generation_identity(actor),
                    role: if actors::effective_role(&group, &actor.id)
                        == Some(cccc_contracts::ActorRole::Foreman)
                    {
                        "foreman"
                    } else {
                        "peer"
                    }
                    .into(),
                    scope_key: actor.default_scope_key.clone(),
                    enabled: actor.enabled,
                });
            }
        }
    }
    Ok(result)
}
