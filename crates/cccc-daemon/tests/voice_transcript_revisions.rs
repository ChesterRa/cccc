use cccc_contracts::{Actor, ActorRole, DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, Scope, assistant_state, ledger};
use serde_json::{Map, Value, json};

#[test]
fn final_revision_supersedes_live_projection_without_deleting_raw_segments() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice revisions", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            let mut foreman = Actor::new("foreman");
            foreman.role = Some(ActorRole::Foreman);
            foreman.command = vec!["true".into()];
            group.actors.push(foreman);
            group.scopes.push(Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("seed group");
    ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    let short_final = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group.group_id,"session_id":"session-short","segment_id":"final-asr",
            "document_path":"docs/voice-secretary/meeting.md","text":"短录音终稿。",
            "is_final":true,"transcript_stage":"final","revision_only":true,
            "supersede_stage":"live","source_model_id":"sense-voice",
            "trigger":{"recognition_backend":"assistant_service_local_asr_final"}
        }),
    );
    assert_eq!(short_final.result["input_event_created"], true);
    assert_eq!(
        short_final.result["input_event"]["text"], "短录音终稿。",
        "a final revision must become the first semantic input when no live input exists"
    );
    let short_late_live = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group.group_id,"session_id":"session-short","segment_id":"late-live",
            "document_path":"docs/voice-secretary/meeting.md","text":"短录音迟到实时文本",
            "is_final":true,"transcript_stage":"live",
            "trigger":{"recognition_backend":"assistant_service_local_asr_streaming"}
        }),
    );
    assert_eq!(short_late_live.result["input_event_created"], false);
    assert!(short_late_live.result["input_event"].is_null());

    for (segment_id, text) in [("live-1", "没有标点的实时一"), ("live-2", "实时二")] {
        ok(
            &home,
            "assistant_voice_transcript_append",
            json!({
                "group_id":group.group_id,"session_id":"session-1","segment_id":segment_id,
                "document_path":"docs/voice-secretary/meeting.md","text":text,"is_final":true,
                "transcript_stage":"live",
                "trigger":{"recognition_backend":"assistant_service_local_asr_streaming"}
            }),
        );
    }
    let final_revision = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group.group_id,"session_id":"session-1","segment_id":"final-asr",
            "document_path":"docs/voice-secretary/meeting.md","text":"最终文本。","is_final":true,
            "transcript_stage":"final","revision_only":true,"supersede_stage":"live",
            "source_model_id":"sense-voice",
            "trigger":{"recognition_backend":"assistant_service_local_asr_final"}
        }),
    );
    let late_live = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group.group_id,"session_id":"session-1","segment_id":"late-live",
            "document_path":"docs/voice-secretary/meeting.md","text":"迟到的实时文本",
            "is_final":true,"transcript_stage":"live",
            "trigger":{"recognition_backend":"assistant_service_local_asr_streaming"}
        }),
    );

    assert_eq!(final_revision.result["input_event_created"], false);
    assert!(final_revision.result["input_event"].is_null());
    assert_eq!(late_live.result["input_event_created"], false);
    assert!(late_live.result["input_event"].is_null());
    ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group.group_id,"session_id":"session-2","segment_id":"live-other",
            "document_path":"docs/voice-secretary/meeting.md","text":"第二场实时文本",
            "is_final":true,"transcript_stage":"live",
            "trigger":{"recognition_backend":"assistant_service_local_asr_streaming"}
        }),
    );
    let state = assistant_state::load(&home, &group.group_id).expect("assistant state");
    let session = state["sessions"]
        .as_array()
        .and_then(|sessions| {
            sessions
                .iter()
                .find(|item| item["session_id"] == "session-1")
        })
        .expect("session");
    let segments = session["segments"].as_array().expect("segments");
    assert_eq!(segments.len(), 4, "raw revisions must remain durable");
    assert_eq!(session["transcript"], "最终文本。");
    assert_eq!(
        segments[2]["supersedes_segment_ids"],
        json!(["live-1", "live-2"])
    );
    assert_eq!(segments[2]["source_model_id"], "sense-voice");
    let session_view = ok(
        &home,
        "assistant_state",
        json!({"group_id":group.group_id,"view":"voice_session","session_id":"session-1"}),
    );
    assert_eq!(session_view.result["session"]["transcript"], "最终文本。");
    let document_view = ok(
        &home,
        "assistant_state",
        json!({
            "group_id":group.group_id,"view":"voice_session",
            "document_path":"docs/voice-secretary/meeting.md"
        }),
    );
    assert_eq!(
        document_view.result["session"]["transcript"],
        "短录音终稿。\n最终文本。\n第二场实时文本"
    );

    let events =
        ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger")).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "assistant.voice.input")
            .count(),
        4,
        "final revisions and late live checkpoints must not enqueue duplicate secretary work"
    );
}

fn ok(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}
