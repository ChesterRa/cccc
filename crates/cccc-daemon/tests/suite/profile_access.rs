use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn user_profiles_are_isolated_for_list_get_upsert_secrets_and_delete() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"a-profile","scope":"user","owner_id":"user-a","name":"A"}),
    );
    call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"b-profile","scope":"user","owner_id":"user-b","name":"B"}),
    );
    call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"global-profile","scope":"global","name":"Global"}),
    );
    let group_a = call(&home, "group_create", json!({"title":"A"})).result["group"]["group_id"]
        .as_str()
        .expect("group A")
        .to_owned();
    let group_b = call(&home, "group_create", json!({"title":"B"})).result["group"]["group_id"]
        .as_str()
        .expect("group B")
        .to_owned();
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_b,
            "actor_id":"secret-source",
            "env_private":{"TOKEN":"secret"},
            "by":"user"
        }),
    );

    let b_list = call(
        &home,
        "actor_profile_list",
        json!({"view":"accessible","caller_id":"user-b","is_admin":false}),
    );
    let ids = b_list.result["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .filter_map(|profile| profile["id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["b-profile", "global-profile"]);

    for (op, args) in [
        (
            "actor_profile_get",
            json!({"profile_id":"a-profile","profile_scope":"user","profile_owner":"user-a"}),
        ),
        (
            "actor_profile_secret_keys",
            json!({"profile_id":"a-profile","profile_scope":"user","profile_owner":"user-a"}),
        ),
        (
            "actor_profile_delete",
            json!({"profile_id":"a-profile","profile_scope":"user","profile_owner":"user-a"}),
        ),
    ] {
        let mut args = args.as_object().cloned().expect("args");
        args.insert("caller_id".into(), json!("user-b"));
        args.insert("is_admin".into(), json!(false));
        assert_denied(raw_call(&home, op, Value::Object(args)));
    }

    let same_id_other_owner = call(
        &home,
        "actor_profile_upsert",
        json!({
            "profile_id":"a-profile",
            "scope":"user",
            "owner_id":"user-b",
            "name":"same id, different owner",
            "caller_id":"user-b",
            "is_admin":false
        }),
    );
    assert_eq!(same_id_other_owner.result["profile"]["owner_id"], "user-b");
    assert_denied(raw_call(
        &home,
        "actor_profile_copy_actor_secrets",
        json!({
            "profile_id":"a-profile",
            "profile_scope":"user",
            "profile_owner":"user-a",
            "group_id":group_b,
            "actor_id":"secret-source",
            "caller_id":"user-a",
            "is_admin":false,
            "allowed_groups":[group_a]
        }),
    ));
    let empty = call(
        &home,
        "actor_profile_secret_keys",
        json!({
            "profile_id":"a-profile",
            "profile_scope":"user",
            "profile_owner":"user-a"
        }),
    );
    assert_eq!(empty.result["keys"], json!([]));
    let copied = call(
        &home,
        "actor_profile_copy_actor_secrets",
        json!({
            "profile_id":"a-profile",
            "profile_scope":"user",
            "profile_owner":"user-a",
            "group_id":group_b,
            "actor_id":"secret-source"
        }),
    );
    assert_eq!(copied.result["keys"], json!(["TOKEN"]));
    let a = call(
        &home,
        "actor_profile_get",
        json!({
            "profile_id":"a-profile",
            "profile_scope":"user",
            "profile_owner":"user-a"
        }),
    );
    assert_eq!(a.result["profile"]["name"], "A");
    assert_eq!(a.result["profile"]["owner_id"], "user-a");
}

#[test]
fn only_an_admin_can_copy_voice_analyst_secrets_to_a_runtime_profile() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"voice-profile","name":"Voice Profile"}),
    );
    cccc_core::codex_voice_settings::replace_private_environment(
        &home,
        &std::collections::BTreeMap::from([("ZAI_API_KEY".into(), "secret".into())]),
    )
    .expect("Voice Analyst secrets");

    assert_denied(raw_call(
        &home,
        "actor_profile_copy_voice_analyst_secrets",
        json!({"profile_id":"voice-profile","caller_id":"user-a","is_admin":false}),
    ));
    let copied = call(
        &home,
        "actor_profile_copy_voice_analyst_secrets",
        json!({"profile_id":"voice-profile","is_admin":true}),
    );
    assert_eq!(copied.result["keys"], json!(["ZAI_API_KEY"]));
    assert_eq!(
        cccc_core::profiles::ProfileStore::new(home)
            .expect("profiles")
            .secret_values("voice-profile")
            .expect("profile secrets")
            .get("ZAI_API_KEY")
            .map(String::as_str),
        Some("secret")
    );
}

#[test]
fn force_delete_converts_linked_actor_with_profile_secrets_intact() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    call(
        &home,
        "actor_profile_upsert",
        json!({
            "profile_id":"detach-profile",
            "name":"Detach Profile",
            "runtime":"codex",
        }),
    );
    call(
        &home,
        "actor_profile_secret_update",
        json!({"profile_id":"detach-profile","set":{"TOKEN":"secret"}}),
    );
    let group_id =
        call(&home, "group_create", json!({"title":"detach"})).result["group"]["group_id"]
            .as_str()
            .expect("group id")
            .to_owned();
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"linked",
            "profile_id":"detach-profile",
            "enabled":false,
            "by":"user"
        }),
    );

    let rejected = raw_call(
        &home,
        "actor_profile_delete",
        json!({"profile_id":"detach-profile","by":"user"}),
    );
    assert!(!rejected.ok);
    let deleted = call(
        &home,
        "actor_profile_delete",
        json!({"profile_id":"detach-profile","force_detach":true,"by":"user"}),
    );
    assert_eq!(deleted.result["detached_count"], 1);
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    let actor = actors.result["actors"]
        .as_array()
        .expect("actors")
        .iter()
        .find(|actor| actor["id"] == "linked")
        .expect("linked actor");
    assert_eq!(actor["profile_id"], "");
    assert_eq!(actor["profile_scope"], "global");
    assert_eq!(actor["profile_owner"], "");
    assert_eq!(actor["runtime"], "codex");
    assert_eq!(actor["runner"], "pty");
    let keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"linked","by":"user"}),
    );
    assert_eq!(keys.result["keys"], json!(["TOKEN"]));
}

fn assert_denied(response: DaemonResponse) {
    assert!(!response.ok);
    assert!(
        matches!(
            response.error.expect("error").code.as_str(),
            "permission_denied" | "profile_not_found"
        ),
        "unauthorized profile access must fail closed"
    );
}

#[test]
fn legacy_profile_without_runtime_does_not_block_web_model_creation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::fs::write_json(
        &home.root().join("profiles.json"),
        &json!({"profiles":{"legacy":{"id":"legacy","name":"Legacy","env":{}}}}),
    )
    .expect("legacy fixture");
    let gid =
        call(&home, "group_create", json!({"title":"legacy profile"})).result["group"]["group_id"]
            .clone();
    let linked = call(
        &home,
        "actor_add",
        json!({"group_id":gid,"actor_id":"legacy-agent","profile_id":"legacy","by":"user"}),
    );
    assert_eq!(linked.result["actor"]["runtime"], "codex");
    call(
        &home,
        "actor_add",
        json!({"group_id":gid,"actor_id":"web","runtime":"web_model","by":"user"}),
    );
    let duplicate = raw_call(
        &home,
        "actor_add",
        json!({"group_id":gid,"actor_id":"second-web","runtime":"web_model","by":"user"}),
    );
    assert_eq!(
        duplicate.error.expect("second Web Model rejected").code,
        "chatgpt_web_model_singleton"
    );
}

#[test]
fn profile_switching_to_web_model_respects_the_singleton() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_a = call(&home, "group_create", json!({"title":"a"})).result["group"]["group_id"]
        .as_str()
        .expect("group a")
        .to_owned();
    let group_b = call(&home, "group_create", json!({"title":"b"})).result["group"]["group_id"]
        .as_str()
        .expect("group b")
        .to_owned();
    let upsert = |runtime: &str| {
        raw_call(
            &home,
            "actor_profile_upsert",
            json!({"profile_id":"shared","name":"Shared","runtime":runtime}),
        )
    };
    assert!(upsert("codex").ok);
    // An unlinked profile may switch freely: no actor runs with it yet.
    assert!(upsert("web_model").ok);
    assert!(upsert("codex").ok);
    for (group, actor) in [(&group_a, "one"), (&group_b, "two")] {
        call(
            &home,
            "actor_add",
            json!({"group_id":group,"actor_id":actor,"profile_id":"shared","by":"user"}),
        );
    }
    let two_linked = upsert("web_model");
    assert_eq!(
        two_linked.error.expect("two linked actors").code,
        "chatgpt_web_model_singleton"
    );
    call(
        &home,
        "actor_remove",
        json!({"group_id":group_b,"actor_id":"two","by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":group_b,"actor_id":"web","runtime":"web_model","by":"user"}),
    );
    let slot_taken = upsert("web_model");
    assert_eq!(
        slot_taken.error.expect("slot already owned").code,
        "chatgpt_web_model_singleton"
    );
    call(
        &home,
        "actor_remove",
        json!({"group_id":group_b,"actor_id":"web","by":"user"}),
    );
    assert!(upsert("web_model").ok, "one linked actor and a free slot");
    let duplicate = raw_call(
        &home,
        "actor_add",
        json!({"group_id":group_b,"actor_id":"second-web","runtime":"web_model","by":"user"}),
    );
    assert_eq!(
        duplicate
            .error
            .expect("profile runtime owns the slot even before restart")
            .code,
        "chatgpt_web_model_singleton"
    );
}

#[test]
fn linked_actor_edits_and_snapshots_follow_profile_ownership() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let gid = call(
        &home,
        "group_create",
        json!({"title":"profile transitions"}),
    )
    .result["group"]["group_id"]
        .clone();
    for id in ["source", "target", "copy"] {
        call(
            &home,
            "actor_profile_upsert",
            json!({"profile_id":id,"name":id,"runtime":"codex"}),
        );
    }
    call(
        &home,
        "actor_profile_secret_update",
        json!({"profile_id":"target","set":{"PROFILE_ONLY":"synthetic","SHARED":"profile"}}),
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":gid,"actor_id":"agent","enabled":false,
        "env_private":{"OLD_ONLY":"synthetic","SHARED":"old"},"by":"user"}),
    );
    call(
        &home,
        "actor_update",
        json!({"group_id":gid,"actor_id":"agent","profile_id":"source"}),
    );
    let renamed = call(
        &home,
        "actor_update",
        json!({"group_id":gid,"actor_id":"agent",
        "title":"Renamed","capability_autoload":[]}),
    );
    assert_eq!(renamed.result["actor"]["title"], "Renamed");
    let linked = call(
        &home,
        "actor_update",
        json!({"group_id":gid,"actor_id":"agent",
        "profile_id":"target","capability_autoload":[]}),
    );
    assert_eq!(linked.result["actor"]["profile_id"], "target");
    let denied = raw_call(
        &home,
        "actor_update",
        json!({"group_id":gid,"actor_id":"agent","runtime":"custom"}),
    );
    assert_eq!(
        denied.error.expect("readonly").code,
        "actor_profile_linked_readonly"
    );
    call(
        &home,
        "actor_profile_copy_actor_secrets",
        json!({"profile_id":"copy","group_id":gid,"actor_id":"agent"}),
    );
    let profiles = cccc_core::profiles::ProfileStore::new(home.clone()).expect("profiles");
    let expected = profiles.secret_values("target").expect("target secrets");
    assert_eq!(
        profiles.secret_values("copy").expect("copied secrets"),
        expected
    );
    let converted = call(
        &home,
        "actor_update",
        json!({"group_id":gid,"actor_id":"agent",
        "profile_action":"convert_to_custom","capability_autoload":[]}),
    );
    assert_eq!(converted.result["actor"]["profile_id"], "");
    call(
        &home,
        "actor_profile_copy_actor_secrets",
        json!({"profile_id":"copy","group_id":gid,"actor_id":"agent"}),
    );
    assert_eq!(
        profiles.secret_values("copy").expect("snapshot secrets"),
        expected
    );
    let keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":gid,"actor_id":"agent"}),
    );
    assert_eq!(keys.result["keys"], json!(["PROFILE_ONLY", "SHARED"]));
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn raw_call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
