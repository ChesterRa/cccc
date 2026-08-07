// Included by the crate-level integration test harness.
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout, Registry, group_scope, ledger, scope, settings};
use serde_json::{Map, Value, json};

#[test]
fn remote_access_requires_secure_configuration_and_token() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    let initial = ok(&home, "remote_access_state", json!({}));
    assert_eq!(initial.result["remote_access"]["provider"], "off");

    let insecure = raw(
        &home,
        "remote_access_configure",
        json!({"provider":"manual","web_public_url":"https://public.example","require_access_token":false,"by":"user"}),
    );
    assert!(!insecure.ok);
    assert_eq!(
        insecure.error.expect("error").code,
        "remote_access_invalid_config"
    );

    ok(
        &home,
        "remote_access_configure",
        json!({"provider":"manual","web_host":"0.0.0.0","web_port":9000,"require_access_token":true,"by":"user"}),
    );
    let missing = raw(&home, "remote_access_start", json!({"by":"user"}));
    assert_eq!(
        missing.error.expect("missing admin error").code,
        "remote_access_admin_token_required"
    );
    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("scoped", vec!["g_test".into()], false, None)
        .expect("scoped token");
    let scoped = raw(&home, "remote_access_start", json!({"by":"user"}));
    assert_eq!(
        scoped.error.expect("scoped token error").code,
        "remote_access_admin_token_required"
    );
    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let started = ok(&home, "remote_access_start", json!({"by":"user"}));
    assert_eq!(started.result["remote_access"]["status"], "running");
    assert_eq!(
        started.result["remote_access"]["endpoint"],
        "http://0.0.0.0:9000"
    );
    let stopped = ok(&home, "remote_access_stop", json!({"by":"user"}));
    assert_eq!(stopped.result["remote_access"]["enabled"], false);
}

#[test]
fn remote_access_mode_matches_python_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");

    let initial = ok(&home, "remote_access_state", json!({}));
    assert_eq!(initial.result["remote_access"]["mode"], "tailnet_only");

    let configured = ok(
        &home,
        "remote_access_configure",
        json!({"mode":"tailnet_only","by":"user"}),
    );
    assert_eq!(configured.result["remote_access"]["mode"], "tailnet_only");

    let legacy = ok(
        &home,
        "remote_access_configure",
        json!({"mode":"team","by":"user"}),
    );
    assert_eq!(legacy.result["remote_access"]["mode"], "tailnet_only");

    let unsupported = raw(
        &home,
        "remote_access_configure",
        json!({"mode":"public","by":"user"}),
    );
    assert!(!unsupported.ok);
    assert_eq!(
        unsupported.error.expect("error").code,
        "remote_access_invalid_config"
    );
}

#[test]
fn remote_access_start_rejects_loopback_binding() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    ok(
        &home,
        "remote_access_configure",
        json!({"provider":"manual","web_host":"127.0.0.1","by":"user"}),
    );
    let start = raw(&home, "remote_access_start", json!({"by":"user"}));
    assert_eq!(
        start.error.expect("unreachable error").code,
        "remote_access_unreachable"
    );
}

#[test]
fn legacy_remote_access_mode_is_migrated_before_stop() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    let mut global = settings::load(&home).expect("settings");
    global
        .remote_access
        .insert("provider".into(), json!("manual"));
    global.remote_access.insert("mode".into(), json!("serve"));
    global.remote_access.insert("enabled".into(), json!(true));
    settings::save(&home, &global).expect("save legacy settings");

    let stopped = ok(&home, "remote_access_stop", json!({"by":"user"}));
    assert_eq!(stopped.result["remote_access"]["mode"], "tailnet_only");
    assert_eq!(stopped.result["remote_access"]["enabled"], false);
    assert_eq!(
        settings::load(&home)
            .expect("migrated settings")
            .remote_access["mode"],
        "tailnet_only"
    );
}

#[test]
fn diagnostics_are_developer_gated_and_logs_are_bounded() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    assert!(!raw(&home, "debug_snapshot", json!({})).ok);
    let mut global = settings::load(&home).expect("settings");
    global
        .observability
        .insert("developer_mode".into(), Value::Bool(true));
    settings::save(&home, &global).expect("save");
    std::fs::write(home.daemon_dir().join("ccccd.log"), "one\ntwo\nthree\n").expect("log");
    let tail = ok(
        &home,
        "debug_tail_logs",
        json!({"component":"daemon","lines":2}),
    );
    assert_eq!(tail.result["lines"], json!(["two", "three"]));
    ok(
        &home,
        "debug_clear_logs",
        json!({"component":"daemon","by":"user"}),
    );
    assert!(
        std::fs::read_to_string(home.daemon_dir().join("ccccd.log"))
            .expect("log")
            .is_empty()
    );
}

#[test]
fn global_settings_reject_non_user_writes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    let denied = raw(
        &home,
        "branding_update",
        json!({"by":"peer1","patch":{"product_name":"Denied"}}),
    );
    assert!(!denied.ok);
    assert_eq!(denied.error.expect("error").code, "permission_denied");
}

#[test]
fn group_reset_appends_a_creation_event_for_the_replacement_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"reset lifecycle"}));
    let old_group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("old group id")
        .to_owned();
    let store = GroupStore::new(home.clone()).expect("store");
    store
        .mutate(&old_group_id, |group| {
            group.automation.insert("version".into(), json!(7));
            Ok(())
        })
        .expect("automation");
    ok(
        &home,
        "actor_add",
        json!({
            "group_id":old_group_id,
            "actor_id":"reset-peer",
            "env_private":{"TOKEN":"secret"},
            "by":"user"
        }),
    );
    store
        .mutate(&old_group_id, |group| {
            group.actors[0].avatar_asset_path = "state/blobs/old-avatar.png".into();
            Ok(())
        })
        .expect("avatar path");
    let project = temp.path().join("reset-project");
    std::fs::create_dir(&project).expect("project");
    let attached_scope = scope::detect(&project).expect("scope");
    group_scope::attach(&store, &old_group_id, attached_scope.clone()).expect("attach scope");
    cccc_core::active::set(&home, &old_group_id).expect("active");
    let missing_confirm = raw(
        &home,
        "group_reset",
        json!({"group_id":old_group_id,"by":"user"}),
    );
    assert_eq!(
        missing_confirm.error.expect("confirm error").code,
        "invalid_args"
    );
    assert!(store.load(&old_group_id).is_ok());
    let reset = ok(
        &home,
        "group_reset",
        json!({"group_id":old_group_id,"confirm":old_group_id,"by":"user"}),
    );
    let new_group_id = reset.result["new_group_id"].as_str().expect("new group id");
    let events = ledger::tail(&store.ledger_path(new_group_id).expect("ledger"), 1).expect("tail");

    assert_eq!(reset.result["group_id"], new_group_id);
    assert_eq!(reset.result["deleted_old"], true);
    assert!(store.load(&old_group_id).is_err());
    let replacement = store.load(new_group_id).expect("replacement");
    assert_eq!(replacement.automation["version"], 7);
    assert_eq!(replacement.actors[0].id, "reset-peer");
    assert!(replacement.actors[0].avatar_asset_path.is_empty());
    assert_eq!(
        Registry::load(&home).expect("registry").defaults[&attached_scope.scope_key],
        new_group_id
    );
    assert_eq!(
        ok(
            &home,
            "actor_env_private_keys",
            json!({"group_id":new_group_id,"actor_id":"reset-peer"})
        )
        .result["keys"],
        json!(["TOKEN"])
    );
    assert_eq!(
        cccc_core::active::get(&home).expect("active").as_deref(),
        Some(new_group_id)
    );
    assert_eq!(events[0].kind, "group.create");
    assert_eq!(events[0].data["reset_from"], old_group_id);
}

fn ok(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn raw(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
