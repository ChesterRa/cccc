use super::*;

#[test]
fn group_authorization_uses_the_decoded_route_identity() {
    let id = "g_one";
    let encoded = percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC);
    let path = format!("/api/v1/groups/{encoded}/pins");
    let resolved = group_from_path(&path).expect("Group identity");
    assert_eq!(resolved, id);
    let principal = Principal {
        user_id: "fixture".into(),
        allowed_groups: vec![id.into()],
        is_admin: false,
        raw_token: String::new(),
    };
    assert!(principal.allows(&resolved));
    let other = group_from_path("/api/v1/groups/g_other/pins").expect("other Group identity");
    assert!(!principal.allows(&other));
    assert!(requires_admin(&Method::DELETE, &path));
    assert_eq!(group_from_path("/api/v1/groups/copy/preview_import"), None);
}

#[test]
fn direct_connection_management_never_becomes_a_public_peer_endpoint() {
    for method in [Method::GET, Method::POST] {
        assert!(requires_admin(&method, "/api/v1/connect/direct"));
        assert!(!is_public(&method, "/api/v1/connect/direct"));
    }
}

#[test]
fn process_recovery_requires_an_administrator_for_reads_and_changes() {
    for method in [Method::GET, Method::HEAD, Method::POST] {
        for suffix in ["daemon", "web", "web/retry", "web/accept"] {
            let path = format!("/api/v1/process-recovery/{suffix}");
            assert!(requires_admin(&method, &path));
            assert!(!is_public(&method, &path));
        }
    }
}

#[test]
fn chet_history_and_inputs_require_administrator_authority() {
    for method in [Method::GET, Method::HEAD, Method::POST] {
        for path in [
            "/api/v1/chet/history",
            "/api/v1/chet/status",
            "/api/v1/chet/inputs",
        ] {
            assert!(requires_admin(&method, path));
            assert!(!is_public(&method, path));
        }
    }
}

#[test]
fn shared_voice_services_require_admin_but_task_controls_follow_the_group() {
    for method in [Method::GET, Method::PUT, Method::POST] {
        assert!(requires_admin(&method, "/api/v1/voice-secretary/settings"));
        assert!(requires_admin(&method, "/api/v1/voice/asr/models"));
    }
    assert!(requires_admin(
        &Method::POST,
        "/api/v1/groups/g/assistants/voice_secretary/models/install"
    ));
    for path in [
        "/api/v1/voice-secretary/runtime",
        "/api/v1/voice-secretary/runtime/reset",
        "/api/v1/voice-secretary/term",
    ] {
        assert!(requires_admin(&Method::GET, path));
        assert!(requires_admin(&Method::POST, path));
    }
    for action in ["cancel", "retry", "handoff", "candidate"] {
        assert!(!requires_admin(
            &Method::POST,
            &format!("/api/v1/groups/g/assistants/voice_secretary/tasks/t/{action}")
        ));
    }
}

#[test]
fn legacy_profiles_stay_admin_only_while_scoped_profiles_use_user_policy() {
    assert!(!requires_admin(&Method::GET, "/api/v1/profiles"));
    assert!(requires_admin(&Method::POST, "/api/v1/actor_profiles"));
    assert!(requires_admin(
        &Method::GET,
        "/api/v1/actor_profiles/ap_one/env_private"
    ));
    assert!(requires_admin(
        &Method::POST,
        "/api/v1/space/providers/notebooklm/credential"
    ));
    assert!(requires_admin(
        &Method::GET,
        "/api/v1/codex_voice/calls/active"
    ));
    assert!(requires_admin(
        &Method::PUT,
        "/api/v1/codex_voice/analyst-settings"
    ));
    assert!(requires_admin(
        &Method::GET,
        "/api/v1/codex_voice/analysts/a_one/terminal"
    ));
    assert!(!requires_admin(&Method::GET, "/api/v1/groups/g_one/actors"));
}
