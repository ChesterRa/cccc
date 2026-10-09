use cccc_contracts::{Actor, ActorRole, DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, Scope, assistant_state, ledger};
use fs2::FileExt;
use serde_json::{Map, Value, json};
use std::fs::OpenOptions;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[path = "assistant_voice_ops/settings_autosave.rs"]
mod settings_autosave;

#[path = "assistant_voice_ops/document_delete.rs"]
mod document_delete;
#[path = "assistant_voice_ops/document_delete_consistency.rs"]
mod document_delete_consistency;
#[path = "assistant_voice_ops/document_library.rs"]
mod document_library;
#[path = "assistant_voice_ops/voice_session_update.rs"]
mod voice_session_update;

fn load_voice_state(home: &HomeLayout, group_id: &str) -> Value {
    let mut state = assistant_state::load(home, group_id).expect("assistant state");
    let documents = ok(
        home,
        "assistant_voice_document_list",
        json!({"group_id":group_id,"include_archived":true}),
    );
    let root = state.as_object_mut().expect("assistant state object");
    for key in ["documents", "active_document_id", "active_document_path"] {
        root.insert(key.into(), documents.result[key].clone());
    }
    state
}

fn update_voice_state(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> std::io::Result<()>,
) {
    assistant_state::update(home, group_id, change).expect("assistant state update");
}

fn set_voice_active_document_id(home: &HomeLayout, group_id: &str, document_id: &Value) {
    let path = home
        .root()
        .join("voice-secretary")
        .join(group_id)
        .join("documents/index.json");
    let mut index: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("voice document index"))
            .expect("valid voice document index");
    index["active_document_id"] = document_id.clone();
    let mut bytes = serde_json::to_vec_pretty(&index).expect("serialize voice document index");
    bytes.push(b'\n');
    cccc_core::fs::atomic_write(&path, &bytes).expect("write voice document index");
}

#[test]
fn document_only_transcript_clear_reports_that_visible_history_was_cleared() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let document_path = "docs/voice-secretary/document-only.md";
    let appended = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group_id,
            "by":"user",
            "session_id":"document-only-session",
            "segment_id":"segment-1",
            "text":"durable document transcript",
            "document_path":document_path,
            "is_final":true
        }),
    );
    let document_id = appended.result["document"]["document_id"]
        .as_str()
        .expect("document id");
    let transcript_path = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("documents")
        .join(document_id)
        .join("transcript.jsonl");
    assert!(transcript_path.is_file());

    update_voice_state(&home, &group_id, |state| {
        state.insert("sessions".into(), json!([]));
        Ok(())
    });
    let fallback = ok(
        &home,
        "assistant_state",
        json!({"group_id":group_id,"view":"voice_session","document_path":document_path}),
    );
    assert_eq!(fallback.result["session"]["source"], "document_transcript");

    let cleared = ok(
        &home,
        "assistant_voice_session_transcript_clear",
        json!({"group_id":group_id,"document_path":document_path,"by":"user"}),
    );
    assert_eq!(cleared.result["cleared"], true);
    assert!(!transcript_path.exists());
}

#[test]
fn transcript_append_and_clear_share_the_group_mutation_lock() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let lock_path = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("transcript.lock");
    std::fs::create_dir_all(lock_path.parent().expect("lock parent")).expect("lock directory");

    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("transcript lock");
    lock.lock_exclusive().expect("lock append");
    let append_home = home.clone();
    let append_group_id = group_id.clone();
    let (append_started_tx, append_started_rx) = mpsc::channel();
    let append = thread::spawn(move || {
        append_started_tx.send(()).expect("append start receiver");
        call(
            &append_home,
            "assistant_voice_transcript_append",
            json!({
                "group_id":append_group_id,
                "session_id":"locked-session",
                "segment_id":"locked-segment",
                "document_path":"docs/voice-secretary/locked.md",
                "text":"serialized transcript",
                "is_final":true,
                "by":"user"
            }),
        )
    });
    append_started_rx.recv().expect("append started");
    thread::sleep(Duration::from_millis(200));
    assert!(
        !append.is_finished(),
        "append must wait for the transcript mutation lock"
    );
    FileExt::unlock(&lock).expect("unlock append");
    drop(lock);
    let appended = append.join().expect("append thread");
    assert!(appended.ok, "append failed: {:?}", appended.error);

    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("transcript lock");
    lock.lock_exclusive().expect("lock clear");
    let clear_home = home.clone();
    let clear_group_id = group_id.clone();
    let (clear_started_tx, clear_started_rx) = mpsc::channel();
    let clear = thread::spawn(move || {
        clear_started_tx.send(()).expect("clear start receiver");
        call(
            &clear_home,
            "assistant_voice_session_transcript_clear",
            json!({
                "group_id":clear_group_id,
                "session_id":"locked-session",
                "document_path":"docs/voice-secretary/locked.md",
                "by":"user"
            }),
        )
    });
    clear_started_rx.recv().expect("clear started");
    thread::sleep(Duration::from_millis(200));
    assert!(
        !clear.is_finished(),
        "clear must wait for the transcript mutation lock"
    );
    FileExt::unlock(&lock).expect("unlock clear");
    drop(lock);
    let cleared = clear.join().expect("clear thread");
    assert!(cleared.ok, "clear failed: {:?}", cleared.error);
    assert_eq!(cleared.result["cleared"], true);
}

#[test]
fn voice_session_mutations_reject_path_like_session_ids() {
    let (temp, home, _store, group_id) = enabled_voice_group();
    let external_session = temp.path().join("external-session");
    let external_transcript = external_session.join("transcripts/segments.jsonl");
    std::fs::create_dir_all(external_transcript.parent().expect("transcript parent"))
        .expect("external transcript directory");
    std::fs::write(&external_transcript, b"preserve me\n").expect("external transcript");
    let external_session_id = external_session.to_string_lossy().into_owned();

    update_voice_state(&home, &group_id, |state| {
        state.insert(
            "sessions".into(),
            json!([{
                "schema":1,
                "group_id":group_id,
                "session_id":external_session_id,
                "capture_mode":"document",
                "document_path":"docs/voice-secretary/unsafe.md",
                "segments":[],
                "transcript":"unsafe"
            }]),
        );
        Ok(())
    });

    let clear = call(
        &home,
        "assistant_voice_session_transcript_clear",
        json!({
            "group_id":group_id,
            "session_id":external_session_id,
            "by":"user"
        }),
    );
    assert!(!clear.ok, "absolute session id unexpectedly accepted");
    assert_eq!(
        clear.error.expect("invalid session id").code,
        "invalid_args"
    );
    assert!(
        external_transcript.is_file(),
        "transcript outside the group session root was deleted"
    );

    let update = call(
        &home,
        "assistant_voice_session_update",
        json!({
            "group_id":group_id,
            "session_id":"..",
            "by":"assistant:voice_secretary",
            "patch":{"status":"closed"}
        }),
    );
    assert!(
        !update.ok,
        "parent-directory session id unexpectedly accepted"
    );
    assert_eq!(
        update.error.expect("invalid session id").code,
        "invalid_args"
    );
}

#[test]
fn voice_input_append_routes_voice_instruction_to_the_ask_pipeline() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let args = json!({
        "group_id":group_id,
        "kind":"voice_instruction",
        "request_id":"voice-ask-generic-entry",
        "input_append_id":"voice-ask-generic-entry-input",
        "instruction":"Check the latest summary for omissions"
    });
    let input = ok(&home, "assistant_voice_input_append", args.clone());

    assert_eq!(input.result["request_id"], "voice-ask-generic-entry");
    assert_eq!(input.result["input_event_created"], true);
    assert_eq!(input.result["ask_request"]["status"], "pending");
    assert_eq!(
        input.result["input_event"]["metadata"]["target_kind"],
        "secretary"
    );

    let retry = ok(&home, "assistant_voice_input_append", args);
    assert_eq!(retry.result["input_event_created"], false);
    assert_eq!(retry.result["request_id"], "voice-ask-generic-entry");
}

#[test]
fn document_instruction_targets_an_existing_document_and_requires_a_report() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/release.md",
            "content":"# 发布计划\n"
        }),
    );
    let input = ok(
        &home,
        "assistant_voice_document_instruction",
        json!({
            "group_id":group_id,
            "request_id":"voice-ask-document",
            "input_append_id":"voice-ask-document-input",
            "document_path":"docs/voice-secretary/release.md",
            "instruction":"补充负责人和回滚步骤"
        }),
    );

    assert_eq!(
        input.result["document"]["document_id"],
        saved.result["document"]["document_id"]
    );
    assert_eq!(
        input.result["input_event"]["metadata"]["target_kind"],
        "document"
    );
    assert_eq!(
        input.result["input_event"]["trigger"]["intent_hint"],
        "document_instruction"
    );
    let state = load_voice_state(&home, &group_id);
    assert_eq!(state["documents"].as_array().map(Vec::len), Some(1));
    assert_eq!(state["ask_requests"][0]["target_kind"], "document");
    assert_eq!(
        state["ask_requests"][0]["document_path"],
        "docs/voice-secretary/release.md"
    );
}

#[test]
fn archiving_document_removes_it_from_index_and_selects_the_next_active_document() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let first = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/first.md",
            "content":"# First\n"
        }),
    );
    let second = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/second.md",
            "content":"# Second\n"
        }),
    );
    assert_eq!(
        load_voice_state(&home, &group_id)["active_document_id"],
        second.result["document"]["document_id"]
    );

    let archived = ok(
        &home,
        "assistant_voice_document_archive",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/second.md"
        }),
    );
    assert_eq!(archived.result["document"]["status"], "archived");

    let state = load_voice_state(&home, &group_id);
    assert_eq!(
        state["active_document_id"],
        first.result["document"]["document_id"]
    );
    assert_eq!(
        state["active_document_path"],
        "docs/voice-secretary/first.md"
    );

    set_voice_active_document_id(&home, &group_id, &second.result["document"]["document_id"]);

    let index = ok(&home, "assistant_index", json!({"group_id":group_id}));
    assert_eq!(index.result["documents"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        index.result["documents"][0]["document_path"],
        "docs/voice-secretary/first.md"
    );
    assert!(
        index.result["documents_by_path"]
            .get("docs/voice-secretary/second.md")
            .is_none()
    );
    assert_eq!(
        index.result["active_document_path"],
        "docs/voice-secretary/first.md"
    );
    let repaired = load_voice_state(&home, &group_id);
    assert_eq!(
        repaired["active_document_id"],
        first.result["document"]["document_id"]
    );
    assert_eq!(
        repaired["active_document_path"],
        "docs/voice-secretary/first.md"
    );
    let archived_select = call(
        &home,
        "assistant_voice_document_select",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/second.md"
        }),
    );
    assert!(!archived_select.ok);

    let active = ok(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group_id}),
    );
    assert_eq!(active.result["documents"].as_array().map(Vec::len), Some(1));
    let all = ok(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group_id,"include_archived":true}),
    );
    assert_eq!(all.result["documents"].as_array().map(Vec::len), Some(2));

    ok(
        &home,
        "assistant_voice_document_archive",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/first.md"
        }),
    );
    set_voice_active_document_id(&home, &group_id, &first.result["document"]["document_id"]);
    let empty_index = ok(&home, "assistant_index", json!({"group_id":group_id}));
    assert_eq!(
        empty_index.result["documents"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(empty_index.result["active_document_id"], "");
    assert_eq!(empty_index.result["active_document_path"], "");
    let repaired_empty = load_voice_state(&home, &group_id);
    assert_eq!(repaired_empty["active_document_id"], "");
    assert_eq!(repaired_empty["active_document_path"], "");
}

#[test]
fn semantic_voice_inputs_do_not_create_meeting_transcripts() {
    let (temp, home, _store, group_id) = enabled_voice_group();
    let prompt = ok(
        &home,
        "assistant_voice_input_append",
        json!({
            "group_id":group_id,
            "kind":"prompt_refine",
            "request_id":"voice-prompt-storage",
            "voice_transcript":"补充验收标准",
            "composer_text":"检查方案"
        }),
    );
    assert!(prompt.result["segment"].is_null());
    assert!(prompt.result["segment_path"].is_null());
    assert_eq!(
        prompt.result["input_event"]["session_id"],
        "voice-secretary-prompt-refine"
    );

    let instruction = ok(
        &home,
        "assistant_voice_document_instruction",
        json!({
            "group_id":group_id,
            "instruction":"按负责人整理行动项"
        }),
    );
    assert!(instruction.result["segment"].is_null());
    assert_eq!(
        instruction.result["input_event"]["session_id"],
        "voice-secretary-user-instruction"
    );

    let state = load_voice_state(&home, &group_id);
    assert!(
        state["sessions"]
            .as_array()
            .is_some_and(|sessions| sessions.is_empty())
    );
    assert!(
        state["documents"]
            .as_array()
            .is_some_and(|documents| documents.is_empty())
    );
    assert!(
        !temp
            .path()
            .join("workspace/docs/voice-secretary/meeting.md")
            .exists()
    );
    let voice_root = home.root().join("voice-secretary").join(&group_id);
    assert!(voice_root.join("input_events.jsonl").is_file());
    assert!(!voice_root.join("voice-secretary-prompt-refine").exists());
    assert!(!voice_root.join("voice-secretary-user-instruction").exists());
}

#[test]
fn prompt_status_reads_the_requested_result_including_no_change() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    update_voice_state(&home, &group_id, |state| {
        state.insert("voice_prompt_drafts".into(), json!({
            "older-pending":{"request_id":"older-pending","status":"pending","draft_text":"Candidate A","updated_at":"2026-10-09T00:00:00Z"},
            "newer-pending":{"request_id":"newer-pending","status":"pending","draft_text":"Candidate B","updated_at":"2026-10-09T00:01:00Z"},
            "unchanged":{"request_id":"unchanged","status":"no_change","draft_text":"","updated_at":"2026-10-09T00:02:00Z"}
        }));
        Ok(())
    });
    let path = home
        .groups_dir()
        .join(&group_id)
        .join("state/assistants.json");
    let before = std::fs::read(&path).expect("canonical state");
    for (request_id, expected_status) in [("older-pending", "pending"), ("unchanged", "no_change")]
    {
        let result = ok(
            &home,
            "assistant_state",
            json!({
                "group_id":group_id,"assistant_id":"voice_secretary","prompt_request_id":request_id
            }),
        );
        assert_eq!(result.result["prompt_draft"]["request_id"], request_id);
        assert_eq!(result.result["prompt_draft"]["status"], expected_status);
    }
    let missing = ok(
        &home,
        "assistant_state",
        json!({
            "group_id":group_id,"prompt_request_id":"not-yet-completed"
        }),
    );
    assert!(
        missing.result["prompt_draft"].is_null(),
        "an explicit read must not return another request's candidate"
    );
    let default = ok(&home, "assistant_state", json!({"group_id":group_id}));
    assert_eq!(
        default.result["prompt_draft"]["request_id"],
        "newer-pending"
    );
    assert_eq!(
        std::fs::read(&path).expect("state unchanged by reads"),
        before
    );
}

#[test]
fn prompt_refine_instructions_match_the_published_task_action() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let tools: Vec<Value> =
        serde_json::from_str(include_str!("../../../../resources/mcp_tools.json"))
            .expect("published tool catalog");
    let actions = tools
        .iter()
        .find(|tool| tool["name"] == "cccc_voice_secretary_task")
        .expect("task tool")["inputSchema"]["properties"]["action"]["enum"]
        .as_array()
        .expect("published actions");
    for operation in [
        "append_to_composer_end",
        "replace",
        "replace_with_refined_prompt",
    ] {
        let response = ok(
            &home,
            "assistant_voice_input_append",
            json!({
                "group_id":group_id,"kind":"prompt_refine","request_id":operation,
                "composer_text":"Improve the proposal","operation":operation
            }),
        );
        let text = response.result["input_event"]["text"]
            .as_str()
            .expect("task input");
        let (_, output) = text
            .split_once("Required output:\n")
            .expect("required output");
        let action = output
            .split_once("action=\"")
            .expect("action")
            .1
            .split_once('"')
            .expect("action value")
            .0;
        assert!(
            actions.iter().any(|value| value.as_str() == Some(action)),
            "Prompt requests unpublished task action: {action}"
        );
        assert_eq!(action, "draft");
        assert!(output.contains(if operation == "append_to_composer_end" {
            "only the text to add"
        } else {
            "complete replacement prompt"
        }));
    }
}

#[test]
fn prompt_refine_rejects_empty_input_without_persisting_a_request() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let response = call(
        &home,
        "assistant_voice_input_append",
        json!({
            "group_id":group_id,
            "kind":"prompt_refine",
            "text":"",
            "voice_transcript":" ",
            "composer_text":"\n"
        }),
    );

    assert!(!response.ok);
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("empty_prompt_refine_input")
    );
    let state = load_voice_state(&home, &group_id);
    assert!(
        state
            .get("voice_prompt_requests")
            .is_none_or(|value| value.as_object().is_none_or(Map::is_empty))
    );
}

#[test]
fn legacy_voice_secretary_shape_is_migrated_to_canonical_runtime_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("legacy", "").expect("group");
    store.mutate(&group.group_id,|doc|{doc.extra.insert("assistants".into(),json!({"voice_secretary":{"assistant_id":"voice_secretary","enabled":true,"lifecycle":"idle","config":{"recognition_backend":"browser_asr"}}}));Ok(())}).expect("legacy state");
    let index = ok(&home, "assistant_state", json!({"group_id":group.group_id}));
    assert_eq!(index.result["assistant"]["enabled"], true);
    let legacy_alias = ok(&home, "assistant_index", json!({"group_id":group.group_id}));
    assert_eq!(legacy_alias.result["assistant"]["enabled"], true);
    ok(
        &home,
        "voice_secretary_settings_update",
        json!({"settings":{"config":{"recognition_language":"zh-CN"}}}),
    );
    let loaded = store.load(&group.group_id).expect("load");
    let state = &loaded.extra["assistants"];
    assert!(state.get("assistant").is_none());
    assert_eq!(
        load_voice_state(&home, &group.group_id)["assistant"]["config"]["recognition_language"],
        "zh-CN"
    );
    let runtime = load_voice_state(&home, &group.group_id);
    assert_eq!(runtime["assistant"]["lifecycle"], "idle");
}

#[test]
fn assistant_state_and_recording_lease_use_the_public_daemon_contract() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let state = ok(
        &home,
        "assistant_state",
        json!({"group_id":group_id,"assistant_id":"voice_secretary"}),
    );
    assert_eq!(state.result["assistant"]["assistant_id"], "voice_secretary");
    assert_eq!(state.result["recording_lease"], json!({}));

    let acquired = ok(
        &home,
        "assistant_voice_recording_lease",
        json!({"group_id":group_id,"action":"acquire","owner_id":"tab-a","ttl_seconds":30}),
    );
    let lease_id = acquired.result["lease_id"]
        .as_str()
        .expect("private lease token");
    assert!(acquired.result["lease"].get("lease_id").is_none());

    let conflict = call(
        &home,
        "assistant_voice_recording_lease",
        json!({"group_id":group_id,"action":"acquire","owner_id":"tab-b"}),
    );
    assert!(!conflict.ok);
    let conflict_error = conflict.error.expect("conflict error");
    assert_eq!(conflict_error.code, "assistant_voice_recording_busy");
    assert!(
        conflict_error.details["active_lease"]
            .get("lease_id")
            .is_none()
    );

    let stale = ok(
        &home,
        "assistant_voice_recording_lease",
        json!({"group_id":group_id,"action":"heartbeat","owner_id":"tab-a","lease_id":"stale"}),
    );
    assert_eq!(stale.result["lost"], true);
    let active = ok(
        &home,
        "assistant_state",
        json!({"group_id":group_id,"assistant_id":"voice_secretary"}),
    );
    assert_eq!(active.result["recording_lease"]["owner_id"], "tab-a");
    assert!(active.result["recording_lease"].get("lease_id").is_none());

    let heartbeat = ok(
        &home,
        "assistant_voice_recording_lease",
        json!({"group_id":group_id,"action":"heartbeat","owner_id":"tab-a","lease_id":lease_id}),
    );
    assert_eq!(heartbeat.result["lost"], false);
    assert_eq!(heartbeat.result["lease_id"], lease_id);
    let released = ok(
        &home,
        "assistant_voice_recording_lease",
        json!({"group_id":group_id,"action":"release","owner_id":"tab-a","lease_id":lease_id}),
    );
    assert_eq!(released.result["released"], true);
    let status = ok(
        &home,
        "assistant_voice_recording_lease",
        json!({"group_id":group_id,"action":"status"}),
    );
    assert_eq!(status.result["lease"], json!({}));
    assert!(
        home.root()
            .join("state/voice_secretary_recording_lease.json")
            .is_file()
    );
}

#[test]
fn disabled_voice_secretary_direct_dictation_heartbeat_preserves_lease_scope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("direct dictation", "").expect("group");
    let acquired = ok(
        &home,
        "assistant_voice_recording_lease",
        json!({
            "group_id":group.group_id,
            "action":"acquire",
            "owner_id":"tab-direct",
            "capture_mode":"prompt",
            "recognition_backend":"assistant_service_local_asr",
            "dispatch_target":"composer"
        }),
    );
    let lease_id = acquired.result["lease_id"]
        .as_str()
        .expect("private lease token");

    let heartbeat = ok(
        &home,
        "assistant_voice_recording_lease",
        json!({
            "group_id":group.group_id,
            "action":"heartbeat",
            "owner_id":"tab-direct",
            "lease_id":lease_id
        }),
    );
    assert_eq!(heartbeat.result["acquired"], true);
    assert_eq!(heartbeat.result["lease"]["capture_mode"], "prompt");
    assert_eq!(
        heartbeat.result["lease"]["recognition_backend"],
        "assistant_service_local_asr"
    );
    assert_eq!(heartbeat.result["lease"]["dispatch_target"], "composer");
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
    let args = json!({"group_id":group.group_id,"by":"user","session_id":"retry-session","segment_id":"retry-segment","text":"必须可靠送达","document_path":"docs/voice-secretary/retry.md","is_final":true});
    let failed = call(&home, "assistant_voice_transcript_append", args.clone());
    assert!(!failed.ok);
    let state = load_voice_state(&home, &group.group_id);
    assert_eq!(state["input_latest_seq"].as_u64().unwrap_or(0), 0);
    assert!(
        !home
            .root()
            .join("voice-secretary")
            .join(&group.group_id)
            .join("input_events.jsonl")
            .exists()
    );

    std::fs::remove_file(workspace.join("docs")).expect("remove blocker");
    let retried = ok(&home, "assistant_voice_transcript_append", args);
    assert_eq!(retried.result["input_event_created"], true);
    let source_path = home
        .root()
        .join("voice-secretary")
        .join(&group.group_id)
        .join("input_events.jsonl");
    let rows = std::fs::read_to_string(source_path)
        .expect("source log")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("source"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["secretary_target"].is_object());
    assert_eq!(retried.result["secretary_processing_deferred"], true);
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
fn global_secretary_enablement_does_not_launch_the_foreman_runtime() {
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
    let response = call(&home, "assistant_state", json!({"group_id":group.group_id}));
    assert!(response.ok);
    assert!(response.result.get("actor_started").is_none());
    let saved = store.load(&group.group_id).expect("saved group");
    assert!(
        !saved
            .actors
            .iter()
            .any(|actor| actor.id == "voice-secretary")
    );
    assert_eq!(response.result["assistant"]["enabled"], true);
}

#[cfg(unix)]
#[test]
fn durable_log_remains_idempotent_after_session_window_is_trimmed() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let args = json!({"group_id":group_id,"by":"user","session_id":"long-session","segment_id":"old-segment","text":"只处理一次","document_path":"docs/voice-secretary/long.md","is_final":true});
    ok(&home, "assistant_voice_transcript_append", args.clone());
    update_voice_state(&home, &group_id, |state| {
        state["sessions"][0]["segments"] = json!([]);
        Ok(())
    });
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
        .join("input_events.jsonl");
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
fn creating_empty_document_writes_its_workspace_file() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "title":"Empty notes",
            "create_new":true,
            "by":"user"
        }),
    );
    let document = &saved.result["document"];
    let absolute_path = document["absolute_path"].as_str().expect("absolute path");

    assert!(std::path::Path::new(absolute_path).is_file());
    assert_eq!(
        std::fs::read_to_string(absolute_path).expect("read empty document"),
        ""
    );
    let listed = ok(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group_id}),
    );
    assert_eq!(listed.result["documents"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        listed.result["documents"][0]["document_id"],
        document["document_id"]
    );
}

#[test]
fn saving_unindexed_existing_document_without_content_preserves_file() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let group = store.load(&group_id).expect("group");
    let workspace = std::path::Path::new(&group.scopes[0].url);
    let document_path = "docs/voice-secretary/external.md";
    let absolute_path = workspace.join(document_path);
    std::fs::create_dir_all(absolute_path.parent().expect("document parent"))
        .expect("create document parent");
    std::fs::write(&absolute_path, "# External\n\npreserve me\n").expect("write external document");

    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "document_path":document_path,
            "title":"External notes"
        }),
    );

    assert_eq!(
        std::fs::read_to_string(&absolute_path).expect("read external document"),
        "# External\n\npreserve me\n"
    );
    assert_eq!(
        saved.result["document"]["content"],
        "# External\n\npreserve me\n"
    );
    assert_eq!(saved.result["document"]["revision_count"], 1);
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

#[test]
fn listing_reconciles_repository_document_edits_once() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/reconciled.md",
            "content":"first"
        }),
    );
    let absolute_path = saved.result["document"]["absolute_path"]
        .as_str()
        .expect("absolute path");
    std::fs::write(absolute_path, "# Updated\n\n- 新内容\n").expect("external document edit");

    let first = ok(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    let document = &first.result["documents"][0];
    assert_eq!(document["content"], "# Updated\n\n- 新内容\n");
    assert_eq!(document["content_chars"], 17);
    assert_eq!(document["revision_count"], 2);
    assert_ne!(
        document["content_sha256"],
        saved.result["document"]["content_sha256"]
    );

    let second = ok(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(second.result["documents"][0]["revision_count"], 2);
    assert_eq!(
        load_voice_state(&home, &group_id)["documents"][0]["content"],
        "# Updated\n\n- 新内容\n"
    );

    let events = ledger::read_all(&store.ledger_path(&group_id).expect("ledger")).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.kind == "assistant.voice.document" && event.data["action"] == "reconciled"
            })
            .count(),
        1
    );
}

#[test]
fn concurrent_document_lists_reconcile_only_once() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/concurrent.md",
            "content":"before"
        }),
    );
    std::fs::write(
        saved.result["document"]["absolute_path"]
            .as_str()
            .expect("absolute path"),
        "after",
    )
    .expect("external edit");

    let handles = (0..2)
        .map(|_| {
            let home = home.clone();
            let group_id = group_id.clone();
            std::thread::spawn(move || {
                ok(
                    &home,
                    "assistant_voice_document_list",
                    json!({"group_id":group_id,"by":"user"}),
                )
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        let response = handle.join().expect("list thread");
        assert_eq!(response.result["documents"][0]["content"], "after");
        assert_eq!(response.result["documents"][0]["revision_count"], 2);
    }

    let events = ledger::read_all(&store.ledger_path(&group_id).expect("ledger")).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.kind == "assistant.voice.document" && event.data["action"] == "reconciled"
            })
            .count(),
        1
    );
}

#[test]
fn missing_repository_document_does_not_clear_registry_content() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({
            "group_id":group_id,
            "document_path":"docs/voice-secretary/missing.md",
            "content":"preserve me"
        }),
    );
    std::fs::remove_file(
        saved.result["document"]["absolute_path"]
            .as_str()
            .expect("absolute path"),
    )
    .expect("remove document");

    let listed = ok(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group_id}),
    );
    assert_eq!(listed.result["documents"][0]["content"], "preserve me");
    assert_eq!(listed.result["documents"][0]["revision_count"], 1);
    assert_eq!(
        load_voice_state(&home, &group_id)["documents"][0]["content"],
        "preserve me"
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
// Included by the crate-level integration test harness.
