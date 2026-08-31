use super::*;
use cccc_contracts::utc_now;
use cccc_core::group_bridge_identity::GroupBridgeIdentity;
use cccc_core::group_bridge_identity::{
    authenticated_legacy_session_peer_id, authenticated_session_v2_peer_id,
};
use cccc_core::{GroupStore, group_bridge_legacy};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::accept_async;

async fn receive_v2_hello(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    expected_peer_id: &str,
    server_identity: &GroupBridgeIdentity,
) -> (Value, Value) {
    let challenge = signed_challenge(server_identity);
    socket
        .send(Message::Text(challenge.to_string().into()))
        .await
        .expect("challenge");
    let hello = message_json(socket.next().await.expect("hello").expect("hello frame"))
        .expect("hello json");
    assert_eq!(
        authenticated_session_v2_peer_id(&hello, &challenge).as_deref(),
        Some(expected_peer_id)
    );
    (hello, challenge)
}

async fn send_v2_ready(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    identity: &GroupBridgeIdentity,
    hello: &Value,
    challenge: &Value,
) {
    let mut ready = json!({
        "ok":true,"type":"ready",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    });
    identity
        .sign_session_ready_v2(&mut ready, hello, challenge)
        .expect("sign ready");
    socket
        .send(Message::Text(ready.to_string().into()))
        .await
        .expect("ready");
}

fn signed_challenge(identity: &GroupBridgeIdentity) -> Value {
    let mut challenge = json!({
        "type":"challenge",
        "protocol":"/cccc/group_bridge/session-ws/2.0.0",
        "nonce":uuid::Uuid::new_v4().simple().to_string(),
        "issued_at":utc_now(),
        "expires_at":(chrono::Utc::now()+chrono::Duration::seconds(30)).to_rfc3339(),
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    });
    identity
        .sign_session_challenge_v2(&mut challenge)
        .expect("sign challenge");
    challenge
}

fn test_identity(temp: &tempfile::TempDir, name: &str) -> GroupBridgeIdentity {
    let home = HomeLayout::from_path(temp.path().join(name)).expect("identity home");
    GroupBridgeIdentity::load_or_create(&home).expect("identity")
}

fn seed_route_trust(home: &HomeLayout, config: &RouteConfig, minimum_protocol: u64) {
    group_bridge_legacy::update(home, |state| {
        state["trusts"] = json!([{
            "trust_id":config.trust_id,
            "registration_id":config.registration_id,
            "group_id":config.local_group_id,
            "remote_group_id":config.remote_group_id,
            "remote_peer_id":config.remote_peer_id,
            "remote_endpoint":config.endpoint,
            "transport":"group_bridge_session",
            "status":"active",
            "min_session_protocol":minimum_protocol
        }]);
        Ok(())
    })
    .expect("seed route trust");
}

fn persisted_minimum(home: &HomeLayout, trust_id: &str) -> u64 {
    group_bridge_legacy::load(home).expect("bridge state")["trusts"]
        .as_array()
        .and_then(|trusts| trusts.iter().find(|trust| trust["trust_id"] == trust_id))
        .and_then(|trust| trust["min_session_protocol"].as_u64())
        .unwrap_or(1)
}

mod connection;
mod delivery;

#[test]
fn session_url_is_derived_from_http_endpoint() {
    assert_eq!(
        handshake::session_url("https://remote.example:9443/base?x=1", 1).expect("url"),
        "wss://remote.example:9443/api/group-bridge/session/ws"
    );
    assert_eq!(
        handshake::session_url("https://remote.example:9443/base?x=1", 2).expect("url"),
        "wss://remote.example:9443/api/group-bridge/session/ws/v2"
    );
}

#[test]
fn inactive_or_incomplete_trust_is_not_started() {
    assert!(route_state::route_config(&json!({"status":"revoked"})).is_none());
    assert!(
        route_state::route_config(&json!({
            "status":"active","transport":"group_bridge_session",
            "trust_id":"t","group_id":"g","remote_group_id":"r","remote_peer_id":"p"
        }))
        .is_none()
    );
}

mod reconnect;
mod security;
