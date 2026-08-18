use crate::dispatch::OpError;
use crate::ops::{actor_profile_runtime, actor_secrets};
use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};
use std::collections::BTreeMap;

pub(super) fn resolve_launch_actor(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> Result<Actor, OpError> {
    let mut actor = actor_profile_runtime::resolve(home, actor)?;
    let profile_secrets = actor_profile_runtime::profile_secrets(home, &actor)?;
    let actor_secret_values = actor_secrets::values(home, &group.group_id, &actor.id)?;
    actor.env.extend(profile_secrets);
    actor.env.extend(actor_secret_values);
    Ok(actor)
}

pub(super) fn launch_env(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> BTreeMap<String, String> {
    let mut env = actor.env.clone();
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), group.group_id.clone());
    env.insert("CCCC_ACTOR_ID".into(), actor.id.clone());
    env
}
