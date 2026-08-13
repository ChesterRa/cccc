#![cfg(unix)]

// Included by the crate-level integration test harness.

use cccc_core::HomeLayout;
use serde_json::{Value, json};

#[path = "runtime_lifecycle/support.rs"]
mod support;
#[path = "runtime_lifecycle/terminal_attachment.rs"]
mod terminal_attachment;

use support::{call, raw_call};

#[test]
fn actor_lifecycle_controls_terminal_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(
        &home,
        "group_create",
        json!({"title":"runtime-test","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    attach_project_scope(&home, &group_id, temp.path());
    assert!(
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":"peer1",
                "runner":"pty",
                "runtime":"custom",
                "command":["sh","-c","printf 'daemon-runtime-ready\\n• Working (1s • esc to interrupt)\\n'; sleep 5"],
                "by":"user"
            }),
        )
        .ok
    );
    let started = call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    assert_eq!(started.result["event"]["kind"], "actor.start");
    let groups = call(&home, "group_list", json!({}));
    let summary = groups.result["groups"]
        .as_array()
        .and_then(|groups| groups.iter().find(|group| group["group_id"] == group_id))
        .expect("group summary");
    assert_eq!(summary["running"], true);
    assert_eq!(summary["runtime_status"]["running_actor_count"], 1);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let tail = call(
        &home,
        "terminal_tail",
        json!({"group_id":group_id,"actor_id":"peer1"}),
    );
    assert!(
        tail.result["text"]
            .as_str()
            .unwrap_or_default()
            .contains("daemon-runtime-ready")
    );
    let end_cursor = tail.result["end_cursor"].as_u64().expect("end cursor");
    let since = call(
        &home,
        "terminal_since",
        json!({"group_id":group_id,"actor_id":"peer1","after":end_cursor}),
    );
    assert_eq!(since.result["history"]["data"], "");
    assert_eq!(since.result["history"]["end_cursor"], end_cursor);
    let missing_cursor = raw_call(
        &home,
        "terminal_since",
        json!({"group_id":group_id,"actor_id":"peer1"}),
    );
    assert!(!missing_cursor.ok);
    assert_eq!(
        missing_cursor
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("invalid_args")
    );
    for invalid_size in [
        json!({"group_id":group_id,"actor_id":"peer1","cols":9,"rows":1}),
        json!({"group_id":group_id,"actor_id":"peer1"}),
        json!({"group_id":group_id,"actor_id":"peer1","cols":70_000,"rows":24}),
    ] {
        let response = raw_call(&home, "term_resize", invalid_size);
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_size")
        );
    }
    let resized = call(
        &home,
        "term_resize",
        json!({"group_id":group_id,"actor_id":"peer1","cols":100,"rows":30}),
    );
    assert!(resized.ok);
    assert_eq!(resized.result["group_id"], group_id);
    assert_eq!(resized.result["actor_id"], "peer1");
    assert_eq!(resized.result["cols"], 100);
    assert_eq!(resized.result["rows"], 30);
    assert!(
        call(
            &home,
            "terminal_resize",
            json!({"group_id":group_id,"actor_id":"peer1","cols":101,"rows":31}),
        )
        .ok
    );
    terminal_attachment::assert_resize_ownership(&home, &group_id);
    let restarted = call(
        &home,
        "actor_restart",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    assert_eq!(restarted.result["event"]["kind"], "actor.restart");
    assert!(
        call(
            &home,
            "actor_update",
            json!({
                "group_id":group_id,
                "actor_id":"peer1",
                "runtime":"codex",
                "runtime_state_source":"app_server",
                "by":"user"
            }),
        )
        .ok
    );
    cccc_core::codex_hook_state::begin_launch(
        &home,
        "codex",
        &group_id,
        "peer1",
        "token",
        "HookPending",
    )
    .expect("begin hook launch");
    cccc_core::codex_hook_state::record(
        &home,
        &group_id,
        "peer1",
        "token",
        &json!({"hook_event_name":"SessionStart","session_id":"s1"}),
    )
    .expect("session state");
    cccc_core::codex_hook_state::record(
        &home,
        &group_id,
        "peer1",
        "token",
        &json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"s1",
            "turn_id":"turn-1"
        }),
    )
    .expect("hook state");
    let working = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(working.result["actors"][0]["running"], true);
    assert_eq!(
        working.result["actors"][0]["effective_working_state"],
        "waiting"
    );
    assert_eq!(
        working.result["actors"][0]["effective_working_reason"],
        "codex_hook_pending"
    );
    let stopped = call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    assert_eq!(stopped.result["event"]["kind"], "actor.stop");
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(actors.result["actors"][0]["running"], false);
    assert_eq!(
        actors.result["actors"][0]["effective_working_state"],
        "stopped"
    );
    assert_eq!(
        actors.result["actors"][0]["effective_working_reason"],
        "runner_not_running"
    );
    assert_eq!(actors.result["actors"][0]["runner_effective"], "pty");

    assert!(
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":"peer-remove",
                "runner":"pty",
                "runtime":"custom",
                "command":["sh","-c","sleep 30"],
                "by":"user"
            }),
        )
        .ok
    );
    assert!(
        call(
            &home,
            "actor_start",
            json!({"group_id":group_id,"actor_id":"peer-remove","by":"user"}),
        )
        .ok
    );
    assert!(cccc_runtime::status(&group_id, "peer-remove").is_ok());
    let removed = call(
        &home,
        "actor_remove",
        json!({"group_id":group_id,"actor_id":"peer-remove","by":"user"}),
    );
    assert_eq!(removed.result["actor_id"], "peer-remove");
    assert_eq!(removed.result["event"]["kind"], "actor.remove");
    assert!(cccc_runtime::status(&group_id, "peer-remove").is_err());
}

#[test]
fn terminal_operations_distinguish_invalid_targets_from_inactive_pty_sessions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(
        &home,
        "group_create",
        json!({"title":"terminal-targets","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    assert!(
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":"pty-stopped",
                "runner":"pty",
                "runtime":"codex",
                "by":"user"
            }),
        )
        .ok
    );
    assert!(
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":"headless",
                "runner":"headless",
                "runtime":"web_model",
                "by":"user"
            }),
        )
        .ok
    );

    let args = |op: &str, target_group: &str, actor_id: &str| {
        let mut args = json!({"group_id":target_group,"actor_id":actor_id})
            .as_object()
            .cloned()
            .expect("args");
        if op == "terminal_since" {
            args.insert("after".into(), json!(0));
        }
        if op == "term_resize" {
            args.insert("cols".into(), json!(80));
            args.insert("rows".into(), json!(24));
        }
        Value::Object(args)
    };
    let operations = [
        "terminal_tail",
        "terminal_snapshot",
        "terminal_replay",
        "terminal_history",
        "terminal_since",
        "terminal_clear",
        "term_resize",
    ];

    for op in operations {
        for (target_group, actor_id, expected) in [
            ("missing-group", "missing", "group_not_found"),
            (group_id.as_str(), "missing", "actor_not_found"),
            (group_id.as_str(), "headless", "not_pty_actor"),
        ] {
            let response = raw_call(&home, op, args(op, target_group, actor_id));
            assert_eq!(
                response.error.as_ref().map(|error| error.code.as_str()),
                Some(expected),
                "{op} should reject {target_group}/{actor_id}: {response:?}"
            );
        }
    }

    for op in [
        "terminal_tail",
        "terminal_snapshot",
        "terminal_history",
        "terminal_since",
    ] {
        let response = raw_call(&home, op, args(op, &group_id, "pty-stopped"));
        assert!(
            response.ok,
            "{op} should return an empty history: {response:?}"
        );
    }
    for op in ["terminal_replay", "terminal_clear", "term_resize"] {
        let response = raw_call(&home, op, args(op, &group_id, "pty-stopped"));
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("actor_not_running"),
            "{op} requires an active PTY session: {response:?}"
        );
    }
}

#[test]
fn actor_update_enabled_keeps_persisted_and_live_lifecycle_in_sync() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"update lifecycle","by":"user"}),
    );
    let group_id = created.result["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    attach_project_scope(&home, &group_id, temp.path());
    for actor_id in ["peer1", "keeper"] {
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":actor_id,
                "runner":"pty",
                "runtime":"custom",
                "command":["sh","-c","sleep 30"],
                "by":"user"
            }),
        );
        call(
            &home,
            "actor_start",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        );
    }

    let disabled = call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"enabled":false},
            "by":"user"
        }),
    );
    assert_eq!(disabled.result["actor"]["enabled"], false);
    assert_eq!(disabled.result["event"]["kind"], "actor.update");
    assert!(cccc_runtime::status(&group_id, "peer1").is_err());
    assert!(
        cccc_runtime::status(&group_id, "keeper").is_ok_and(|status| status.running),
        "disabling one actor must not stop another"
    );
    assert!(
        cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .load(&group_id)
            .expect("group")
            .running,
        "the keeper keeps the group running"
    );

    let enabled = call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"enabled":true},
            "by":"user"
        }),
    );
    assert_eq!(enabled.result["actor"]["enabled"], true);
    assert!(cccc_runtime::status(&group_id, "peer1").is_ok_and(|status| status.running));

    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    let enabled_while_stopped = call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"enabled":true},
            "by":"user"
        }),
    );
    assert_eq!(enabled_while_stopped.result["actor"]["enabled"], true);
    assert!(cccc_runtime::status(&group_id, "peer1").is_err());
    let stopped = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .load(&group_id)
        .expect("group");
    assert!(!stopped.running);
    assert_eq!(stopped.state, cccc_contracts::GroupState::Stopped);
}

#[test]
fn actor_update_enable_rolls_back_when_runtime_start_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"update rollback","by":"user"}),
    );
    let group_id = created.result["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    attach_project_scope(&home, &group_id, temp.path());
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"keeper",
            "runner":"pty",
            "runtime":"custom",
            "command":["sh","-c","sleep 30"],
            "by":"user"
        }),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"keeper","by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"broken",
            "runner":"pty",
            "runtime":"custom",
            "command":["cccc-command-that-does-not-exist"],
            "enabled":false,
            "by":"user"
        }),
    );

    let failed = raw_call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"broken",
            "patch":{"enabled":true},
            "by":"user"
        }),
    );
    assert!(!failed.ok, "failed runtime launch must not report success");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .load(&group_id)
        .expect("group");
    assert!(
        !group
            .actors
            .iter()
            .find(|actor| actor.id == "broken")
            .expect("broken actor")
            .enabled
    );
    assert!(group.running, "the keeper must keep the group running");
    assert!(cccc_runtime::status(&group_id, "broken").is_err());
    let events = cccc_core::ledger::read_all(
        &cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .ledger_path(&group_id)
            .expect("ledger path"),
    )
    .expect("ledger");
    assert!(
        !events
            .iter()
            .any(|event| { event.kind == "actor.update" && event.data["actor_id"] == "broken" })
    );
    let recovered = call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"broken","by":"user"}),
    );
    assert_eq!(recovered.result["event"]["kind"], "actor.stop");
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
}

#[test]
fn actor_update_enabled_compensates_runtime_when_event_append_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"update event rollback","by":"user"}),
    );
    let group_id = created.result["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    attach_project_scope(&home, &group_id, temp.path());
    for actor_id in ["peer1", "keeper"] {
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":actor_id,
                "runner":"pty",
                "runtime":"custom",
                "command":["sh","-c","sleep 30"],
                "by":"user"
            }),
        );
        call(
            &home,
            "actor_start",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        );
    }

    let backup = obstruct_ledger(&home, &group_id);
    let failed_disable = raw_call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"enabled":false},
            "by":"user"
        }),
    );
    assert!(!failed_disable.ok);
    let restored = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .load(&group_id)
        .expect("group");
    assert!(restored.actors[0].enabled);
    assert!(restored.running);
    assert!(cccc_runtime::status(&group_id, "peer1").is_ok_and(|status| status.running));
    restore_ledger(&home, &group_id, &backup);

    call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"enabled":false},
            "by":"user"
        }),
    );
    assert!(cccc_runtime::status(&group_id, "peer1").is_err());

    let backup = obstruct_ledger(&home, &group_id);
    let failed_enable = raw_call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"enabled":true},
            "by":"user"
        }),
    );
    assert!(!failed_enable.ok);
    let restored = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .load(&group_id)
        .expect("group");
    assert!(!restored.actors[0].enabled);
    assert!(restored.running);
    assert!(cccc_runtime::status(&group_id, "peer1").is_err());
    restore_ledger(&home, &group_id, &backup);
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
}

fn obstruct_ledger(home: &HomeLayout, group_id: &str) -> std::path::PathBuf {
    let ledger = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .ledger_path(group_id)
        .expect("ledger path");
    let backup = ledger.with_extension("jsonl.backup");
    std::fs::rename(&ledger, &backup).expect("backup ledger");
    std::fs::create_dir(&ledger).expect("obstruct ledger path");
    backup
}

fn restore_ledger(home: &HomeLayout, group_id: &str, backup: &std::path::Path) {
    let ledger = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .ledger_path(group_id)
        .expect("ledger path");
    std::fs::remove_dir(&ledger).expect("remove ledger obstruction");
    std::fs::rename(backup, ledger).expect("restore ledger");
}

fn attach_project_scope(home: &HomeLayout, group_id: &str, temp_root: &std::path::Path) {
    let project = temp_root.join("project");
    std::fs::create_dir(&project).expect("project scope");
    call(
        home,
        "attach",
        json!({"group_id":group_id,"path":project,"by":"user"}),
    );
}
