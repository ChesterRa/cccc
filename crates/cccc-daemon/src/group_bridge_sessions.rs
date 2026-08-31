use cccc_contracts::{DaemonRequest, GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION};
use cccc_core::HomeLayout;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tokio::sync::{mpsc as tokio_mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

mod handshake;
mod route_state;

const SCAN_INTERVAL: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RouteConfig {
    trust_id: String,
    registration_id: String,
    local_group_id: String,
    remote_group_id: String,
    remote_peer_id: String,
    endpoint: String,
    min_session_protocol: u64,
}

impl RouteConfig {
    fn same_route(&self, other: &Self) -> bool {
        self.trust_id == other.trust_id
            && self.registration_id == other.registration_id
            && self.local_group_id == other.local_group_id
            && self.remote_group_id == other.remote_group_id
            && self.remote_peer_id == other.remote_peer_id
            && self.endpoint == other.endpoint
    }
}

pub(super) struct SessionCommand {
    request: Value,
    response: mpsc::Sender<Value>,
}

struct ManagedWorker {
    config: RouteConfig,
    effective_min_protocol: Arc<AtomicU64>,
    task: JoinHandle<()>,
}

impl ManagedWorker {
    fn matches(&self, desired: &RouteConfig) -> bool {
        self.config.same_route(desired)
            && desired.min_session_protocol <= self.effective_min_protocol.load(Ordering::Acquire)
    }
}

pub(crate) struct SessionManager {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl SessionManager {
    pub(crate) fn start(home: HomeLayout) -> Self {
        let (stop, stop_rx) = watch::channel(false);
        let task = tokio::spawn(run_manager(home, stop_rx));
        Self { stop, task }
    }

    pub(crate) async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

pub(crate) fn send(
    local_group_id: &str,
    remote_group_id: &str,
    remote_peer_id: &str,
    request: Value,
) -> Option<Value> {
    route_state::send(
        local_group_id,
        remote_group_id,
        remote_peer_id,
        request,
        REQUEST_TIMEOUT,
    )
}

async fn run_manager(home: HomeLayout, stop: watch::Receiver<bool>) {
    run_manager_with_interval(home, stop, SCAN_INTERVAL).await;
}

async fn run_manager_with_interval(
    home: HomeLayout,
    mut stop: watch::Receiver<bool>,
    scan_interval: Duration,
) {
    let mut workers: HashMap<String, ManagedWorker> = HashMap::new();
    loop {
        if *stop.borrow() {
            break;
        }
        crate::ops::schedule_due_retries(home.clone());
        let desired = route_state::load_routes(&home);
        workers.retain(|id, worker| {
            let keep = desired.get(id).is_some_and(|config| worker.matches(config))
                && !worker.task.is_finished();
            if !keep {
                worker.task.abort();
                route_state::unregister(&worker.config);
            }
            keep
        });
        for (id, config) in desired {
            if workers.contains_key(&id) {
                continue;
            }
            let effective_min_protocol = Arc::new(AtomicU64::new(config.min_session_protocol));
            let task = tokio::spawn(run_worker_tracking(
                home.clone(),
                config.clone(),
                stop.clone(),
                effective_min_protocol.clone(),
            ));
            workers.insert(
                id,
                ManagedWorker {
                    config,
                    effective_min_protocol,
                    task,
                },
            );
        }
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { break; }
            }
            () = tokio::time::sleep(scan_interval) => {}
        }
    }
    for (_, worker) in workers {
        worker.task.abort();
        route_state::unregister(&worker.config);
    }
}

#[cfg(test)]
async fn run_worker(home: HomeLayout, config: RouteConfig, stop: watch::Receiver<bool>) {
    let effective_min_protocol = Arc::new(AtomicU64::new(config.min_session_protocol));
    run_worker_tracking(home, config, stop, effective_min_protocol).await;
}

async fn run_worker_tracking(
    home: HomeLayout,
    config: RouteConfig,
    mut stop: watch::Receiver<bool>,
    effective_min_protocol: Arc<AtomicU64>,
) {
    let mut retry = Duration::from_secs(1);
    while !*stop.borrow() {
        match connect_once_tracking(&home, &config, stop.clone(), &effective_min_protocol).await {
            Ok(()) if *stop.borrow() => break,
            Ok(()) => route_state::update_status(&home, &config, false, "session closed"),
            Err(error) => {
                route_state::update_status(&home, &config, false, &error);
                tracing::warn!(
                    trust_id = %config.trust_id,
                    endpoint = %config.endpoint,
                    %error,
                    retry_seconds = retry.as_secs(),
                    "Group Bridge session reconnecting"
                );
            }
        }
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { break; }
            }
            () = tokio::time::sleep(retry) => {}
        }
        retry = (retry * 2).min(Duration::from_secs(30));
    }
    route_state::unregister(&config);
    route_state::update_status(&home, &config, false, "");
}

#[cfg(test)]
async fn connect_once(
    home: &HomeLayout,
    config: &RouteConfig,
    stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let effective_min_protocol = AtomicU64::new(config.min_session_protocol);
    connect_once_tracking(home, config, stop, &effective_min_protocol).await
}

async fn connect_once_tracking(
    home: &HomeLayout,
    config: &RouteConfig,
    mut stop: watch::Receiver<bool>,
    effective_min_protocol: &AtomicU64,
) -> Result<(), String> {
    let socket = handshake::connect_tracking(home, config, effective_min_protocol).await?;
    let (mut sink, mut stream) = socket.split();

    let (command_tx, mut command_rx) = tokio_mpsc::unbounded_channel();
    route_state::register(config, command_tx);
    route_state::update_status(home, config, true, "");
    crate::ops::schedule_pending_route_retry(
        home.clone(),
        config.local_group_id.clone(),
        config.remote_group_id.clone(),
        config.remote_peer_id.clone(),
    );
    let mut pending: HashMap<String, mpsc::Sender<Value>> = HashMap::new();
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let result = loop {
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { break Ok(()); }
            }
            _ = heartbeat.tick() => {
                if let Err(error) = sink.send(Message::Text(json!({"type":"ping"}).to_string().into())).await {
                    break Err(error.to_string());
                }
            }
            command = command_rx.recv() => {
                let Some(command) = command else { break Err("session command channel closed".into()); };
                let request_id = uuid::Uuid::new_v4().simple().to_string();
                let mut frame = command.request.as_object().cloned().unwrap_or_default();
                frame.insert("type".into(), json!("request"));
                frame.insert("request_id".into(), json!(request_id));
                pending.insert(request_id, command.response);
                if let Err(error) = sink.send(Message::Text(Value::Object(frame).to_string().into())).await {
                    break Err(error.to_string());
                }
            }
            incoming = stream.next() => {
                let Some(incoming) = incoming else { break Err("session closed".into()); };
                let frame = match incoming {
                    Ok(message) => message_json(message)?,
                    Err(error) => break Err(error.to_string()),
                };
                match frame["type"].as_str().unwrap_or("") {
                    "response" => {
                        if let Some(waiter) = frame["response_to"].as_str().and_then(|id| pending.remove(id)) {
                            let _ = waiter.send(frame.get("result").cloned().unwrap_or_else(|| json!({})));
                        }
                    }
                    "request" => {
                        let response_to = frame["request_id"].clone();
                        let result = receive_request(home, config, &frame);
                        let response = json!({"type":"response","response_to":response_to,"result":result});
                        if let Err(error) = sink.send(Message::Text(response.to_string().into())).await {
                            break Err(error.to_string());
                        }
                    }
                    "ping" => {
                        if let Err(error) = sink.send(Message::Text(json!({"type":"pong"}).to_string().into())).await {
                            break Err(error.to_string());
                        }
                    }
                    "pong" => {}
                    _ => {}
                }
            }
        }
    };
    route_state::unregister(config);
    for (_, waiter) in pending {
        let _ = waiter.send(json!({
            "ok":false,
            "error":{"code":"peer_session_unavailable","message":"Group Bridge session disconnected"}
        }));
    }
    result
}

fn receive_request(home: &HomeLayout, config: &RouteConfig, frame: &Value) -> Value {
    if frame["message_contract_version"].as_u64() != Some(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION) {
        return json!({
            "ok":false,
            "error":{"code":"contract_version_mismatch","message":"Group Bridge message contract version does not match"}
        });
    }
    let operation = frame["op"].as_str().unwrap_or("");
    if !matches!(operation, "remote_send" | "reply_request_cancel") {
        return json!({
            "ok":false,
            "error":{"code":"unsupported_op","message":"unsupported Group Bridge session operation"}
        });
    }
    let request = DaemonRequest {
        v: 1,
        op: if operation == "reply_request_cancel" {
            "group_bridge_receive_reply_request_cancel".into()
        } else {
            "group_bridge_receive_remote_send".into()
        },
        args: Map::from_iter([
            ("target_group_id".into(), json!(config.local_group_id)),
            ("src_group_id".into(), json!(config.remote_group_id)),
            ("remote_peer_id".into(), json!(config.remote_peer_id)),
            (
                "idempotency_key".into(),
                frame
                    .get("idempotency_key")
                    .cloned()
                    .unwrap_or_else(|| json!(uuid::Uuid::new_v4().simple().to_string())),
            ),
            (
                "payload".into(),
                frame.get("payload").cloned().unwrap_or_else(|| json!({})),
            ),
        ]),
    };
    let response = crate::dispatch::dispatch(home, &request);
    if response.ok {
        Value::Object(response.result)
    } else {
        let error = response.error;
        json!({
            "ok":false,
            "error":{
                "code":error.as_ref().map(|item| item.code.as_str()).unwrap_or("daemon_receive_failed"),
                "message":error.as_ref().map(|item| item.message.as_str()).unwrap_or("daemon receive failed")
            }
        })
    }
}

fn message_json(message: Message) -> Result<Value, String> {
    match message {
        Message::Text(text) => serde_json::from_str(&text).map_err(|error| error.to_string()),
        Message::Close(_) => Err("session closed".into()),
        _ => Err("unexpected non-text Group Bridge frame".into()),
    }
}

#[cfg(test)]
#[path = "group_bridge_sessions/tests/mod.rs"]
mod tests;
