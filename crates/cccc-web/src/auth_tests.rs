use super::*;
use axum::body::Body;

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
    assert!(!requires_admin(&Method::GET, "/api/v1/groups/g_one/actors"));
}

#[test]
fn websocket_origin_must_match_the_served_origin() {
    let same_origin = Request::builder()
        .uri("/api/v1/groups/g_one/actors/a/term")
        .header(header::UPGRADE, "websocket")
        .header(header::HOST, "cccc.example")
        .header(header::ORIGIN, "http://cccc.example")
        .body(Body::empty())
        .expect("request");
    assert!(websocket_origin_allowed_with_proxy(&same_origin, false));

    let cross_origin = Request::builder()
        .uri("/api/v1/groups/g_one/actors/a/term")
        .header(header::UPGRADE, "websocket")
        .header(header::HOST, "cccc.example")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .expect("request");
    assert!(!websocket_origin_allowed_with_proxy(&cross_origin, false));
}
