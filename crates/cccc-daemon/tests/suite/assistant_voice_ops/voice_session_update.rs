use super::*;

#[test]
fn voice_session_mutations_enforce_status_permissions() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    store
        .mutate(&group_id, |group| {
            let mut peer = Actor::new("peer");
            peer.role = Some(ActorRole::Peer);
            group.actors.push(peer);
            Ok(())
        })
        .expect("add peer");

    for (op, args) in [
        (
            "assistant_voice_session_update",
            json!({
                "group_id":group_id,
                "session_id":"peer-session",
                "by":"peer",
                "completion_event":"diarization_ready",
                "patch":{"status":"closed","diarization_ready":true}
            }),
        ),
        (
            "assistant_voice_session_transcript_clear",
            json!({
                "group_id":group_id,
                "session_id":"peer-session",
                "by":"peer"
            }),
        ),
    ] {
        let denied = call(&home, op, args);
        assert!(!denied.ok, "{op} unexpectedly allowed a peer");
        assert_eq!(
            denied.error.expect("permission error").code,
            "permission_denied"
        );
    }

    let allowed = ok(
        &home,
        "assistant_voice_session_update",
        json!({
            "group_id":group_id,
            "session_id":"foreman-session",
            "by":"foreman",
            "patch":{"status":"closed"}
        }),
    );
    assert_eq!(allowed.result["session"]["status"], "closed");
    assert_eq!(allowed.result["session"]["schema"], 1);
    assert_eq!(allowed.result["session"]["group_id"], group_id);
    assert_eq!(allowed.result["session"]["session_id"], "foreman-session");
    assert_eq!(allowed.result["session"]["capture_mode"], "document");
}

#[test]
fn voice_session_update_prunes_missing_session_fallback_to_fifty() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    update_voice_state(&home, &group_id, |state| {
        let sessions = (0..50)
            .map(|index| {
                json!({
                    "session_id":format!("session-{index:02}"),
                    "updated_at":format!("2026-08-10T00:{index:02}:00Z")
                })
            })
            .collect::<Vec<_>>();
        state.insert("sessions".into(), Value::Array(sessions));
        Ok(())
    });

    let updated = ok(
        &home,
        "assistant_voice_session_update",
        json!({
            "group_id":group_id,
            "session_id":"session-new",
            "by":"assistant:voice_secretary",
            "patch":{"status":"closed","diarization_ready":true,"diarization":{}}
        }),
    );
    assert_eq!(updated.result["session"]["schema"], 1);
    assert_eq!(updated.result["session"]["group_id"], group_id);

    let state = load_voice_state(&home, &group_id);
    let sessions = state["sessions"].as_array().expect("voice sessions");
    assert_eq!(sessions.len(), 50);
    assert!(
        sessions
            .iter()
            .any(|session| session["session_id"] == "session-new")
    );
    assert!(
        sessions
            .iter()
            .all(|session| session["session_id"] != "session-00")
    );
}

#[test]
fn completion_persists_state_and_emits_each_outcome_once() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let args = |ready: bool| {
        json!({
            "group_id":group_id, "session_id":"completion-1", "by":"assistant:voice_secretary",
            "completion_event":if ready {"diarization_ready"} else {"diarization_failed"},
            "patch":{"status":"closed", "document_path":"docs/meeting.md", "diarization_ready":ready,
                "diarization_error":if ready {Value::Null} else {json!({"code":"model_failed","message":"analysis failed"})},
                "error":null}
        })
    };
    for ready in [false, false, true, true] {
        let response = ok(&home, "assistant_voice_session_update", args(ready));
        assert_eq!(response.result["session"]["diarization_ready"], ready);
        assert!(
            response.result["completion_event_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
        );
    }
    let events = ledger::read_all(&store.ledger_path(&group_id).expect("ledger path"))
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "assistant.voice.session")
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].data["action"], "diarization_failed");
    assert_eq!(events[0].data["error_code"], "model_failed");
    assert_eq!(events[1].data["action"], "diarization_ready");
    assert_eq!(events[1].data["error_message"], "");
    assert_eq!(events[1].by, "system");
    assert_eq!(events[1].data["document_path"], "docs/meeting.md");
}

#[test]
fn invalid_completion_cannot_mutate_the_session_or_publish_an_event() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let before = assistant_state::load(&home, &group_id).expect("state");
    for action in [
        json!("anything_else"),
        json!(true),
        json!("diarization_failed"),
    ] {
        let response = call(
            &home,
            "assistant_voice_session_update",
            json!({
                "group_id":group_id, "session_id":"invalid-completion", "by":"assistant:voice_secretary",
                "completion_event":action, "patch":{"status":"closed","diarization_ready":true}
            }),
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("invalid completion").code,
            "invalid_args"
        );
    }
    assert_eq!(
        assistant_state::load(&home, &group_id).expect("state"),
        before
    );
    assert!(
        ledger::read_all(&store.ledger_path(&group_id).expect("ledger path"))
            .expect("events")
            .iter()
            .all(|event| event.kind != "assistant.voice.session")
    );
}

#[test]
fn completion_append_failure_is_reported_and_can_be_retried_after_state_persistence() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let path = store.ledger_path(&group_id).expect("ledger path");
    let original = std::fs::read(&path).expect("ledger");
    std::fs::remove_file(&path).expect("remove fixture ledger");
    std::fs::create_dir(&path).expect("block fixture append");
    let args = json!({
        "group_id":group_id,"session_id":"retry-completion","by":"assistant:voice_secretary",
        "completion_event":"diarization_ready",
        "patch":{"status":"closed","diarization_ready":true,"document_path":"docs/meeting.md"}
    });
    let failed = call(&home, "assistant_voice_session_update", args.clone());
    assert!(!failed.ok);
    assert_eq!(failed.error.expect("append failed").code, "io_error");
    let state = assistant_state::load(&home, &group_id).expect("state persisted");
    assert!(
        state["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|session| session["session_id"] == "retry-completion"
                && session["diarization_ready"] == true)
    );
    std::fs::remove_dir(&path).expect("unblock fixture ledger");
    std::fs::write(&path, original).expect("restore fixture ledger");
    ok(&home, "assistant_voice_session_update", args.clone());
    ok(&home, "assistant_voice_session_update", args);
    assert_eq!(
        ledger::read_all(&path)
            .expect("events")
            .iter()
            .filter(|event| event.kind == "assistant.voice.session")
            .count(),
        1
    );
}
