use cccc_contracts::{
    Actor, ActorRole, ActorRuntime, DaemonRequest, DaemonResponse, Event, RunnerKind,
};
use cccc_core::{GroupStore, HomeLayout, Scope, ledger, ledger_archive};
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
    assert_eq!(
        enabled.result["assistant"]["health"]["actor"]["configured"],
        true
    );
    assert_eq!(
        enabled.result["assistant"]["health"]["actor"]["running"],
        false
    );
    assert_eq!(enabled.result["assistant"]["lifecycle"], "idle");
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
    let assistant_state = &store.load(&group.group_id).expect("load").extra["assistants"];
    assert_eq!(assistant_state["sessions"][0]["capture_mode"], "document");

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
fn prompt_refine_round_trip_uses_distinct_input_and_draft_operations() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let input = ok(
        &home,
        "assistant_voice_input_append",
        json!({
            "group_id":group_id,
            "kind":"prompt_refine",
            "request_id":"voice-prompt-test",
            "text":"",
            "voice_transcript":"  补充风险和验收标准  ",
            "composer_text":"检查这个方案",
            "operation":"append_to_composer_end",
            "composer_snapshot_hash":"snapshot-1",
            "composer_context":{"recent_chat_excerpt":"需要补回滚方案"}
        }),
    );
    assert_eq!(input.result["request_id"], "voice-prompt-test");
    assert_eq!(
        input.result["input_event"]["request_id"],
        "voice-prompt-test"
    );
    assert_eq!(
        input.result["input_event"]["metadata"]["target_kind"],
        "composer"
    );
    assert_eq!(
        input.result["input_event"]["metadata"]["operation"],
        "append_to_composer_end"
    );
    let input_text = input.result["input_event"]["text"]
        .as_str()
        .expect("input text");
    assert!(input_text.contains("Target: composer"));
    assert!(input_text.contains("Request id: voice-prompt-test"));
    assert!(input_text.contains("检查这个方案"));
    assert!(input_text.contains("补充风险和验收标准"));
    assert!(input_text.contains("需要补回滚方案"));

    let notifications_before = ledger::read_all(&store.ledger_path(&group_id).expect("ledger"))
        .expect("events")
        .into_iter()
        .filter(|event| {
            event.kind == "system.notify" && event.data["kind"] == "voice_secretary_input"
        })
        .count();
    let submit = ok(
        &home,
        "assistant_voice_prompt_draft_submit",
        json!({
            "group_id":group_id,
            "by":"assistant:voice_secretary",
            "request_id":"voice-prompt-test",
            "draft_text":"请检查这个方案，补充风险、验收标准和回滚路径。"
        }),
    );
    assert_eq!(submit.result["prompt_draft"]["status"], "pending");
    assert_eq!(
        submit.result["prompt_draft"]["operation"],
        "append_to_composer_end"
    );
    assert_eq!(
        submit.result["prompt_draft"]["composer_snapshot_hash"],
        "snapshot-1"
    );
    assert!(submit.result.get("input_event").is_none());

    let state = &store.load(&group_id).expect("load").extra["assistants"];
    assert_eq!(
        state["voice_prompt_requests"]["voice-prompt-test"]["composer_text"],
        "检查这个方案"
    );
    assert_eq!(
        state["voice_prompt_drafts"]["voice-prompt-test"]["draft_text"],
        "请检查这个方案，补充风险、验收标准和回滚路径。"
    );
    let notifications_after = ledger::read_all(&store.ledger_path(&group_id).expect("ledger"))
        .expect("events")
        .into_iter()
        .filter(|event| {
            event.kind == "system.notify" && event.data["kind"] == "voice_secretary_input"
        })
        .count();
    assert_eq!(notifications_after, notifications_before);
}

#[test]
fn prompt_refine_reused_request_id_delivers_each_append_and_deduplicates_retries() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let first_args = json!({
        "group_id":group_id,
        "kind":"prompt_refine",
        "request_id":"voice-prompt-reused",
        "input_append_id":"voice-prompt-append-one",
        "voice_transcript":"补充风险",
        "composer_text":"检查方案"
    });
    let first = ok(&home, "assistant_voice_input_append", first_args.clone());
    assert_eq!(first.result["input_event_created"], true);
    assert_eq!(first.result["input_append_id"], "voice-prompt-append-one");

    ok(
        &home,
        "assistant_voice_prompt_draft_submit",
        json!({
            "group_id":group_id,
            "by":"assistant:voice_secretary",
            "request_id":"voice-prompt-reused",
            "draft_text":"补充风险后的方案"
        }),
    );
    let retry = ok(&home, "assistant_voice_input_append", first_args);
    assert_eq!(retry.result["input_event_created"], false);
    assert_eq!(
        store.load(&group_id).expect("load").extra["assistants"]["voice_prompt_drafts"]["voice-prompt-reused"]
            ["status"],
        "pending"
    );

    let second = ok(
        &home,
        "assistant_voice_input_append",
        json!({
            "group_id":group_id,
            "kind":"prompt_refine",
            "request_id":"voice-prompt-reused",
            "input_append_id":"voice-prompt-append-two",
            "voice_transcript":"再补充验收标准",
            "composer_text":"检查方案"
        }),
    );
    assert_eq!(second.result["input_event_created"], true);
    assert_ne!(
        first.result["input_event"]["segment_id"],
        second.result["input_event"]["segment_id"]
    );
    let second_text = second.result["input_event"]["text"]
        .as_str()
        .expect("second input text");
    assert!(second_text.contains("补充风险"));
    assert!(second_text.contains("再补充验收标准"));

    let state = &store.load(&group_id).expect("load").extra["assistants"];
    assert_eq!(
        state["voice_prompt_requests"]["voice-prompt-reused"]["voice_transcripts"],
        json!(["补充风险", "再补充验收标准"])
    );
    assert_eq!(
        state["voice_prompt_drafts"]["voice-prompt-reused"]["status"],
        "stale"
    );
    let events =
        ledger::read_all(&store.ledger_path(&group_id).expect("ledger path")).expect("ledger");
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.kind == "assistant.voice.input"
                    && event.data["request_id"] == "voice-prompt-reused"
            })
            .count(),
        2
    );
}

#[test]
fn voice_instruction_round_trip_persists_and_reports_the_ask_reply() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let args = json!({
        "group_id":group_id,
        "kind":"voice_instruction",
        "request_id":"voice-ask-weather",
        "input_append_id":"voice-ask-weather-input",
        "instruction":"厦门天气怎么样？",
        "language":"mixed",
        "trigger":{
            "trigger_kind":"service_voice_instruction",
            "target_kind":"secretary"
        }
    });
    let input = ok(&home, "assistant_voice_document_instruction", args.clone());
    assert_eq!(input.result["request_id"], "voice-ask-weather");
    assert_eq!(input.result["input_event_created"], true);
    assert_eq!(input.result["ask_request"]["status"], "pending");
    assert_eq!(
        input.result["input_event"]["text"],
        "Task:\n厦门天气怎么样？"
    );
    assert_eq!(
        input.result["input_event"]["metadata"]["target_kind"],
        "secretary"
    );
    assert_eq!(
        input.result["input_event"]["metadata"]["request_id"],
        "voice-ask-weather"
    );
    assert_eq!(
        input.result["input_event"]["trigger"]["intent_hint"],
        "secretary_task"
    );
    assert!(input.result["input_event"]["trigger"]["instruction_policy"].is_object());

    let retry = ok(&home, "assistant_voice_document_instruction", args);
    assert_eq!(retry.result["input_event_created"], false);
    let state = &store.load(&group_id).expect("load").extra["assistants"];
    assert_eq!(state["ask_requests"].as_array().map(Vec::len), Some(1));
    assert_eq!(state["ask_requests"][0]["request_id"], "voice-ask-weather");
    assert_eq!(state["assistant"]["lifecycle"], "working");

    let feedback_args = json!({
        "group_id":group_id,
        "by":"voice-secretary",
        "request_id":"voice-ask-weather",
        "status":"done",
        "reply_text":"厦门今天多云，外出注意防晒。",
        "source_urls":["https://www.weather.com.cn/fujian/xiamen/"]
    });
    let feedback = ok(
        &home,
        "assistant_voice_instruction_feedback",
        feedback_args.clone(),
    );
    assert_eq!(feedback.result["ask_request"]["status"], "done");
    assert_eq!(
        feedback.result["ask_request"]["reply_text"],
        "厦门今天多云，外出注意防晒。"
    );
    assert_eq!(feedback.result["assistant"]["lifecycle"], "idle");
    assert_eq!(feedback.result["event"]["kind"], "assistant.voice.request");

    ok(&home, "assistant_voice_instruction_feedback", feedback_args);
    let events =
        ledger::read_all(&store.ledger_path(&group_id).expect("ledger path")).expect("ledger");
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.kind == "assistant.voice.request"
                    && event.data["request_id"] == "voice-ask-weather"
            })
            .count(),
        1
    );

    let index = ok(&home, "assistant_index", json!({"group_id":group_id}));
    assert_eq!(index.result["ask_requests"][0]["status"], "done");
    assert_eq!(
        index.result["latest_ask_request"]["reply_text"],
        "厦门今天多云，外出注意防晒。"
    );
}

#[test]
fn voice_instruction_feedback_requires_an_existing_request_and_secretary_actor() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let forbidden = call(
        &home,
        "assistant_voice_instruction_feedback",
        json!({
            "group_id":group_id,
            "by":"user",
            "request_id":"voice-ask-missing",
            "status":"done",
            "reply_text":"不应写入"
        }),
    );
    assert!(!forbidden.ok);
    assert_eq!(
        forbidden.error.as_ref().map(|error| error.code.as_str()),
        Some("assistant_voice_instruction_feedback_forbidden")
    );

    let missing = call(
        &home,
        "assistant_voice_instruction_feedback",
        json!({
            "group_id":group_id,
            "by":"voice-secretary",
            "request_id":"voice-ask-missing",
            "status":"done",
            "reply_text":"不存在"
        }),
    );
    assert!(!missing.ok);
    assert_eq!(
        missing.error.as_ref().map(|error| error.code.as_str()),
        Some("voice_ask_request_not_found")
    );
}

#[test]
fn document_instruction_targets_an_existing_document_and_requires_a_report() {
    let (_temp, home, store, group_id) = enabled_voice_group();
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
    let state = &store.load(&group_id).expect("load").extra["assistants"];
    assert_eq!(state["documents"].as_array().map(Vec::len), Some(1));
    assert_eq!(state["ask_requests"][0]["target_kind"], "document");
    assert_eq!(
        state["ask_requests"][0]["document_path"],
        "docs/voice-secretary/release.md"
    );
}

#[test]
fn archiving_document_removes_it_from_index_and_selects_the_next_active_document() {
    let (_temp, home, store, group_id) = enabled_voice_group();
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
        store.load(&group_id).expect("load").extra["assistants"]["active_document_id"],
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

    let state = &store.load(&group_id).expect("load").extra["assistants"];
    assert_eq!(
        state["active_document_id"],
        first.result["document"]["document_id"]
    );
    assert_eq!(
        state["active_document_path"],
        "docs/voice-secretary/first.md"
    );

    cccc_core::integration_state::group_update(&store, &group_id, "assistants", |value| {
        value["active_document_id"] = second.result["document"]["document_id"].clone();
        value["active_document_path"] = json!("docs/voice-secretary/second.md");
        Ok(())
    })
    .expect("seed legacy archived active target");

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
    let repaired = &store.load(&group_id).expect("load").extra["assistants"];
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
    cccc_core::integration_state::group_update(&store, &group_id, "assistants", |value| {
        value["active_document_id"] = first.result["document"]["document_id"].clone();
        value["active_document_path"] = json!("docs/voice-secretary/first.md");
        Ok(())
    })
    .expect("seed legacy final archived target");
    let empty_index = ok(&home, "assistant_index", json!({"group_id":group_id}));
    assert_eq!(
        empty_index.result["documents"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(empty_index.result["active_document_id"], "");
    assert_eq!(empty_index.result["active_document_path"], "");
    let repaired_empty = &store.load(&group_id).expect("load").extra["assistants"];
    assert_eq!(repaired_empty["active_document_id"], "");
    assert_eq!(repaired_empty["active_document_path"], "");
}

#[test]
fn semantic_voice_inputs_do_not_create_meeting_transcripts() {
    let (temp, home, store, group_id) = enabled_voice_group();
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

    let state = &store.load(&group_id).expect("load").extra["assistants"];
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
    assert!(voice_root.join("inputs.jsonl").is_file());
    assert!(!voice_root.join("voice-secretary-prompt-refine").exists());
    assert!(!voice_root.join("voice-secretary-user-instruction").exists());
}

#[test]
fn prompt_refine_rejects_empty_input_without_persisting_a_request() {
    let (_temp, home, store, group_id) = enabled_voice_group();
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
    let state = &store.load(&group_id).expect("load").extra["assistants"];
    assert!(
        state
            .get("voice_prompt_requests")
            .is_none_or(|value| value.as_object().is_none_or(Map::is_empty))
    );
}

#[test]
fn prompt_draft_submit_requires_existing_request_and_supports_no_op_and_ack() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let missing = call(
        &home,
        "assistant_voice_prompt_draft_submit",
        json!({
            "group_id":group_id,
            "by":"voice-secretary",
            "request_id":"missing",
            "draft_text":"不会保存"
        }),
    );
    assert!(!missing.ok);
    assert_eq!(
        missing.error.as_ref().map(|error| error.code.as_str()),
        Some("prompt_request_not_found")
    );

    ok(
        &home,
        "assistant_voice_input_append",
        json!({
            "group_id":group_id,
            "kind":"prompt_refine",
            "request_id":"voice-prompt-noop",
            "composer_text":"请优化这段提示词",
            "operation":"replace_with_refined_prompt"
        }),
    );
    let no_op = ok(
        &home,
        "assistant_voice_prompt_draft_submit",
        json!({
            "group_id":group_id,
            "by":"voice-secretary",
            "request_id":"voice-prompt-noop",
            "no_op":true,
            "draft_text":"该占位文本必须被忽略"
        }),
    );
    assert_eq!(no_op.result["prompt_draft"]["status"], "no_change");
    assert_eq!(no_op.result["prompt_draft"]["draft_text"], "");
    assert!(store.load(&group_id).expect("load").extra["assistants"]["prompt_draft"].is_null());

    ok(
        &home,
        "assistant_voice_input_append",
        json!({
            "group_id":group_id,
            "kind":"prompt_refine",
            "request_id":"voice-prompt-ack",
            "composer_text":"待替换内容"
        }),
    );
    ok(
        &home,
        "assistant_voice_prompt_draft_submit",
        json!({
            "group_id":group_id,
            "by":"assistant:voice_secretary",
            "request_id":"voice-prompt-ack",
            "draft_text":"替换后的内容"
        }),
    );
    let ack = ok(
        &home,
        "assistant_voice_prompt_draft_ack",
        json!({
            "group_id":group_id,
            "request_id":"voice-prompt-ack",
            "status":"applied"
        }),
    );
    assert_eq!(ack.result["prompt_draft"]["status"], "applied");
    let state = &store.load(&group_id).expect("load").extra["assistants"];
    assert!(state["prompt_draft"].is_null());
    assert_eq!(
        state["voice_prompt_drafts"]["voice-prompt-ack"]["status"],
        "applied"
    );
}

#[test]
fn voice_request_skips_empty_modern_alias_and_uses_request_text() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let response = ok(
        &home,
        "assistant_voice_request",
        json!({
            "group_id":group_id,
            "text":"",
            "instruction":" ",
            "request_text":"  整理行动项  ",
            "target":"@foreman"
        }),
    );

    assert_eq!(response.result["request"]["request_text"], "整理行动项");
    assert_eq!(
        response.result["notify_event"]["data"]["text"],
        "整理行动项"
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
fn enabling_voice_secretary_rolls_back_runtime_start_failure() {
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
    let response = call(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    assert!(!response.ok);
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("voice_secretary_start_failed")
    );
    let restored = store.load(&group.group_id).expect("restored group");
    assert!(
        !restored
            .actors
            .iter()
            .any(|actor| actor.id == "voice-secretary")
    );
    assert!(
        !restored
            .extra
            .get("assistants")
            .and_then(|state| state.get("assistant"))
            .and_then(|assistant| assistant.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );
}

#[cfg(unix)]
#[test]
fn headless_voice_secretary_health_tracks_its_local_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    let fake_app_server = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      ;;
    *'"method":"thread/start"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"thread":{"id":"thread-1"}}}\n' "$id"
      ;;
  esac
done
"#;
    store
        .mutate(&group.group_id, |doc| {
            let mut foreman = Actor::new("foreman");
            foreman.role = Some(ActorRole::Foreman);
            foreman.runtime = ActorRuntime::Codex;
            foreman.runner = RunnerKind::Headless;
            foreman.command = vec!["sh".into(), "-c".into(), fake_app_server.into()];
            doc.actors.push(foreman);
            doc.running = true;
            Ok(())
        })
        .expect("running headless group");

    let enabled = ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    assert_eq!(
        enabled.result["assistant"]["health"]["actor"]["running"],
        true
    );
    assert!(
        enabled.result["assistant"]["health"]["actor"]["pid"]
            .as_u64()
            .is_some()
    );

    ok(
        &home,
        "actor_stop",
        json!({"group_id":group.group_id,"actor_id":"voice-secretary","by":"user"}),
    );
    store
        .mutate(&group.group_id, |doc| {
            doc.running = true;
            Ok(())
        })
        .expect("keep group running after provider exit");
    let stopped = ok(&home, "assistant_index", json!({"group_id":group.group_id}));
    assert_eq!(
        stopped.result["assistant"]["health"]["actor"]["running"],
        false
    );
    assert_eq!(
        stopped.result["assistant"]["health"]["actor"]["pid"],
        Value::Null
    );
    assert_eq!(stopped.result["assistant"]["lifecycle"], "failed");
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
fn voice_input_ignores_legacy_events_missing_segment_identity() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let ledger_path = store.ledger_path(&group_id).expect("ledger");
    let mut legacy_input = Event::new("assistant.voice.input", &group_id);
    legacy_input.data = json!({
        "assistant_id":"voice_secretary",
        "input_kind":"asr_transcript",
        "input_preview":"旧版事件没有录音段标识"
    })
    .as_object()
    .cloned()
    .expect("legacy input data");
    ledger::append(&ledger_path, &legacy_input).expect("append legacy input");
    let mut legacy_notice = Event::new("system.notify", &group_id);
    legacy_notice.data = json!({
        "kind":"voice_secretary_input",
        "context":{"input_envelope":{"input_id":"legacy-input"}}
    })
    .as_object()
    .cloned()
    .expect("legacy notice data");
    ledger::append(&ledger_path, &legacy_notice).expect("append legacy notice");
    ledger_archive::compact(&home, &group_id, "legacy voice fixture")
        .expect("compact ledger")
        .expect("archived segment");

    let response = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group_id,
            "by":"user",
            "session_id":"current-session",
            "segment_id":"current-segment",
            "text":"新语音仍应正常投递",
            "document_path":"docs/voice-secretary/legacy-compatible.md",
            "is_final":true
        }),
    );

    assert_eq!(response.result["input_event_created"], true);
    assert_eq!(response.result["input_notify_emitted"], true);
    let events = ledger::read_all(&ledger_path).expect("events");
    assert!(events.iter().any(|event| {
        event.kind == "assistant.voice.input"
            && event.data.get("session_id").and_then(Value::as_str) == Some("current-session")
            && event.data.get("segment_id").and_then(Value::as_str) == Some("current-segment")
    }));
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
        json!({"group_id":group_id,"by":"voice-secretary"}),
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
        json!({"group_id":group_id,"by":"voice-secretary"}),
    );
    assert_eq!(second.result["documents"][0]["revision_count"], 2);
    assert_eq!(
        store.load(&group_id).expect("load").extra["assistants"]["documents"][0]["content"],
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
                    json!({"group_id":group_id,"by":"voice-secretary"}),
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
    let (_temp, home, store, group_id) = enabled_voice_group();
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
        store.load(&group_id).expect("load").extra["assistants"]["documents"][0]["content"],
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
// Included by the crate-level integration test harness.
