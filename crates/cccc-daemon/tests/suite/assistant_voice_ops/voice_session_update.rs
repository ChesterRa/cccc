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
                "patch":{"status":"closed"}
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
