use cccc_contracts::utc_now;
use cccc_core::{HomeLayout, group_bridge_legacy};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;

use super::{RouteConfig, SessionCommand};

type RouteKey = (String, String, String);
type RouteSender = tokio_mpsc::UnboundedSender<SessionCommand>;

static LIVE_ROUTES: OnceLock<Mutex<HashMap<RouteKey, RouteSender>>> = OnceLock::new();

pub(super) fn send(
    local_group_id: &str,
    remote_group_id: &str,
    remote_peer_id: &str,
    request: Value,
    timeout: Duration,
) -> Option<Value> {
    let sender = live_routes()
        .lock()
        .ok()?
        .get(&route_key(local_group_id, remote_group_id, remote_peer_id))
        .cloned()?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(SessionCommand {
            request,
            response: response_tx,
        })
        .ok()?;
    response_rx.recv_timeout(timeout).ok()
}

pub(super) fn load_routes(home: &HomeLayout) -> HashMap<String, RouteConfig> {
    let state = group_bridge_legacy::load(home).unwrap_or_else(|_| json!({}));
    state["trusts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(route_config)
        .map(|config| (config.trust_id.clone(), config))
        .collect()
}

pub(super) fn route_config(value: &Value) -> Option<RouteConfig> {
    if value["status"].as_str()? != "active"
        || value["transport"].as_str()? != "group_bridge_session"
    {
        return None;
    }
    Some(RouteConfig {
        trust_id: nonempty(value, "trust_id")?,
        registration_id: nonempty(value, "registration_id")
            .unwrap_or_else(|| value["trust_id"].as_str().unwrap_or("").to_owned()),
        local_group_id: nonempty(value, "group_id")?,
        remote_group_id: nonempty(value, "remote_group_id")?,
        remote_peer_id: nonempty(value, "remote_peer_id")?,
        endpoint: nonempty(value, "remote_endpoint")?,
        min_session_protocol: value["min_session_protocol"].as_u64().unwrap_or(1),
    })
}

pub(super) fn register(config: &RouteConfig, sender: RouteSender) {
    if let Ok(mut routes) = live_routes().lock() {
        routes.insert(config_key(config), sender);
    }
}

pub(super) fn unregister(config: &RouteConfig) {
    if let Ok(mut routes) = live_routes().lock() {
        routes.remove(&config_key(config));
    }
}

#[cfg(test)]
pub(super) fn contains(local: &str, remote: &str, peer: &str) -> bool {
    live_routes()
        .lock()
        .is_ok_and(|routes| routes.contains_key(&route_key(local, remote, peer)))
}

pub(super) fn update_status(home: &HomeLayout, config: &RouteConfig, connected: bool, error: &str) {
    let now = utc_now();
    let _ = group_bridge_legacy::update(home, |state| {
        let Some(trusts) = state.get_mut("trusts").and_then(Value::as_array_mut) else {
            return Ok(());
        };
        let Some(trust) = trusts
            .iter_mut()
            .find(|trust| trust["trust_id"] == config.trust_id)
        else {
            return Ok(());
        };
        trust["session_connected"] = json!(connected);
        trust["session_last_error"] = json!(error);
        trust["session_updated_at"] = json!(now);
        if connected {
            trust["session_connected_at"] = json!(now);
        } else if !error.is_empty() {
            trust["session_last_error_at"] = json!(now);
        }
        Ok(())
    });
}

fn live_routes() -> &'static Mutex<HashMap<RouteKey, RouteSender>> {
    LIVE_ROUTES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn config_key(config: &RouteConfig) -> RouteKey {
    route_key(
        &config.local_group_id,
        &config.remote_group_id,
        &config.remote_peer_id,
    )
}

fn route_key(local: &str, remote: &str, peer: &str) -> RouteKey {
    (
        local.trim().into(),
        remote.trim().into(),
        peer.trim().into(),
    )
}

fn nonempty(value: &Value, field: &str) -> Option<String> {
    value[field]
        .as_str()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
}
