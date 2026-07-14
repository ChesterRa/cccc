use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{HomeLayout, settings};
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
    assert!(!raw(&home, "remote_access_start", json!({"by":"user"})).ok);
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
