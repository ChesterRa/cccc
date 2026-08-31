use cccc_contracts::{GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION, utc_now};
use cccc_core::group_bridge_identity::{
    GroupBridgeIdentity, authenticated_session_challenge_v2_peer_id,
    authenticated_session_ready_v2_peer_id,
};
use cccc_core::{HomeLayout, group_bridge_legacy};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};

use super::RouteConfig;

type SessionSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[cfg(test)]
pub(super) async fn connect(
    home: &HomeLayout,
    config: &RouteConfig,
) -> Result<SessionSocket, String> {
    let effective_min_protocol = AtomicU64::new(config.min_session_protocol);
    connect_tracking(home, config, &effective_min_protocol).await
}

pub(super) async fn connect_tracking(
    home: &HomeLayout,
    config: &RouteConfig,
    effective_min_protocol: &AtomicU64,
) -> Result<SessionSocket, String> {
    let identity = GroupBridgeIdentity::load_or_create(home).map_err(|error| error.to_string())?;
    let minimum_protocol = minimum_protocol(home, config, effective_min_protocol)?;
    let v2_url = session_url(&config.endpoint, 2)?;
    let v2 = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&v2_url),
    )
    .await
    .map_err(|_| "session v2 connect timed out".to_owned())?;
    let (mut socket, protocol) = match v2 {
        Ok((socket, _)) => (socket, 2),
        Err(error) if v2_unavailable(&error) && minimum_protocol < 2 => {
            let v1_url = session_url(&config.endpoint, 1)?;
            let (socket, _) = tokio::time::timeout(
                Duration::from_secs(5),
                tokio_tungstenite::connect_async(&v1_url),
            )
            .await
            .map_err(|_| "session v1 fallback connect timed out".to_owned())?
            .map_err(|error| format!("session v1 fallback failed: {error}"))?;
            (socket, 1)
        }
        Err(error) if v2_unavailable(&error) => {
            return Err("session v2 is pinned for this trust; refusing v1 downgrade".into());
        }
        Err(error) => return Err(format!("session v2 connect failed: {error}")),
    };
    let mut v2_transcript = None;
    let hello = if protocol == 2 {
        let challenge = next_json(&mut socket, "session v2 challenge timed out").await?;
        validate_challenge(config, &challenge)?;
        let hello = identity
            .sign_session_hello_v2(&config.remote_group_id, &config.local_group_id, &challenge)
            .map_err(|error| error.to_string())?;
        v2_transcript = Some((challenge, hello.clone()));
        hello
    } else {
        identity
            .sign_session_hello(&config.remote_group_id, &config.local_group_id)
            .map_err(|error| error.to_string())?
    };
    socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .map_err(|error| error.to_string())?;
    let ready = next_json(&mut socket, "session handshake timed out").await?;
    if ready.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "session rejected: {}",
            ready
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("remote rejected Group Bridge session")
        ));
    }
    if ready["message_contract_version"].as_u64() != Some(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION) {
        return Err("Group Bridge message contract version does not match".into());
    }
    if let Some((challenge, hello)) = v2_transcript {
        let ready_peer = authenticated_session_ready_v2_peer_id(&ready, &hello, &challenge)
            .ok_or_else(|| "remote Group Bridge v2 ready signature is invalid".to_owned())?;
        if ready_peer != config.remote_peer_id {
            return Err("remote Group Bridge v2 ready identity does not match trust".into());
        }
        effective_min_protocol.fetch_max(2, Ordering::Release);
        pin_v2(home, config)?;
    }
    Ok(socket)
}

fn minimum_protocol(
    home: &HomeLayout,
    config: &RouteConfig,
    effective_min_protocol: &AtomicU64,
) -> Result<u64, String> {
    let state = group_bridge_legacy::load(home).map_err(|error| error.to_string())?;
    state["trusts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|trust| trust_matches(trust, config))
        .map(|trust| {
            trust["min_session_protocol"]
                .as_u64()
                .unwrap_or(1)
                .max(config.min_session_protocol)
                .max(effective_min_protocol.load(Ordering::Acquire))
        })
        .ok_or_else(|| "Group Bridge trust is no longer active".into())
}

fn pin_v2(home: &HomeLayout, config: &RouteConfig) -> Result<(), String> {
    group_bridge_legacy::update(home, |state| {
        let trust = state["trusts"]
            .as_array_mut()
            .and_then(|trusts| trusts.iter_mut().find(|trust| trust_matches(trust, config)))
            .ok_or_else(|| std::io::Error::other("Group Bridge trust is no longer active"))?;
        trust["min_session_protocol"] = json!(2);
        trust["updated_at"] = json!(utc_now());
        Ok(())
    })
    .map_err(|error| error.to_string())
}

fn trust_matches(trust: &Value, config: &RouteConfig) -> bool {
    trust["status"] == "active"
        && trust["trust_id"].as_str() == Some(config.trust_id.as_str())
        && trust["group_id"].as_str() == Some(config.local_group_id.as_str())
        && trust["remote_group_id"].as_str() == Some(config.remote_group_id.as_str())
        && trust["remote_peer_id"].as_str() == Some(config.remote_peer_id.as_str())
        && trust["remote_endpoint"].as_str() == Some(config.endpoint.as_str())
}

fn validate_challenge(config: &RouteConfig, challenge: &Value) -> Result<(), String> {
    if challenge["type"] != "challenge"
        || challenge["protocol"] != "/cccc/group_bridge/session-ws/2.0.0"
        || challenge["message_contract_version"].as_u64()
            != Some(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION)
    {
        return Err("remote returned an invalid Group Bridge v2 challenge".into());
    }
    let server_peer_id = authenticated_session_challenge_v2_peer_id(challenge)
        .ok_or_else(|| "remote Group Bridge v2 challenge signature is invalid".to_owned())?;
    if server_peer_id != config.remote_peer_id {
        return Err("remote Group Bridge v2 challenge identity does not match trust".into());
    }
    if challenge["expires_at"]
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|expires_at| expires_at.with_timezone(&chrono::Utc) <= chrono::Utc::now())
    {
        return Err("remote Group Bridge v2 challenge expired".into());
    }
    Ok(())
}

async fn next_json(socket: &mut SessionSocket, timeout: &str) -> Result<Value, String> {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .map_err(|_| timeout.to_owned())?
        .ok_or_else(|| "session closed during handshake".to_owned())?
        .map_err(|error| error.to_string())?;
    super::message_json(message)
}

pub(super) fn session_url(endpoint: &str, protocol: u8) -> Result<String, String> {
    let mut url = reqwest::Url::parse(endpoint).map_err(|error| error.to_string())?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return Err("Group Bridge endpoint must use http or https".into()),
    };
    url.set_scheme(scheme)
        .map_err(|_| "invalid Group Bridge endpoint scheme".to_owned())?;
    url.set_path(if protocol >= 2 {
        "/api/group-bridge/session/ws/v2"
    } else {
        "/api/group-bridge/session/ws"
    });
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn v2_unavailable(error: &WebSocketError) -> bool {
    matches!(
        error,
        WebSocketError::Http(response)
            if matches!(response.status().as_u16(), 401 | 404 | 405)
    )
}
