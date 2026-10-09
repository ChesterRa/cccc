use super::*;

#[test]
fn global_voice_defaults_apply_to_every_group_without_creating_runtime_work() {
    let (_temp, home, groups, group_id) = enabled_voice_group();
    let other = groups.create("other", "").expect("group");
    groups
        .mutate(&group_id, |doc| {
            doc.running = true;
            Ok(())
        })
        .expect("running");
    let saved = ok(
        &home,
        "voice_secretary_settings_update",
        json!({"settings":{"config":{
            "recognition_backend":"external_provider_asr","external_asr_provider":"volcengine",
            "recognition_language":"ja-JP","auto_document_max_window_seconds":45,"guidance":"Preserve budget numbers"
        }}}),
    );
    assert_eq!(
        saved.result["settings"]["config"]["recognition_language"],
        "ja-JP"
    );
    assert_eq!(saved.result["configured"], false);
    for group in [&group_id, &other.group_id] {
        let state = ok(&home, "assistant_state", json!({"group_id":group}));
        assert_eq!(state.result["assistant"]["enabled"], true);
        assert_eq!(
            state.result["assistant"]["config"]["recognition_language"],
            "ja-JP"
        );
        assert_eq!(
            state.result["assistant"]["config"]["auto_document_max_window_seconds"],
            45
        );
        let doc = groups.load(group).expect("group");
        assert!(doc.actors.iter().all(|actor| actor.id != "voice-secretary"));
        assert!(doc.extra.get("assistants").is_none());
    }
    assert!(
        !call(
            &home,
            "assistant_settings_update",
            json!({"group_id":group_id,"patch":{"enabled":false}})
        )
        .ok
    );
}

#[test]
fn global_work_rules_are_captured_by_new_tasks_without_overwriting_old_actor_prompts() {
    let (_temp, home, groups, group_id) = enabled_voice_group();
    cccc_core::group_prompts::write_help(
        &groups,
        &group_id,
        "## @voice_secretary\n\nObsolete Actor instructions\n",
    )
    .expect("old prompt retained");
    ok(
        &home,
        "voice_secretary_settings_update",
        json!({"settings":{"config":{"guidance":"Preserve budget numbers"}}}),
    );
    let input = ok(
        &home,
        "assistant_voice_document_instruction",
        json!({"group_id":group_id,"instruction":"Organize the budget","input_append_id":"rules"}),
    );
    assert_eq!(
        input.result["input_event"]["secretary_guidance"],
        "Preserve budget numbers"
    );
    let invalid = call(
        &home,
        "voice_secretary_settings_update",
        json!({"settings":{"config":{"guidance":true}}}),
    );
    assert!(!invalid.ok);
    assert_eq!(
        cccc_core::settings::load(&home)
            .expect("settings")
            .voice_secretary
            .config
            .guidance,
        "Preserve budget numbers"
    );
    ok(
        &home,
        "voice_secretary_settings_update",
        json!({"settings":{"config":{"guidance":" "}}}),
    );
    let restored = ok(
        &home,
        "assistant_voice_document_instruction",
        json!({"group_id":group_id,"instruction":"Use the default rules","input_append_id":"default-rules"}),
    );
    assert_eq!(
        restored.result["input_event"]["secretary_guidance"],
        include_str!("../../../../../resources/voice-secretary-guidance.md")
    );
    assert!(
        cccc_core::group_prompts::read_help(&groups, &group_id)
            .expect("prompt")
            .content
            .expect("content")
            .contains("Obsolete Actor instructions")
    );
}

#[test]
fn invalid_global_preferences_do_not_change_saved_configuration() {
    let (_temp, home, _, _) = enabled_voice_group();
    for config in [
        json!({"recognition_backend":"mock"}),
        json!({"auto_document_max_window_seconds":0}),
        json!({"external_asr_provider":"unknown"}),
        json!({"guidance":true}),
    ] {
        assert!(
            !call(
                &home,
                "voice_secretary_settings_update",
                json!({"settings":{"config":config}})
            )
            .ok
        );
        assert_eq!(
            cccc_core::settings::load(&home)
                .expect("settings")
                .voice_secretary,
            Default::default()
        );
    }
}

#[test]
fn removed_actor_protocol_cannot_be_used_to_project_a_result() {
    let (_temp, home, _, group_id) = enabled_voice_group();
    for op in [
        "assistant_settings_update",
        "assistant_status_update",
        "assistant_voice_document_input_read",
        "assistant_voice_prompt_draft_submit",
        "assistant_voice_instruction_feedback",
        "assistant_voice_request",
    ] {
        assert!(
            !call(
                &home,
                op,
                json!({"group_id":group_id,"by":"assistant:voice_secretary"})
            )
            .ok,
            "obsolete operation {op}"
        );
    }
}

#[test]
fn disabled_recording_checkpoints_still_process_the_final_document_transcript() {
    let (_temp, home, _, group_id) = enabled_voice_group();
    ok(
        &home,
        "voice_secretary_settings_update",
        json!({"settings":{"config":{"auto_document_max_window_seconds":null}}}),
    );
    let appended = ok(
        &home,
        "assistant_voice_transcript_append",
        json!({
            "group_id":group_id,"session_id":"checkpoints-off","segment_id":"final-asr",
            "document_path":"notes.md","text":"Keep this final recording","is_final":true,"flush":true,
            "transcript_stage":"final","revision_only":true
        }),
    );
    assert_eq!(appended.result["input_event_created"], true);
    assert!(
        appended.result["input_event"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Keep this final recording"))
    );
    assert_eq!(
        appended.result["assistant"]["config"]["auto_document_max_window_seconds"],
        Value::Null
    );
}
