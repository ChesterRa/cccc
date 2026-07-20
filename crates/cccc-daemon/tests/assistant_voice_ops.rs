use cccc_contracts::{Actor, ActorRole, DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, Scope, ledger};
use serde_json::{Map, Value, json};

#[test]
fn voice_input_is_durable_idempotent_and_delivered_to_internal_actor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            let mut foreman = Actor::new("foreman");
            foreman.role = Some(ActorRole::Foreman);
            foreman.command = vec!["true".into()];
            doc.actors.push(foreman);
            doc.scopes.push(Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            doc.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("seed group");
    ok(
        &home,
        "actor_env_private_update",
        json!({"group_id":group.group_id,"actor_id":"foreman","set":{"VOICE_TEST_SECRET":"kept-private"}}),
    );

    let enabled = ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"assistant_id":"voice_secretary","by":"user","patch":{"enabled":true,"config":{"recognition_backend":"assistant_service_local_asr"}}}),
    );
    assert_eq!(enabled.result["assistant"]["enabled"], true);
    let loaded = store.load(&group.group_id).expect("load");
    let secretary = loaded
        .actors
        .iter()
        .find(|actor| actor.id == "voice-secretary")
        .expect("secretary actor");
    assert_eq!(secretary.internal_kind.as_deref(), Some("voice_secretary"));
    assert_eq!(secretary.runtime, loaded.actors[0].runtime);
    let secret_keys = ok(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group.group_id,"actor_id":"voice-secretary"}),
    );
    assert!(
        secret_keys.result["keys"]
            .as_array()
            .is_some_and(|keys| keys.iter().any(|key| key == "VOICE_TEST_SECRET"))
    );

    let args = json!({"group_id":group.group_id,"by":"user","session_id":"session-1","segment_id":"segment-1","text":"讨论发布计划和负责人。","language":"zh-CN","document_path":"docs/voice-secretary/meeting.md","is_final":true});
    let first = ok(&home, "assistant_voice_transcript_append", args.clone());
    assert_eq!(first.result["input_event_created"], true);
    assert_eq!(first.result["input_notify_emitted"], true);
    assert!(workspace.join("docs/voice-secretary/meeting.md").is_file());
    let duplicate = ok(&home, "assistant_voice_transcript_append", args);
    assert_eq!(duplicate.result["input_event_created"], false);

    let read = ok(
        &home,
        "assistant_voice_document_input_read",
        json!({"group_id":group.group_id,"by":"voice-secretary"}),
    );
    assert_eq!(read.result["item_count"], 1);
    assert!(
        read.result["input_text"]
            .as_str()
            .unwrap_or("")
            .contains("发布计划")
    );
    let second_read = ok(
        &home,
        "assistant_voice_document_input_read",
        json!({"group_id":group.group_id,"by":"voice-secretary"}),
    );
    assert_eq!(second_read.result["item_count"], 0);

    let events = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
        .expect("ledger");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "assistant.voice.input")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "system.notify"
                && event.data["kind"] == "voice_secretary_input")
            .count(),
        1
    );
}

#[test]
fn disabling_voice_secretary_removes_internal_actor_without_touching_documents() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            let mut actor = Actor::new("foreman");
            actor.role = Some(ActorRole::Foreman);
            doc.actors.push(actor);
            Ok(())
        })
        .expect("foreman");
    ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group.group_id,"document_path":"docs/voice-secretary/notes.md","content":"keep me"}),
    );
    ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":false}}),
    );
    let loaded = store.load(&group.group_id).expect("load");
    assert!(
        !loaded
            .actors
            .iter()
            .any(|actor| actor.id == "voice-secretary")
    );
    assert_eq!(
        loaded.extra["assistants"]["documents"][0]["content"],
        "keep me"
    );
}

#[test]
fn legacy_voice_secretary_shape_is_read_and_kept_in_sync() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("legacy", "").expect("group");
    store.mutate(&group.group_id,|doc|{doc.extra.insert("assistants".into(),json!({"voice_secretary":{"assistant_id":"voice_secretary","enabled":true,"lifecycle":"idle","config":{"recognition_backend":"browser_asr"}}}));Ok(())}).expect("legacy state");
    let index = ok(&home, "assistant_index", json!({"group_id":group.group_id}));
    assert_eq!(index.result["assistant"]["enabled"], true);
    ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":false,"config":{"recognition_language":"zh-CN"}}}),
    );
    let state = &store.load(&group.group_id).expect("load").extra["assistants"];
    assert_eq!(
        state["assistant"]["config"]["recognition_language"],
        "zh-CN"
    );
    assert_eq!(
        state["voice_secretary"]["config"]["recognition_language"],
        "zh-CN"
    );
}

#[test]
fn voice_input_retries_cleanly_after_document_preflight_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::write(workspace.join("docs"), b"blocks directory creation").expect("blocker");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            let mut foreman = Actor::new("foreman");
            foreman.role = Some(ActorRole::Foreman);
            doc.actors.push(foreman);
            doc.scopes.push(Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            doc.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("seed");
    ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    let args = json!({"group_id":group.group_id,"by":"user","session_id":"retry-session","segment_id":"retry-segment","text":"必须可靠送达","document_path":"docs/voice-secretary/retry.md","is_final":true});
    let failed = call(&home, "assistant_voice_transcript_append", args.clone());
    assert!(!failed.ok);
    let state = &store.load(&group.group_id).expect("load").extra["assistants"];
    assert_eq!(state["input_latest_seq"].as_u64().unwrap_or(0), 0);
    assert!(
        !home
            .root()
            .join("voice-secretary")
            .join(&group.group_id)
            .join("inputs.jsonl")
            .exists()
    );

    std::fs::remove_file(workspace.join("docs")).expect("remove blocker");
    let retried = ok(&home, "assistant_voice_transcript_append", args);
    assert_eq!(retried.result["input_event_created"], true);
    let read = ok(
        &home,
        "assistant_voice_document_input_read",
        json!({"group_id":group.group_id,"by":"voice-secretary"}),
    );
    assert_eq!(read.result["item_count"], 1);
}

#[test]
fn voice_document_and_input_permissions_are_enforced() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).expect("outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, workspace.join("linked")).expect("symlink");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            doc.scopes.push(Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            doc.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("scope");

    assert!(
        !call(
            &home,
            "assistant_voice_document_save",
            json!({"group_id":group.group_id,"document_path":"Cargo.toml","content":"overwrite"})
        )
        .ok
    );
    #[cfg(unix)]
    assert!(!call(&home,"assistant_voice_document_save",json!({"group_id":group.group_id,"document_path":"linked/outside.md","content":"escape"})).ok);
    assert!(
        !call(
            &home,
            "assistant_voice_document_input_read",
            json!({"group_id":group.group_id,"by":"foreman"})
        )
        .ok
    );
    assert!(!outside.join("outside.md").exists());
}

#[test]
fn enabling_voice_secretary_reports_runtime_start_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            let mut foreman = Actor::new("foreman");
            foreman.role = Some(ActorRole::Foreman);
            foreman.command = vec!["/cccc/command/that/does/not/exist".into()];
            doc.actors.push(foreman);
            doc.running = true;
            Ok(())
        })
        .expect("running group");
    let response = ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    assert_eq!(response.result["actor_started"], false);
    assert!(response.result["actor_start_error"].is_object());
    assert!(
        response.result["actor_start_error"]["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
}

#[test]
fn durable_log_remains_idempotent_after_session_window_is_trimmed() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let args = json!({"group_id":group_id,"by":"user","session_id":"long-session","segment_id":"old-segment","text":"只处理一次","document_path":"docs/voice-secretary/long.md","is_final":true});
    ok(&home, "assistant_voice_transcript_append", args.clone());
    store
        .mutate(&group_id, |group| {
            group.extra["assistants"]["sessions"][0]["segments"] = json!([]);
            Ok(())
        })
        .expect("trim session window");
    let duplicate = ok(&home, "assistant_voice_transcript_append", args);
    assert_eq!(duplicate.result["input_event_created"], false);
    let events = ledger::read_all(&store.ledger_path(&group_id).expect("ledger")).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "assistant.voice.input"
                && event.data["segment_id"] == "old-segment")
            .count(),
        1
    );
}

#[test]
fn durable_log_recovers_missing_ledger_delivery() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let input_root = home.root().join("voice-secretary").join(&group_id);
    std::fs::create_dir_all(&input_root).expect("input root");
    std::fs::write(
        input_root.join("inputs.jsonl"),
        format!(
            "{}\n",
            json!({"schema":1,"seq":1,"input_id":"vin-canonical","kind":"asr_transcript","text":"恢复投递","language":"zh-CN","document_path":"docs/voice-secretary/recover.md","session_id":"recover-session","segment_id":"recover-segment","by":"user","trigger":{},"created_at":"2026-01-01T00:00:00Z"})
        ),
    )
    .expect("seed input log");
    let response = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({"group_id":group_id,"by":"user","session_id":"recover-session","segment_id":"recover-segment","text":"恢复投递","document_path":"docs/voice-secretary/recover.md","is_final":true}),
    );
    assert_eq!(response.result["input_event"]["input_id"], "vin-canonical");
    let events = ledger::read_all(&store.ledger_path(&group_id).expect("ledger")).expect("events");
    assert!(events.iter().any(|event| {
        event.kind == "assistant.voice.input" && event.data["input_id"] == "vin-canonical"
    }));
}

#[test]
fn segment_ids_are_scoped_to_the_recording_session() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    for (session_id, text) in [("session-one", "第一段"), ("session-two", "第二段")] {
        let response = ok(
            &home,
            "assistant_voice_transcript_append",
            json!({"group_id":group_id,"by":"user","session_id":session_id,"segment_id":"seg-1","text":text,"document_path":"docs/voice-secretary/scoped.md","is_final":true}),
        );
        assert_eq!(response.result["input_event_created"], true);
    }
    let events = ledger::read_all(&store.ledger_path(&group_id).expect("ledger")).expect("events");
    let inputs = events
        .iter()
        .filter(|event| {
            event.kind == "assistant.voice.input" && event.data["segment_id"] == "seg-1"
        })
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), 2);
    assert_ne!(inputs[0].data["session_id"], inputs[1].data["session_id"]);
}

#[test]
fn incomplete_jsonl_tail_is_repaired_before_appending() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let first = json!({"group_id":group_id,"by":"user","session_id":"tail-one","segment_id":"seg-1","text":"完整记录","document_path":"docs/voice-secretary/tail.md","is_final":true});
    ok(&home, "assistant_voice_transcript_append", first);
    let input_path = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("inputs.jsonl");
    let mut bytes = std::fs::read(&input_path).expect("read input log");
    bytes.extend_from_slice(b"{\"schema\":1,\"segment_id\":\"partial");
    std::fs::write(&input_path, bytes).expect("damage tail");

    let second = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({"group_id":group_id,"by":"user","session_id":"tail-two","segment_id":"seg-1","text":"修复后记录","document_path":"docs/voice-secretary/tail.md","is_final":true}),
    );
    assert_eq!(second.result["input_event_created"], true);
    let records = std::fs::read_to_string(&input_path)
        .expect("read repaired log")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid jsonl"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1]["session_id"], "tail-two");
}

#[test]
fn saving_unchanged_document_does_not_increment_revision() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let args = json!({"group_id":group_id,"document_path":"docs/voice-secretary/stable.md","title":"Stable","content":"same"});
    let first = ok(&home, "assistant_voice_document_save", args.clone());
    let second = ok(&home, "assistant_voice_document_save", args);
    assert_eq!(first.result["document"]["revision_count"], 1);
    assert_eq!(second.result["document"]["revision_count"], 1);
}

#[test]
fn saving_existing_document_replaces_file_contents() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let document_path = "docs/voice-secretary/replaced.md";
    let first = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":document_path,"content":"first"}),
    );
    let absolute_path = first.result["document"]["absolute_path"]
        .as_str()
        .expect("absolute path")
        .to_owned();

    let second = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":document_path,"content":"second"}),
    );

    assert_eq!(second.result["document"]["revision_count"], 2);
    assert_eq!(
        std::fs::read_to_string(absolute_path).expect("read replaced document"),
        "second"
    );
}

fn enabled_voice_group() -> (tempfile::TempDir, HomeLayout, GroupStore, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            let mut foreman = Actor::new("foreman");
            foreman.role = Some(ActorRole::Foreman);
            foreman.command = vec!["true".into()];
            doc.actors.push(foreman);
            doc.scopes.push(Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            doc.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("group");
    ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    (temp, home, store, group.group_id)
}

fn ok(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = call(home, op, args);
    assert!(response.ok, "{op} failed: {:?}", response.error);
    response
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
