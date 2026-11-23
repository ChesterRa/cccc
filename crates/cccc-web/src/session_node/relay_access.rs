use cccc_core::{HomeLayout, session_nodes};
use iroh::EndpointId;
use iroh_relay::server::{Access, AccessControl, ClientRequest, ConnectionId, RelayService};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) const AUTH_PREFIX: &str = "cccc-session-node:";

pub(super) struct RelayAccess {
    home: HomeLayout,
    gateway: String,
    hub_id: EndpointId,
    admitted: Mutex<HashMap<ConnectionId, EndpointId>>,
    checks: tokio::sync::Semaphore,
    pub stopped: AtomicBool,
}

impl std::fmt::Debug for RelayAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionNodeRelayAccess")
            .field("hub_id", &self.hub_id)
            .finish_non_exhaustive()
    }
}

impl RelayAccess {
    pub fn new(home: HomeLayout, gateway: String, hub_id: EndpointId) -> Self {
        Self {
            home,
            gateway,
            hub_id,
            admitted: Mutex::default(),
            checks: tokio::sync::Semaphore::new(2),
            stopped: AtomicBool::new(false),
        }
    }

    pub async fn reap(&self, service: &RelayService) {
        let connections = self
            .admitted
            .lock()
            .map(|map| {
                map.iter()
                    .filter(|(_, node)| **node != self.hub_id)
                    .map(|(id, node)| (*id, *node))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if connections.is_empty() {
            return;
        }
        let home = self.home.clone();
        let gateway = self.gateway.clone();
        let allowed =
            tokio::task::spawn_blocking(move || session_nodes::relay_authorities(&home, &gateway))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
        for (connection, node) in connections {
            if !allowed.contains(&node.to_string()) {
                service.clients().disconnect(node, Some(connection));
            }
        }
    }
}

fn denied() -> Access {
    Access::Deny {
        reason: Some("Session-node relay authority is unavailable".into()),
    }
}

impl AccessControl for RelayAccess {
    async fn on_connect(&self, request: &ClientRequest) -> Access {
        if self.stopped.load(Ordering::Acquire) {
            return denied();
        }
        let node = request.endpoint_id();
        if node != self.hub_id {
            let Ok(_check) = self.checks.acquire().await else {
                return denied();
            };
            if self.stopped.load(Ordering::Acquire) {
                return denied();
            }
            let token = request
                .auth_token()
                .and_then(|value| value.strip_prefix(AUTH_PREFIX).map(str::to_owned));
            let home = self.home.clone();
            let gateway = self.gateway.clone();
            let id = node.to_string();
            let result = tokio::task::spawn_blocking(move || {
                let invite = token.as_deref().and_then(|value| value.split_once(':'));
                session_nodes::authorize_relay(&home, &gateway, &id, invite)
            })
            .await;
            if !matches!(result, Ok(Ok(()))) {
                return denied();
            }
        }
        let Ok(mut admitted) = self.admitted.lock() else {
            return denied();
        };
        if self.stopped.load(Ordering::Acquire)
            || admitted.len() >= 65
            || admitted
                .values()
                .filter(|current| **current == node)
                .count()
                >= 2
        {
            return denied();
        }
        admitted.insert(request.connection_id(), node);
        Access::Allow
    }

    fn on_disconnect(&self, _: EndpointId, connection: ConnectionId) {
        if let Ok(mut admitted) = self.admitted.lock() {
            admitted.remove(&connection);
        }
    }
}
