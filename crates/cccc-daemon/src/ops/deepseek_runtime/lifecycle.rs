use super::{running, start, stop};
use crate::dispatch::OpError;
use crate::ops::{actor_profile_runtime, actor_runtime, actor_secrets};
use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout};
use std::collections::BTreeMap;

pub fn apply(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    kind: &str,
) -> Result<(), OpError> {
    match kind {
        "actor.stop" => stop(&group.group_id, &actor.id),
        "actor.restart" | "actor.new_session" => {
            stop(&group.group_id, &actor.id);
            start_actor(home, group, actor)?;
        }
        _ if !running(&group.group_id, &actor.id) => start_actor(home, group, actor)?,
        _ => {}
    }
    Ok(())
}

fn start_actor(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> Result<(), OpError> {
    let mut actor = resolve_launch_actor(home, group, actor)?;
    actor.normalize_runtime_constraints();
    if actor.command.is_empty() {
        actor.command = cccc_runtime::default_command(actor.runtime);
    }
    actor.env = launch_env(home, group, &actor);
    let executable = crate::ops::codex_mcp::resolve_cccc_executable()
        .ok_or_else(|| setup_required("CCCC executable is not available for DeepSeek setup"))?;
    crate::deepseek_setup::ensure(home, &mut actor.env, &executable).map_err(setup_required)?;
    let session_root = actor
        .env
        .get("CCCC_DEEPSEEK_SESSION_ROOT")
        .ok_or_else(|| setup_required("DeepSeek session root is not configured"))?;
    std::fs::create_dir_all(session_root).map_err(OpError::io)?;
    preflight(&actor)?;
    let cwd = actor_runtime::working_directory(group, &actor)?;
    start(home, group, &actor, &cwd).map_err(OpError::io)
}

fn resolve_launch_actor(
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

fn launch_env(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> BTreeMap<String, String> {
    launch_env_from(std::env::vars(), home, group, actor)
}

fn launch_env_from(
    base: impl IntoIterator<Item = (String, String)>,
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> BTreeMap<String, String> {
    let mut env = base.into_iter().collect::<BTreeMap<_, _>>();
    env.extend(actor.env.clone());
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    env.insert("CCCC_GROUP_ID".into(), group.group_id.clone());
    env.insert("CCCC_ACTOR_ID".into(), actor.id.clone());
    env.insert(
        "CCCC_DEEPSEEK_SESSION_ROOT".into(),
        home.root()
            .join("groups")
            .join(&group.group_id)
            .join("state")
            .join("deepseek")
            .join(&actor.id)
            .join("sessions")
            .to_string_lossy()
            .into_owned(),
    );
    env
}

fn preflight(actor: &Actor) -> Result<(), OpError> {
    debug_assert_eq!(actor.runtime, ActorRuntime::Deepseek);
    cccc_runtime::deepseek_preflight(&actor.command, &actor.env).map_err(setup_required)
}

fn setup_required(message: impl Into<String>) -> OpError {
    let mut error = OpError::new("setup_required", message);
    error
        .details
        .insert("runtime".into(), serde_json::json!("deepseek"));
    error
        .details
        .insert("runnable".into(), serde_json::json!(false));
    error
}

#[cfg(test)]
mod tests {
    use super::launch_env_from;
    use cccc_contracts::{Actor, ActorRuntime};
    use cccc_core::{GroupStore, HomeLayout};

    #[test]
    fn launch_environment_inherits_daemon_values_and_applies_actor_overrides() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("deepseek env", "").expect("group");
        let mut actor = Actor::new("deepseek");
        actor.runtime = ActorRuntime::Deepseek;
        actor.env.insert("PATH".into(), "/actor/bin".into());
        let env = launch_env_from(
            [
                ("PATH".into(), "/daemon/bin".into()),
                ("DSH_HOME".into(), "/daemon/dsh".into()),
                ("HOME".into(), "/daemon/home".into()),
            ],
            &home,
            &group,
            &actor,
        );
        assert_eq!(env.get("PATH").map(String::as_str), Some("/actor/bin"));
        assert_eq!(env.get("DSH_HOME").map(String::as_str), Some("/daemon/dsh"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/daemon/home"));
        assert_eq!(
            env.get("CCCC_ACTOR_ID").map(String::as_str),
            Some("deepseek")
        );
        assert_eq!(
            env.get("CCCC_DEEPSEEK_SESSION_ROOT").map(String::as_str),
            Some(
                home.root()
                    .join("groups")
                    .join(&group.group_id)
                    .join("state/deepseek/deepseek/sessions")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }
}
