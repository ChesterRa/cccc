use super::{running, start_with_private_values, stop};
use crate::dispatch::OpError;
use crate::ops::{actor_profile_runtime, actor_runtime, actor_secrets};
use cccc_contracts::{Actor, ActorRuntime, DEEPSEEK_DEFAULT_MODEL, DEEPSEEK_MODEL_ENV};
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
    let (mut actor, private_env) = resolve_launch_actor(home, group, actor)?;
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
    let private_values = private_env
        .values()
        .cloned()
        .chain(
            private_env
                .keys()
                .filter_map(|key| actor.env.get(key).cloned()),
        )
        .collect();
    start_with_private_values(home, group, &actor, &cwd, private_values).map_err(OpError::io)
}

fn resolve_launch_actor(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> Result<(Actor, BTreeMap<String, String>), OpError> {
    let mut actor = actor_profile_runtime::resolve(home, actor)?;
    let private = actor_secrets::private_values(home, &group.group_id, &actor)?;
    actor.env = actor_secrets::merge_private_values(&actor, private.clone());
    Ok((actor, private))
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
    let model = env
        .get(DEEPSEEK_MODEL_ENV)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(DEEPSEEK_DEFAULT_MODEL)
        .to_owned();
    env.insert(DEEPSEEK_MODEL_ENV.into(), model);
    if let Some(base) = env.get_mut("DEEPSEEK_BASE_URL") {
        *base = base
            .trim()
            .trim_end_matches('/')
            .trim_end_matches("/chat/completions")
            .to_owned();
        if base.is_empty() {
            env.remove("DEEPSEEK_BASE_URL");
        }
    }
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
    use super::{launch_env_from, resolve_launch_actor};
    use cccc_contracts::{Actor, ActorRuntime};
    use cccc_core::{GroupStore, HomeLayout, profiles::ProfileStore};

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
            env.get("DEEPSEEK_MODEL").map(String::as_str),
            Some("deepseek-flash")
        );
        actor
            .env
            .insert("DEEPSEEK_MODEL".into(), " channel/custom-model ".into());
        actor.env.insert(
            "DEEPSEEK_BASE_URL".into(),
            " https://gateway.example/v1/chat/completions/ ".into(),
        );
        let custom = launch_env_from(
            [("DEEPSEEK_MODEL".into(), "daemon-model".into())],
            &home,
            &group,
            &actor,
        );
        assert_eq!(
            custom.get("DEEPSEEK_MODEL").map(String::as_str),
            Some("channel/custom-model")
        );
        assert_eq!(
            custom.get("DEEPSEEK_BASE_URL").map(String::as_str),
            Some("https://gateway.example/v1")
        );
        actor.env.insert("DEEPSEEK_MODEL".into(), "  ".into());
        let defaults = launch_env_from([], &home, &group, &actor);
        assert_eq!(
            defaults.get("DEEPSEEK_MODEL").map(String::as_str),
            Some("deepseek-flash")
        );
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

    #[test]
    fn launch_retains_private_keys_for_custom_and_profile_actors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("deepseek private env", "").expect("group");
        let mut actor = Actor::new("deepseek");
        actor.runtime = ActorRuntime::Deepseek;
        actor.env.insert("PUBLIC_SETTING".into(), "visible".into());
        crate::ops::actor_secrets::replace(
            &home,
            &group.group_id,
            &actor.id,
            [("CUSTOM_TOKEN".into(), "fixture_private_value".into())].into(),
        )
        .expect("custom private env");
        let (custom, private) = resolve_launch_actor(&home, &group, &actor).expect("custom actor");
        assert_eq!(
            private.keys().map(String::as_str).collect::<Vec<_>>(),
            ["CUSTOM_TOKEN"]
        );
        assert_eq!(
            custom.env.get("PUBLIC_SETTING").map(String::as_str),
            Some("visible")
        );
        assert_eq!(
            custom.env.get("CUSTOM_TOKEN").map(String::as_str),
            Some("fixture_private_value")
        );

        let profiles = ProfileStore::new(home.clone()).expect("profiles");
        profiles
            .upsert(
                serde_json::json!({"id":"fixture-profile","runtime":"deepseek"})
                    .as_object()
                    .expect("profile")
                    .clone(),
                None,
            )
            .expect("profile");
        profiles
            .replace_secrets(
                "fixture-profile",
                [("PROFILE_TOKEN".into(), "fixture_profile_value".into())].into(),
            )
            .expect("profile private env");
        actor.profile_id = "fixture-profile".into();
        actor.profile_scope = "global".into();
        let (linked, private) = resolve_launch_actor(&home, &group, &actor).expect("profile actor");
        assert_eq!(
            private.keys().map(String::as_str).collect::<Vec<_>>(),
            ["PROFILE_TOKEN"]
        );
        assert_eq!(
            linked.env.get("PROFILE_TOKEN").map(String::as_str),
            Some("fixture_profile_value")
        );
        assert!(!linked.env.contains_key("CUSTOM_TOKEN"));
        assert!(!linked.env.contains_key("PUBLIC_SETTING"));
    }
}
