use super::*;

#[test]
fn identity_is_stable_and_builds_signed_session_hello() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let first = GroupBridgeIdentity::load_or_create(&home).expect("identity");
    let second = GroupBridgeIdentity::load_or_create(&home).expect("identity");
    assert_eq!(first.peer_id, second.peer_id);
    assert_eq!(first.public_key_b64, second.public_key_b64);
    let hello = first
        .sign_session_hello("g_remote", "g_local")
        .expect("hello");
    assert_eq!(hello["remote_peer_id"], first.peer_id);
    assert!(!hello["signature"].as_str().unwrap_or("").is_empty());
    assert!(!hello["fresh_signature"].as_str().unwrap_or("").is_empty());
    assert!(!hello["nonce"].as_str().unwrap_or("").is_empty());

    let mut challenge = json!({
        "type":"challenge","protocol":SESSION_PROTOCOL_V2,
        "nonce":"challenge-1234567890","issued_at":utc_now(),
        "expires_at":"2099-01-01T00:00:00Z",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    });
    first
        .sign_session_challenge_v2(&mut challenge)
        .expect("signed challenge");
    assert_eq!(
        authenticated_session_challenge_v2_peer_id(&challenge),
        Some(first.peer_id.clone())
    );
    let v2 = first
        .sign_session_hello_v2("g_remote", "g_local", &challenge)
        .expect("v2 hello");
    assert_eq!(
        authenticated_session_v2_peer_id(&v2, &challenge),
        Some(first.peer_id.clone())
    );
    let mut ready = json!({
        "ok":true,"type":"ready",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    });
    first
        .sign_session_ready_v2(&mut ready, &v2, &challenge)
        .expect("signed ready");
    assert_eq!(
        authenticated_session_ready_v2_peer_id(&ready, &v2, &challenge),
        Some(first.peer_id.clone())
    );
    let mut other_hello = v2.clone();
    other_hello["client_nonce"] = json!("different-client-nonce");
    assert!(authenticated_session_ready_v2_peer_id(&ready, &other_hello, &challenge).is_none());
    let different = json!({"nonce":"different-123456789","issued_at":challenge["issued_at"]});
    assert!(authenticated_session_v2_peer_id(&v2, &different).is_none());
}
