use super::*;

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
