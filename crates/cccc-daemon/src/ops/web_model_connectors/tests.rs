use super::*;
use cccc_contracts::{Actor, ActorRuntime};
use serde_json::Value;

fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("valid test fixture").clone(),
    }
}
#[test]
fn pairing_requires_user_stopped_actor_and_confirmation_and_preserves_pending_delivery() {
    for status in [
        "submitting",
        "deferred",
        "pending_new_chat_bind",
        "legacy_recovery_submitting",
        "submission_ambiguous",
        "submission_ambiguous_completion_pending",
        "completion_ambiguous",
        "legacy_submission_unverified",
        "ambiguous",
        "completion_conflict",
    ] {
        let temp = tempfile::tempdir().expect("valid test fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("valid test fixture");
        home.initialize().expect("valid test fixture");
        let groups = GroupStore::new(home.clone()).expect("valid test fixture");
        let mut group = groups.create("pairing", "").expect("valid test fixture");
        let mut actor = Actor::new("a");
        actor.runtime = ActorRuntime::WebModel;
        actor.enabled = true;
        cccc_core::actors::add(&mut group, actor).expect("valid test fixture");
        groups.save(&group).expect("valid test fixture");
        assert!(
            execute(
                &home,
                &request("web_model_connector_configure", json!({"by":"a"}))
            )
            .is_err()
        );
        let configured = execute(
            &home,
            &request("web_model_connector_configure", json!({"by":"user"})),
        )
        .expect("valid test fixture");
        let id = configured["connector"]["connector_id"]
            .as_str()
            .expect("valid test fixture");
        let args = json!({"by":"user","connector_id":id,"group_id":group.group_id,"actor_id":"a"});
        assert!(execute(&home, &request("web_model_pairing_begin", args.clone())).is_err());
        groups
            .mutate(&group.group_id, |g| {
                g.actors[0].enabled = false;
                Ok(())
            })
            .expect("valid test fixture");
        integration_state::group_update(&groups,&group.group_id,"web_model_browser_targets", |v| { *v=json!({"a":{"url":"https://chatgpt.com/c/saved","last_delivery_status":status,"last_delivery_id":"receipt-1"}}); Ok(()) }).expect("valid test fixture");
        let pair = execute(&home, &request("web_model_pairing_begin", args.clone()))
            .expect("valid test fixture");
        execute(
            &home,
            &request(
                "web_model_pairing_accept",
                json!({"by":"user","connector_id":id,"code":pair["code"],"session_key":"host-key"}),
            ),
        )
        .expect("valid test fixture");
        assert!(
            store::binding_for_session(
                &store::load(&home).expect("valid test fixture")[0],
                "host-key"
            )
            .is_none()
        );
        let mut confirm = args.clone();
        confirm["pairing_id"] = pair["pairing_id"].clone();
        confirm["url"] = json!("https://chatgpt.com/c/other");
        assert!(
            execute(
                &home,
                &request("web_model_pairing_confirm", confirm.clone())
            )
            .is_err()
        );
        confirm["url"] = json!("https://chatgpt.com/c/saved");
        execute(&home, &request("web_model_pairing_confirm", confirm)).expect("valid test fixture");
        assert!(
            store::binding_for_session(
                &store::load(&home).expect("valid test fixture")[0],
                "host-key"
            )
            .is_some()
        );
        assert_eq!(
            integration_state::group_get(&groups, &group.group_id, "web_model_browser_targets")
                .expect("valid test fixture")["a"]["last_delivery_id"],
            "receipt-1"
        );
        assert!(execute(&home, &request("web_model_binding_remove", args)).is_err());
    }
}

#[test]
fn automatic_pairing_requires_running_group_and_never_replaces_existing_binding() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("automatic", " ").expect("group");
    let mut actor = Actor::new("a");
    actor.runtime = ActorRuntime::WebModel;
    actor.enabled = true;
    cccc_core::actors::add(&mut group, actor).expect("actor");
    groups.save(&group).expect("save");
    let configured = store::configure(&home).expect("configure");
    let id = &configured["connector"]["connector_id"];
    let args = json!({"by":"user","connector_id":id,"group_id":group.group_id,"actor_id":"a","automatic":true});
    assert!(execute(&home, &request("web_model_pairing_begin", args.clone())).is_err());
    groups
        .mutate(&group.group_id, |g| {
            g.running = true;
            g.state = GroupState::Active;
            Ok(())
        })
        .expect("start group");
    let p =
        execute(&home, &request("web_model_pairing_begin", args.clone())).expect("automatic begin");
    execute(
        &home,
        &request(
            "web_model_pairing_accept",
            json!({"by":"user","connector_id":id,"code":p["code"],"session_key":"host"}),
        ),
    )
    .expect("accept");
    let mut confirm = args.clone();
    confirm["pairing_id"] = p["pairing_id"].clone();
    confirm["url"] = json!("https://chatgpt.com/c/a");
    let first = execute(
        &home,
        &request("web_model_pairing_confirm", confirm.clone()),
    )
    .expect("confirm running actor");
    assert_eq!(
        first,
        execute(&home, &request("web_model_pairing_confirm", confirm)).expect("idempotent confirm")
    );
    assert!(
        execute(&home, &request("web_model_pairing_begin", args.clone())).is_err(),
        "auto cannot replace bound conversation"
    );
    store::retire_actor(&home, &group.group_id, "a").expect("retire");
    groups
        .mutate(&group.group_id, |g| {
            g.state = GroupState::Paused;
            Ok(())
        })
        .expect("pause");
    assert!(execute(&home, &request("web_model_pairing_begin", args)).is_err());
}

#[test]
fn legacy_actor_pairing_keeps_the_creation_identity() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("legacy pairing", "").expect("group");
    let mut actor = Actor::new("a");
    actor.runtime = ActorRuntime::WebModel;
    actor.enabled = true;
    let created_at = actor.created_at.clone();
    group.actors.push(actor);
    group.running = true;
    group.state = GroupState::Active;
    groups.save(&group).expect("save legacy actor");
    let connector = store::configure(&home).expect("connector");
    let id = &connector["connector"]["connector_id"];
    let pair = execute(
        &home,
        &request(
            "web_model_pairing_begin",
            json!({"by":"user","connector_id":id,"group_id":group.group_id,"actor_id":"a","automatic":true}),
        ),
    )
    .expect("start pairing");
    let persisted = groups.load(&group.group_id).expect("reload");
    assert!(
        persisted.actors[0].generation.is_empty(),
        "pairing must not replace a legacy Actor's identity after its browser opens"
    );
    let saved = store::load(&home).expect("store");
    let attempt = store::pairing_for_actor(&saved[0], &group.group_id, "a").expect("attempt");
    assert_eq!(attempt["pairing_id"], pair["pairing_id"]);
    assert_eq!(attempt["generation"], format!("legacy:{created_at}"));

    store::accept_pairing(
        &home,
        id.as_str().expect("connector ID"),
        pair["code"].as_str().expect("code"),
        "host-session",
    )
    .expect("accept");
    execute(
        &home,
        &request(
            "web_model_pairing_confirm",
            json!({"by":"user","connector_id":id,"group_id":group.group_id,"actor_id":"a",
                "pairing_id":pair["pairing_id"],"url":"https://chatgpt.com/c/legacy"}),
        ),
    )
    .expect("confirm");
    let saved = store::load(&home).expect("reload connector");
    let mut binding = store::binding_for_session(&saved[0], "host-session").expect("binding");
    binding["connector_id"] = id.clone();
    store::validate_binding(&home, &binding).expect("legacy Actor remains authorized");
}

#[test]
fn stopping_a_cli_actor_does_not_depend_on_the_web_connector_store() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("CLI", "").expect("group");
    let actor = Actor::new("cli");
    assert_ne!(actor.runtime, ActorRuntime::WebModel);
    group.actors.push(actor);
    std::fs::write(
        home.root().join("web_model_connectors.yaml"),
        "version: 999\n",
    )
    .expect("unreadable Web connector");
    assert!(super::super::actor_runtime::apply(&home, &group, "cli", "actor.stop").is_ok());
}

#[test]
fn grok_binding_requires_stopped_actor_preserves_pending_work_and_retires_on_runtime_change() {
    let temp = tempfile::tempdir().expect("valid test fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("valid test fixture");
    home.initialize().expect("valid test fixture");
    let groups = GroupStore::new(home.clone()).expect("valid test fixture");
    let mut group = groups.create("Grok", "").expect("valid test fixture");
    let mut actor = Actor::new("bot");
    actor.runtime = ActorRuntime::GrokWebModel;
    actor.enabled = true;
    cccc_core::actors::add(&mut group, actor).expect("valid test fixture");
    groups.save(&group).expect("valid test fixture");
    let chatgpt = store::configure(&home).expect("valid test fixture");
    let configured = execute(
        &home,
        &request(
            "web_model_connector_configure",
            json!({"by":"user","provider":"grok_web"}),
        ),
    )
    .expect("valid test fixture");
    assert!(
        execute(
            &home,
            &request(
                "web_model_connector_configure",
                json!({"by":"user","provider":false})
            )
        )
        .is_err()
    );
    let id = &configured["connector"]["connector_id"];
    let mut args = json!({"by":"user","connector_id":id,"group_id":group.group_id,"actor_id":"bot","url":"https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a"});
    assert!(execute(&home, &request("web_model_grok_bind", args.clone())).is_err());
    groups
        .mutate(&group.group_id, |g| {
            g.actors[0].enabled = false;
            Ok(())
        })
        .expect("valid test fixture");
    execute(&home, &request("web_model_grok_bind", args.clone())).expect("valid test fixture");
    let connector = || {
        store::load(&home)
            .expect("valid test fixture")
            .into_iter()
            .find(|c| &c["connector_id"] == id)
            .expect("valid test fixture")
    };
    let token = store::grok_token(
        &connector(),
        &connector()["bindings"]
            .as_object()
            .expect("valid test fixture")
            .values()
            .next()
            .expect("valid test fixture")
            .clone(),
    )
    .expect("valid test fixture");
    for status in ["submission_ambiguous", "ambiguous", "completion_conflict"] {
        integration_state::group_update(&groups, &group.group_id, "web_model_browser_targets", |v| {
            *v = json!({"bot":{"url":args["url"],"last_delivery_status":status,"last_delivery_id":"pending"}}); Ok(())
        }).expect("valid test fixture");
        execute(&home, &request("web_model_grok_bind", args.clone())).expect("valid test fixture");
        assert!(store::binding_for_token(&connector(), &token).is_some());
        assert!(execute(&home, &request("web_model_binding_remove", args.clone())).is_err());
        let mut changed = args.clone();
        changed["url"] = json!("https://grok.com/bot/958f2446-013f-4225-8dd3-f146295992d7");
        assert!(execute(&home, &request("web_model_grok_bind", changed)).is_err());
        assert_eq!(
            integration_state::group_get(&groups, &group.group_id, "web_model_browser_targets")
                .expect("valid test fixture")["bot"]["last_delivery_id"],
            "pending"
        );
    }
    integration_state::group_update(&groups, &group.group_id, "web_model_browser_targets", |v| {
        *v = json!({});
        Ok(())
    })
    .expect("valid test fixture");
    for runtime in ["web_model", "grok_web_model"] {
        let r = request(
            "actor_update",
            json!({"by":"user","group_id":group.group_id,"actor_id":"bot","patch":{"runtime":runtime}}),
        );
        super::super::actors::resolve_operation(&r)
            .expect("valid test fixture")
            .execute(&home, &r)
            .expect("valid test fixture");
    }
    assert!(
        store::binding_for_token(&connector(), &token).is_none(),
        "switching back cannot resurrect a retired credential"
    );
    args["connector_id"] = chatgpt["connector"]["connector_id"].clone();
    assert!(execute(&home, &request("web_model_grok_bind", args)).is_err());
    assert_eq!(
        store::load(&home)
            .expect("valid test fixture")
            .into_iter()
            .find(|c| c["provider"] == "chatgpt_web")
            .expect("valid test fixture")["secret_hash"],
        chatgpt["connector"]["secret_hash"]
    );
}
