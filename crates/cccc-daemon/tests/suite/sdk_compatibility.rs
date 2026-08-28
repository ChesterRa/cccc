// Included by the crate-level integration test harness.
use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, ledger};
use serde_json::{Map, Value, json};

fn request(op: &str) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.to_owned(),
        args: Map::new(),
    }
}

fn call(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.to_owned(),
            args: args.as_object().cloned().expect("object args"),
        },
    )
}

#[test]
fn ping_exposes_truthful_sdk_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let response = cccc_daemon::handle_request(&home, &request("ping"));

    assert!(response.ok);
    assert_eq!(response.result["ipc_v"], json!(1));
    assert_eq!(response.result["implementation"], json!("rust"));
    assert_eq!(
        response.result["capabilities"]["events_stream"],
        json!(true)
    );
    assert_eq!(
        response.result["capabilities"]["remote_access"],
        json!(true)
    );
    assert_eq!(
        response.result["capabilities"]["term_attachment_status"],
        json!(true)
    );
    assert_eq!(
        response.result["capabilities"]["term_attach_snapshot_v1"],
        json!(true)
    );
    for operation in [
        "presentation_browser_attach",
        "presentation_browser_vnc_attach",
        "space_provider_auth_browser_attach",
        "space_provider_auth_browser_vnc_attach",
        "web_model_browser_attach",
        "web_model_browser_vnc_attach",
    ] {
        assert_eq!(
            response.result["capabilities"][operation],
            json!(false),
            "{operation}"
        );
    }
    assert!(response.result["pid"].as_u64().is_some());
    assert!(
        response.result["version"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let timestamp = response.result["ts"].as_str().expect("ping timestamp");
    chrono::DateTime::parse_from_rfc3339(timestamp).expect("RFC 3339 timestamp");
}

#[test]
fn sdk_operation_probes_recognize_send_and_tracked_send() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");

    for op in ["send", "tracked_send"] {
        let response = cccc_daemon::handle_request(&home, &request(op));
        assert!(!response.ok, "{op} probe unexpectedly succeeded");
        assert_ne!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("unknown_op"),
            "{op} must be discoverable through the SDK compatibility probe"
        );
    }
}

#[test]
fn context_sync_enforces_actor_authority_and_returns_the_standard_receipt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"context authority","by":"user"}),
    );
    let group_id = created.result["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    assert!(
        call(
            &home,
            "group_stop",
            json!({"group_id":group_id,"by":"user"})
        )
        .ok
    );
    for actor_id in ["lead", "peer-a", "peer-b"] {
        let added = call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":actor_id,
                "runtime":"custom",
                "runner":"pty",
                "command":["sh","-c","exit 0"],
                "by":"user"
            }),
        );
        assert!(added.ok, "add {actor_id}: {:?}", added.error);
    }

    let recovered = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"user",
            "ops":[{"op":"agent_state.update","actor_id":"peer-a","focus":"recover"}]
        }),
    );
    assert!(recovered.ok, "user recovery: {:?}", recovered.error);
    assert_eq!(recovered.result["success"], true);
    assert_eq!(recovered.result["dry_run"], false);
    assert!(recovered.result["changes"].is_array());
    assert!(recovered.result["version"].is_string());
    assert!(!recovered.result.contains_key("context"));

    let foreman_state = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"lead",
            "ops":[{"op":"agent_state.update","actor_id":"peer-a","focus":"overwrite"}]
        }),
    );
    assert_eq!(
        foreman_state
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("permission_denied")
    );

    let cross_assigned = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"peer-b",
            "ops":[{"op":"task.create","title":"unauthorized","assignee":"peer-a"}]
        }),
    );
    assert_eq!(
        cross_assigned
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("permission_denied")
    );

    let create_task = |title: &str, assignee: &str| {
        let response = call(
            &home,
            "context_sync",
            json!({
                "group_id":group_id,
                "by":"user",
                "ops":[{"op":"task.create","title":title,"assignee":assignee}]
            }),
        );
        assert!(response.ok, "create {title}: {:?}", response.error);
        let listed = call(&home, "task_list", json!({"group_id":group_id,"by":"user"}));
        listed.result["tasks"]
            .as_array()
            .expect("tasks")
            .iter()
            .find(|task| task["title"] == title)
            .and_then(|task| task["id"].as_str())
            .expect("task id")
            .to_owned()
    };

    let protected_id = create_task("protected", "peer-a");
    for operation in [
        json!({"op":"task.update","task_id":protected_id,"notes":"overwrite"}),
        json!({"op":"task.move","task_id":protected_id,"status":"done"}),
        json!({"op":"task.delete","task_id":protected_id}),
    ] {
        let denied = call(
            &home,
            "context_sync",
            json!({"group_id":group_id,"by":"peer-b","ops":[operation]}),
        );
        assert_eq!(
            denied.error.as_ref().map(|error| error.code.as_str()),
            Some("permission_denied")
        );
    }

    let restorable_id = create_task("restorable", "peer-a");
    assert!(
        call(
            &home,
            "context_sync",
            json!({
                "group_id":group_id,
                "by":"user",
                "ops":[{"op":"task.move","task_id":restorable_id,"status":"archived"}]
            })
        )
        .ok
    );
    let restore = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"peer-b",
            "ops":[{"op":"task.restore","task_id":restorable_id}]
        }),
    );
    assert_eq!(
        restore.error.as_ref().map(|error| error.code.as_str()),
        Some("permission_denied")
    );

    let reassign_id = create_task("reassign", "peer-a");
    let reassign = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"lead",
            "ops":[{"op":"task.update","task_id":reassign_id,"assignee":"peer-b"}]
        }),
    );
    assert!(reassign.ok, "foreman reassign: {:?}", reassign.error);

    let owner_update = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"peer-a",
            "ops":[{"op":"task.update","task_id":protected_id,"notes":"owner update"}]
        }),
    );
    assert!(owner_update.ok, "owner update: {:?}", owner_update.error);
    assert_eq!(owner_update.result["success"], true);

    let atomic = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"peer-b",
            "ops":[
                {"op":"agent_state.update","actor_id":"peer-b","focus":"must roll back"},
                {"op":"agent_state.update","actor_id":"peer-a","focus":"not allowed"}
            ]
        }),
    );
    assert_eq!(
        atomic.error.as_ref().map(|error| error.code.as_str()),
        Some("permission_denied")
    );
    let document = cccc_core::context::ContextStore::new(home)
        .expect("context store")
        .load(&group_id)
        .expect("context");
    assert!(!document.agent_states.contains_key("peer-b"));
}

#[test]
fn context_sync_rejects_invalid_task_state_and_preserves_execution_history() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"task invariants","by":"user"}),
    );
    let group_id = created.result["group_id"].as_str().expect("group id");

    let invalid = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"user",
            "ops":[{"op":"task.create","title":"invalid","status":"bogus"}]
        }),
    );
    assert_eq!(
        invalid.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_args")
    );

    let created_task = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"user",
            "ops":[{"op":"task.create","title":"historical"}]
        }),
    );
    assert!(created_task.ok, "create: {:?}", created_task.error);
    assert!(
        call(
            &home,
            "context_sync",
            json!({
                "group_id":group_id,
                "by":"user",
                "ops":[{"op":"task.move","task_id":"T001","status":"active"}]
            })
        )
        .ok
    );
    let deleted = call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"user",
            "ops":[{"op":"task.delete","task_id":"T001"}]
        }),
    );
    assert_eq!(
        deleted.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_args")
    );
    let tasks = call(&home, "task_list", json!({"group_id":group_id,"by":"user"}));
    assert_eq!(tasks.result["tasks"].as_array().expect("tasks").len(), 1);
    assert_eq!(tasks.result["tasks"][0]["status"], "active");
    let task = call(
        &home,
        "task_list",
        json!({"group_id":group_id,"task_id":"T001","by":"user"}),
    );
    assert_eq!(task.result["task"]["id"], "T001");
    assert_eq!(task.result["task"]["children"], json!([]));
}

#[test]
fn group_update_uses_the_standard_patch_contract_with_bounded_legacy_input() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"before","topic":"old","by":"user"}),
    );
    assert!(created.ok, "create: {:?}", created.error);
    let group_id = created.result["group_id"].as_str().expect("group id");

    let updated = call(
        &home,
        "group_update",
        json!({
            "group_id":group_id,
            "by":"user",
            "patch":{"title":"after","topic":"new"}
        }),
    );
    assert!(updated.ok, "canonical update: {:?}", updated.error);
    assert_eq!(updated.result["group_id"], group_id);
    assert_eq!(updated.result["group"]["title"], "after");
    assert_eq!(updated.result["group"]["topic"], "new");
    assert_eq!(
        updated.result["event"]["data"]["patch"],
        json!({"title":"after","topic":"new"})
    );

    let ledger_path = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .ledger_path(group_id)
        .expect("ledger path");
    let events = ledger::read_all(&ledger_path).expect("ledger");
    let event = events.last().expect("group update event");
    assert_eq!(event.id, updated.result["event"]["id"]);
    assert_eq!(event.data["patch"], json!({"title":"after","topic":"new"}));

    for patch in [json!({}), json!({"unsupported":true})] {
        let rejected = call(
            &home,
            "group_update",
            json!({"group_id":group_id,"by":"user","patch":patch}),
        );
        assert!(!rejected.ok, "invalid patch unexpectedly succeeded");
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_patch")
        );
    }
    for patch in [json!(null), json!(["title"]), json!({"title":7})] {
        let rejected = call(
            &home,
            "group_update",
            json!({"group_id":group_id,"by":"user","patch":patch}),
        );
        assert!(!rejected.ok, "malformed patch unexpectedly succeeded");
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_patch")
        );
    }

    let canonical_wins = call(
        &home,
        "group_update",
        json!({
            "group_id":group_id,
            "by":"user",
            "title":"ignored legacy title",
            "patch":{"title":"canonical"}
        }),
    );
    assert!(
        canonical_wins.ok,
        "precedence update: {:?}",
        canonical_wins.error
    );
    assert_eq!(canonical_wins.result["group"]["title"], "canonical");
    assert_eq!(
        canonical_wins.result["event"]["data"]["patch"],
        json!({"title":"canonical"})
    );

    let blank_title = call(
        &home,
        "group_update",
        json!({"group_id":group_id,"by":"user","patch":{"title":"  "}}),
    );
    assert!(
        blank_title.ok,
        "blank title update: {:?}",
        blank_title.error
    );
    assert_eq!(blank_title.result["group"]["title"], "canonical");

    let legacy = call(
        &home,
        "group_update",
        json!({"group_id":group_id,"by":"user","title":"legacy"}),
    );
    assert!(legacy.ok, "legacy update: {:?}", legacy.error);
    assert_eq!(legacy.result["group"]["title"], "legacy");
    assert_eq!(
        legacy.result["event"]["data"]["patch"],
        json!({"title":"legacy"})
    );
}
