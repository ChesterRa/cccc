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
            "runner":"headless"
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
    assert_eq!(actor["runner"], "headless");
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
