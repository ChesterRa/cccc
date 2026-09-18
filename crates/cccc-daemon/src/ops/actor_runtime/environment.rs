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
    actor.env = actor_secrets::effective_values(home, &group.group_id, &actor)?;
    Ok(actor)
}

pub(super) fn launch_env(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> BTreeMap<String, String> {
    let mut env = actor.env.clone();
    // Packaged/test installations need the same CLI as their owning daemon,
    // even when that executable is not installed on the user's shell PATH.
    crate::ops::codex_mcp::configure_actor_cli(&mut env);
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), group.group_id.clone());
    env.insert("CCCC_ACTOR_ID".into(), actor.id.clone());
    env
}
