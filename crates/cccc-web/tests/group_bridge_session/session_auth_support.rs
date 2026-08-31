use super::*;
use cccc_contracts::utc_now;
use cccc_core::group_bridge_identity::{
    authenticated_session_challenge_v2_peer_id, authenticated_session_ready_v2_peer_id,
};

pub(super) async fn connect_v2_socket(
    address: &str,
    signing: &SigningKey,
    peer_id: &str,
    group_id: &str,
) -> (TestSocket, Value, Value) {
    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/group-bridge/session/ws/v2"))
            .await
            .expect("connect signed v2");
    let challenge = next_socket_json(&mut socket).await;
    assert_eq!(challenge["type"], "challenge");
    assert!(authenticated_session_challenge_v2_peer_id(&challenge).is_some());
    let hello = signed_v2_hello(signing, peer_id, group_id, &challenge);
    socket
        .send(WsMessage::Text(hello.to_string().into()))
        .await
        .expect("v2 hello");
    (socket, hello, challenge)
}

pub(super) async fn next_v2_ready(
    socket: &mut TestSocket,
    hello: &Value,
    challenge: &Value,
) -> Value {
    let ready = next_socket_json(socket).await;
    assert_eq!(ready["ok"], true);
    assert_eq!(
        authenticated_session_ready_v2_peer_id(&ready, hello, challenge),
        challenge["server_peer_id"].as_str().map(str::to_owned)
    );
    ready
}

pub(super) fn signed_hello(signing: &SigningKey, peer_id: &str, group_id: &str) -> Value {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let issued_at = utc_now();
    let legacy_material = json!({
        "protocol":"/cccc/group_bridge/session-ws/1.0.0",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "remote_peer_id":peer_id,
        "src_group_id":"g_sender",
        "target_group_id":group_id
    })
    .to_string();
    let fresh_material = json!({
        "protocol":"/cccc/group_bridge/session-ws/1.0.0",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "remote_peer_id":peer_id,
        "src_group_id":"g_sender",
        "target_group_id":group_id,
        "nonce":nonce,
        "issued_at":issued_at
    })
    .to_string();
    let signature = signing.sign(legacy_material.as_bytes());
    let fresh_signature = signing.sign(fresh_material.as_bytes());
    json!({
        "target_group_id":group_id,"src_group_id":"g_sender","remote_peer_id":peer_id,
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "nonce":nonce,"issued_at":issued_at,
        "public_key":base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes()),
        "signature":base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
        "fresh_signature":base64::engine::general_purpose::STANDARD.encode(fresh_signature.to_bytes())
    })
}

pub(super) fn legacy_signed_hello(signing: &SigningKey, peer_id: &str, group_id: &str) -> Value {
    let material = json!({
        "protocol":"/cccc/group_bridge/session-ws/1.0.0",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "remote_peer_id":peer_id,
        "src_group_id":"g_sender",
        "target_group_id":group_id
    })
    .to_string();
    json!({
        "target_group_id":group_id,"src_group_id":"g_sender","remote_peer_id":peer_id,
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "public_key":base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes()),
        "signature":base64::engine::general_purpose::STANDARD.encode(signing.sign(material.as_bytes()).to_bytes())
    })
}

pub(super) fn signed_v2_hello(
    signing: &SigningKey,
    peer_id: &str,
    group_id: &str,
    challenge: &Value,
) -> Value {
    let client_nonce = uuid::Uuid::new_v4().simple().to_string();
    let material = json!({
        "protocol":"/cccc/group_bridge/session-ws/2.0.0",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "remote_peer_id":peer_id,
        "src_group_id":"g_sender",
        "target_group_id":group_id,
        "challenge_nonce":challenge["nonce"],
        "challenge_issued_at":challenge["issued_at"],
        "client_nonce":client_nonce
    })
    .to_string();
    json!({
        "target_group_id":group_id,"src_group_id":"g_sender","remote_peer_id":peer_id,
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
        "challenge_nonce":challenge["nonce"],"challenge_issued_at":challenge["issued_at"],
        "client_nonce":client_nonce,
        "public_key":base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes()),
        "signature":base64::engine::general_purpose::STANDARD.encode(signing.sign(material.as_bytes()).to_bytes())
    })
}
