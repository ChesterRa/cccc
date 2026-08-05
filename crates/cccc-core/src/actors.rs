use cccc_contracts::{Actor, ActorRole, utc_now};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

use crate::GroupDoc;

const RESERVED: &[&str] = &[
    "user", "all", "system", "foreman", "peers", "admin", "root", "cccc",
];

/// Existing recipient selector used when a cross-group caller omits `to`.
pub const CROSS_GROUP_FOREMAN_RECIPIENT: &str = "@foreman";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniqueForemanError {
    NotFound,
    NotUnique,
}

pub fn validate_actor_id(value: &str) -> io::Result<String> {
    let id = value.trim();
    let mut chars = id.chars();
    let first = chars
        .next()
        .ok_or_else(|| io::Error::other("actor id is required"))?;
    let valid = id.chars().count() <= 32
        && first.is_alphanumeric()
        && chars.all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_')
        && !RESERVED.contains(&id.to_lowercase().as_str());
    if !valid {
        return Err(io::Error::other(
            "actor id must be 1-32 letters or numbers, optionally followed by '-' or '_'",
        ));
    }
    Ok(id.to_owned())
}

pub fn visible(group: &GroupDoc) -> impl Iterator<Item = &Actor> {
    group
        .actors
        .iter()
        .filter(|actor| actor.internal_kind.is_none())
}

pub fn find<'a>(group: &'a GroupDoc, actor_id: &str) -> Option<&'a Actor> {
    group.actors.iter().find(|actor| actor.id == actor_id)
}

pub fn unique_available_foreman(group: &GroupDoc) -> Result<&Actor, UniqueForemanError> {
    let matches = visible(group)
        .filter(|actor| {
            actor.enabled && effective_role(group, &actor.id) == Some(ActorRole::Foreman)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [actor] => Ok(actor),
        [] => Err(UniqueForemanError::NotFound),
        _ => Err(UniqueForemanError::NotUnique),
    }
}

pub fn effective_role(group: &GroupDoc, actor_id: &str) -> Option<ActorRole> {
    let actor = find(group, actor_id)?;
    if actor.internal_kind.is_some() {
        return Some(ActorRole::Peer);
    }
    Some(
        visible(group)
            .next()
            .filter(|first| first.id == actor_id)
            .map_or(ActorRole::Peer, |_| ActorRole::Foreman),
    )
}

pub fn add(group: &mut GroupDoc, mut actor: Actor) -> io::Result<Actor> {
    actor.id = validate_actor_id(&actor.id)?;
    if find(group, &actor.id).is_some() {
        return Err(io::Error::other(format!(
            "actor already exists: {}",
            actor.id
        )));
    }
    if actor.internal_kind.is_some()
        && serde_json::to_value(actor.runtime).ok() == Some(Value::String("web_model".into()))
    {
        return Err(io::Error::other(
            "internal actors cannot use web_model runtime",
        ));
    }
    actor.role = None;
    actor.capability_autoload = dedupe(actor.capability_autoload);
    actor.capability_hidden = dedupe(actor.capability_hidden);
    actor.updated_at = utc_now();
    group.actors.push(actor.clone());
    actor.role = effective_role(group, &actor.id);
    Ok(actor)
}

pub fn update(
    group: &mut GroupDoc,
    actor_id: &str,
    patch: &Map<String, Value>,
) -> io::Result<Actor> {
    let index = group
        .actors
        .iter()
        .position(|actor| actor.id == actor_id)
        .ok_or_else(|| io::Error::other(format!("actor not found: {actor_id}")))?;
    let mut value = serde_json::to_value(&group.actors[index]).map_err(io::Error::other)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| io::Error::other("invalid actor"))?;
    for (key, value) in patch {
        if key != "id" && key != "created_at" && key != "role" {
            object.insert(key.clone(), value.clone());
        }
    }
    object.insert("updated_at".into(), Value::String(utc_now()));
    let mut actor: Actor = serde_json::from_value(value).map_err(io::Error::other)?;
    actor.role = None;
    if actor.runtime == cccc_contracts::ActorRuntime::WebModel {
        actor.runner = cccc_contracts::RunnerKind::Headless;
        actor.command.clear();
    }
    group.actors[index] = actor.clone();
    actor.role = effective_role(group, actor_id);
    Ok(actor)
}

pub fn remove(group: &mut GroupDoc, actor_id: &str) -> io::Result<Actor> {
    let index = group
        .actors
        .iter()
        .position(|actor| actor.id == actor_id)
        .ok_or_else(|| io::Error::other(format!("actor not found: {actor_id}")))?;
    Ok(group.actors.remove(index))
}

pub fn reorder(group: &mut GroupDoc, ids: &[String]) -> io::Result<Vec<Actor>> {
    let visible_ids: BTreeSet<_> = visible(group).map(|actor| actor.id.clone()).collect();
    let requested: BTreeSet<_> = ids.iter().cloned().collect();
    if visible_ids != requested || requested.len() != ids.len() {
        return Err(io::Error::other(
            "actor_ids must include every visible actor exactly once",
        ));
    }
    let mut by_id: BTreeMap<_, _> = group
        .actors
        .drain(..)
        .map(|actor| (actor.id.clone(), actor))
        .collect();
    let mut ordered: Vec<_> = ids.iter().filter_map(|id| by_id.remove(id)).collect();
    ordered.extend(by_id.into_values());
    group.actors.clone_from(&ordered);
    Ok(ordered)
}

pub fn resolve_recipients(group: &GroupDoc, tokens: &[String]) -> io::Result<Vec<String>> {
    let actors: Vec<_> = visible(group).collect();
    let mut output = Vec::new();
    for raw in tokens {
        let mut token = raw.trim();
        if token.starts_with('@') && !matches!(token, "@all" | "@peers" | "@foreman" | "@user") {
            token = token.trim_start_matches('@');
        }
        let canonical = match token {
            "" => continue,
            "@all" | "@peers" | "@foreman" => token.to_owned(),
            "user" | "@user" => "user".into(),
            _ if actors.iter().any(|actor| actor.id == token) => token.into(),
            _ => {
                let matches: Vec<_> = actors
                    .iter()
                    .filter(|actor| actor.title.eq_ignore_ascii_case(token))
                    .collect();
                if matches.len() != 1 {
                    return Err(io::Error::other(format!(
                        "unknown or ambiguous recipient: {token}"
                    )));
                }
                matches[0].id.clone()
            }
        };
        if !output.contains(&canonical) {
            output.push(canonical);
        }
    }
    Ok(output)
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| !value.is_empty() && seen.insert(value.clone()))
        .collect()
}
