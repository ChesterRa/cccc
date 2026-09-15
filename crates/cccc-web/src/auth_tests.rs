use super::*;

#[test]
fn direct_connection_management_never_becomes_a_public_peer_endpoint() {
    for method in [Method::GET, Method::POST] {
        assert!(requires_admin(&method, "/api/v1/connect/direct"));
        assert!(!is_public(&method, "/api/v1/connect/direct"));
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
