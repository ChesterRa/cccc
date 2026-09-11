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
    // apply 已按本次启动解析运行配置；不能在加锁/启动过程中再次切换快照。
    let mut actor = actor.clone();
    let profile_secrets = actor_profile_runtime::profile_secrets(home, &actor)?;
    let actor_secret_values = actor_secrets::values(home, &group.group_id, &actor.id)?;
    actor.env.extend(profile_secrets);
    actor.env.extend(actor_secret_values);
    let default = cccc_runtime::default_command(actor.runtime);
    cccc_core::cli_management::apply_command(
        home,
        cccc_core::runtime_mcp::name(actor.runtime),
        &default,
        &mut actor.command,
        &mut actor.env,
    )
    .map_err(OpError::io)?;
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

#[cfg(all(test, unix))]
#[path = "environment_tests.rs"]
mod tests;

#[cfg(all(test, windows))]
#[path = "environment_windows_tests.rs"]
mod windows_tests;
