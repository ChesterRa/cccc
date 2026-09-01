use super::super::*;
use super::super::{launch, process};
use cccc_core::{GroupStore, HomeLayout, group_scope, scope};
use std::io;
use std::path::Path;

#[test]
fn scope_binding_requires_the_exact_attached_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let root = temp.path().join("project");
    let other = temp.path().join("other");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::create_dir_all(&other).expect("other");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    let group = group_scope::attach(
        &store,
        &group.group_id,
        scope::detect(&root).expect("scope"),
    )
    .expect("attach");

    let binding = launch::bind_scope(&home, &group.group_id, &root).expect("binding");
    assert_eq!(binding.group_id, group.group_id);
    assert_eq!(binding.root, root.canonicalize().expect("canonical root"));
    assert_eq!(
        launch::bind_scope(&home, &group.group_id, &other)
            .expect_err("unattached root")
            .kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn app_server_launch_matches_the_codex_actor_yolo_policy() {
    assert_eq!(
        launch::app_server_command(Path::new("/opt/codex"), Some("  voice  ")),
        vec![
            "/opt/codex",
            "--dangerously-bypass-approvals-and-sandbox",
            "--search",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
            "--profile",
            "voice",
            "app-server",
            "--listen",
            "ws://127.0.0.1:0",
        ]
    );
}

#[test]
fn app_server_endpoint_must_be_an_ip_loopback_websocket() {
    assert_eq!(
        process::parse_listening_endpoint("  listening on: ws://127.0.0.1:43123  ").as_deref(),
        Some("ws://127.0.0.1:43123")
    );
    assert!(process::validate_loopback_endpoint("ws://127.0.0.1:43123").is_ok());
    assert!(process::validate_loopback_endpoint("ws://[::1]:43123").is_ok());
    assert!(process::validate_loopback_endpoint("ws://0.0.0.0:43123").is_err());
    assert!(process::validate_loopback_endpoint("wss://127.0.0.1:43123").is_err());
    assert!(process::validate_loopback_endpoint("ws://localhost:43123").is_err());
    assert_eq!(ElicitationAction::Decline.as_str(), "decline");
}
