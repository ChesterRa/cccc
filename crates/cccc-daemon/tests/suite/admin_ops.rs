// Included by the crate-level integration test harness.
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{
    GroupStore, HomeLayout, Registry, group_scope, ledger, membership, scope, settings,
};
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn membership_account_server(responses: Vec<(u16, &'static str)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind account fixture");
    let address = listener.local_addr().expect("account address");
    let origin = format!("http://{address}");
    let response_origin = origin.clone();
    thread::spawn(move || {
        for (status, body) in responses {
            let body = body.replace("$ORIGIN", &response_origin);
            let (mut stream, _) = listener.accept().expect("account request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let reason = if status == 200 { "OK" } else { "Bad Request" };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("account response");
        }
    });
    origin
}

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
        json!({"provider":"manual","web_public_url":"https://public.example","web_port":9000,"require_access_token":true,"by":"user"}),
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
        "https://public.example"
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
fn membership_verbs_fail_closed_when_the_account_plane_rejects_login() {
    let origin = membership_account_server(vec![(500, r#"{"error":{"message":"unavailable"}}"#)]);
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");

    let status = ok(&home, "membership_status", json!({"by":"user"}));
    assert_eq!(status.result["membership"]["logged_in"], false);

    let login = raw(
        &home,
        "membership_login",
        json!({"by":"user","account_origin":origin}),
    );
    assert_eq!(login.error.expect("login error").code, "membership_network");

    let missing_token = raw(&home, "membership_reach_on", json!({"by":"user"}));
    assert_eq!(missing_token.error.expect("gate").code, "membership_gate");

    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let missing_login = raw(&home, "membership_reach_on", json!({"by":"user"}));
    assert_eq!(
        missing_login.error.expect("not logged in").code,
        "membership_not_logged_in"
    );

    let rejected = raw(
        &home,
        "remote_access_configure",
        json!({"provider":"reach","by":"user"}),
    );
    assert_eq!(
        rejected.error.expect("configure").code,
        "remote_access_invalid_config"
    );

    let logout = ok(&home, "membership_logout", json!({"by":"user"}));
    assert_eq!(
        logout.result["membership"]["warning"],
        membership::LOGOUT_WARNING
    );
}

#[test]
fn membership_status_is_user_only_and_rust_login_reuses_python_credentials() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    std::fs::create_dir_all(membership::path(&home).parent().expect("parent")).expect("secrets");
    std::fs::write(
        membership::path(&home),
        serde_json::to_vec_pretty(&json!({
            "logged_in": true,
            "device_id": "d-python",
            "device_token": "device-secret",
            "hostname": "https://d-python.example.test",
            "tunnel_token": "tunnel-secret",
            "disabled": false,
            "last_error": null,
            "pending_login": {"device_code":"pending-secret","interval":120}
        }))
        .expect("json"),
    )
    .expect("membership");

    let denied = raw(&home, "membership_status", json!({"by":"peer1"}));
    assert_eq!(denied.error.expect("denied").code, "permission_denied");

    let login = raw(
        &home,
        "membership_login",
        json!({"by":"user","account_origin":"http://127.0.0.1:1"}),
    );
    assert!(
        login.ok,
        "existing membership should be reused: {:?}",
        login.error
    );
    let saved: Value =
        serde_json::from_slice(&std::fs::read(membership::path(&home)).expect("saved membership"))
            .expect("saved json");
    assert_eq!(saved["device_token"], "device-secret");
    assert_eq!(saved["tunnel_token"], "tunnel-secret");
    assert_eq!(saved["pending_login"]["device_code"], "pending-secret");
}

#[test]
fn rust_membership_login_and_poll_complete_the_device_flow() {
    let origin = membership_account_server(vec![
        (
            200,
            r#"{"device_code":"dc-rust","user_code":"RUST-CODE","verification_uri":"$ORIGIN/device","expires_in":600,"interval":120}"#,
        ),
        (
            200,
            r#"{"access_token":"device-token","device_id":"device-rust","hostname":"https://device-rust.example.test"}"#,
        ),
    ]);
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    let started = ok(
        &home,
        "membership_login",
        json!({"by":"user","account_origin":origin}),
    );
    assert_eq!(
        started.result["membership"]["pending"]["user_code"],
        "RUST-CODE"
    );
    assert_eq!(started.result["membership"]["pending"]["interval"], 120);
    let granted = ok(
        &home,
        "membership_login_poll",
        json!({"by":"user","account_origin":origin}),
    );
    assert_eq!(granted.result["membership"]["logged_in"], true);
    assert_eq!(granted.result["membership"]["device_id"], "device-rust");
    let stored = membership::load(&home).expect("membership state");
    assert_eq!(stored.device_token.as_deref(), Some("device-token"));
    assert_eq!(stored.account_origin.as_deref(), Some(origin.as_str()));
    assert!(stored.pending_login.is_none());
}

#[test]
fn rust_membership_status_applies_a_remote_cut() {
    let origin = membership_account_server(vec![(
        403,
        r#"{"error":{"code":"disabled","message":"device disabled"}}"#,
    )]);
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    membership::save(
        &home,
        &membership::MembershipState {
            logged_in: true,
            account_origin: Some(origin.clone()),
            device_id: Some("device-rust".into()),
            device_token: Some("device-token".into()),
            hostname: Some("https://device-rust.example.test".into()),
            ..membership::MembershipState::default()
        },
    )
    .expect("membership");
    settings::update(&home, |global| {
        global
            .remote_access
            .insert("provider".into(), Value::String("reach".into()));
        global
            .remote_access
            .insert("enabled".into(), Value::Bool(true));
        global.remote_access.insert(
            "web_public_url".into(),
            Value::String("https://device-rust.example.test".into()),
        );
        Ok(())
    })
    .expect("settings");
    let status = ok(
        &home,
        "membership_status",
        json!({"by":"user","account_origin":origin}),
    );
    assert_eq!(status.result["membership"]["cut"], true);
    assert_eq!(status.result["membership"]["online"], false);
    let remote = settings::load(&home).expect("settings").remote_access;
    assert_eq!(remote["enabled"], false);
    assert_eq!(remote["web_public_url"], "");
}

#[test]
fn rust_remote_access_does_not_report_reach_running_without_a_live_helper() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, Some("acc_reach_status"))
        .expect("admin token");
    settings::update(&home, |global| {
        global
            .remote_access
            .insert("provider".into(), Value::String("reach".into()));
        global
            .remote_access
            .insert("enabled".into(), Value::Bool(true));
        global.remote_access.insert(
            "web_public_url".into(),
            Value::String("https://device-rust.example.test".into()),
        );
        Ok(())
    })
    .expect("settings");

    let state = ok(&home, "remote_access_state", json!({"by":"user"}));
    assert_eq!(state.result["remote_access"]["status"], "error");
    assert_eq!(state.result["remote_access"]["endpoint"], Value::Null);
    assert_eq!(
        state.result["remote_access"]["diagnostics"]["reach_helper_running"],
        false
    );
}

#[test]
fn rust_remote_access_rejects_configuration_while_reach_is_active() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    settings::update(&home, |global| {
        global
            .remote_access
            .insert("provider".into(), Value::String("reach".into()));
        global
            .remote_access
            .insert("enabled".into(), Value::Bool(true));
        Ok(())
    })
    .expect("settings");

    let changed = raw(
        &home,
        "remote_access_configure",
        json!({"provider":"manual","web_port":9000,"by":"user"}),
    );
    assert_eq!(
        changed.error.expect("ownership error").code,
        "remote_access_invalid_config"
    );
    let remote = settings::load(&home).expect("settings").remote_access;
    assert_eq!(remote["provider"], "reach");
    assert_eq!(remote["enabled"], true);
}

#[cfg(unix)]
#[test]
fn rust_reach_off_stops_a_cloudflared_process_started_by_python() {
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use std::time::{Duration, Instant};

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    let helper_dir = home.root().join("libexec").join("cloudflared");
    std::fs::create_dir_all(&helper_dir).expect("helper dir");
    let helper = helper_dir.join("cloudflared-test-helper");
    symlink("/bin/sleep", &helper).expect("helper symlink");
    let mut child = Command::new(&helper)
        .arg("30")
        .spawn()
        .expect("cloudflared fixture");
    std::fs::write(
        helper_dir.join("cloudflared.pid"),
        serde_json::to_vec(&json!({
            "schema":1,
            "pid":child.id(),
            "executable":std::fs::canonicalize(&helper).expect("helper executable")
        }))
        .expect("pid marker"),
    )
    .expect("pid");
    std::fs::write(helper_dir.join("cloudflared.token"), "secret").expect("token");
    let mut global = settings::load(&home).expect("settings");
    global
        .remote_access
        .insert("provider".into(), json!("reach"));
    global.remote_access.insert("enabled".into(), json!(true));
    settings::save(&home, &global).expect("settings save");

    let stopped = raw(&home, "membership_reach_off", json!({"by":"user"}));
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut exited = false;
    while Instant::now() < deadline {
        if child.try_wait().expect("wait").is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !exited {
        let _ = child.kill();
    }
    let _ = child.wait();
    assert!(stopped.ok, "reach off failed: {:?}", stopped.error);
    assert!(exited, "tracked cloudflared process remained alive");
    assert!(!helper_dir.join("cloudflared.pid").exists());
    assert!(!helper_dir.join("cloudflared.token").exists());
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
