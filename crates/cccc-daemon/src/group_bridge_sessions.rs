use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::group_bridge_identity::GroupBridgeIdentity;
use cccc_core::{HomeLayout, group_bridge_legacy, integration_state};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;
use tokio::sync::{mpsc as tokio_mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const STORE_KEY: &str = "group_bridge";
const SCAN_INTERVAL: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

type RouteKey = (String, String, String);
type RouteSender = tokio_mpsc::UnboundedSender<SessionCommand>;

static LIVE_ROUTES: OnceLock<Mutex<HashMap<RouteKey, RouteSender>>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
struct RouteConfig {
    trust_id: String,
    registration_id: String,
    local_group_id: String,
    remote_group_id: String,
    remote_peer_id: String,
    endpoint: String,
}

struct SessionCommand {
    request: Value,
    response: mpsc::Sender<Value>,
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
    let key = route_key(local_group_id, remote_group_id, remote_peer_id);
    let sender = live_routes().lock().ok()?.get(&key).cloned()?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(SessionCommand {
            request,
            response: response_tx,
        })
        .ok()?;
    response_rx.recv_timeout(REQUEST_TIMEOUT).ok()
}

async fn run_manager(home: HomeLayout, mut stop: watch::Receiver<bool>) {
    let mut workers: HashMap<String, (RouteConfig, JoinHandle<()>)> = HashMap::new();
    loop {
        if *stop.borrow() {
            break;
        }
        let desired = load_routes(&home);
        workers.retain(|id, (config, task)| {
            let keep = desired.get(id) == Some(config) && !task.is_finished();
            if !keep {
                task.abort();
                unregister(config);
            }
            keep
        });
        for (id, config) in desired {
            if workers.contains_key(&id) {
                continue;
            }
            let worker = tokio::spawn(run_worker(home.clone(), config.clone(), stop.clone()));
            workers.insert(id, (config, worker));
        }
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { break; }
            }
            () = tokio::time::sleep(SCAN_INTERVAL) => {}
        }
    }
    for (_, (config, task)) in workers {
        task.abort();
        unregister(&config);
    }
}

async fn run_worker(home: HomeLayout, config: RouteConfig, mut stop: watch::Receiver<bool>) {
    let mut retry = Duration::from_secs(1);
    while !*stop.borrow() {
        match connect_once(&home, &config, stop.clone()).await {
            Ok(()) if *stop.borrow() => break,
            Ok(()) => update_status(&home, &config, false, "session closed"),
            Err(error) => {
                update_status(&home, &config, false, &error);
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
    unregister(&config);
    update_status(&home, &config, false, "");
}

async fn connect_once(
    home: &HomeLayout,
    config: &RouteConfig,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let identity = GroupBridgeIdentity::load_or_create(home).map_err(|error| error.to_string())?;
    let url = session_url(&config.endpoint)?;
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&url),
    )
    .await
    .map_err(|_| "session connect timed out".to_owned())?
    .map_err(|error| format!("session connect failed: {error}"))?;
    let (mut sink, mut stream) = socket.split();
    let hello = identity
        .sign_session_hello(&config.remote_group_id, &config.local_group_id)
        .map_err(|error| error.to_string())?;
    sink.send(Message::Text(hello.to_string().into()))
        .await
        .map_err(|error| error.to_string())?;
    let ready = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .map_err(|_| "session handshake timed out".to_owned())?
        .ok_or_else(|| "session closed during handshake".to_owned())?
        .map_err(|error| error.to_string())?;
    let ready = message_json(ready)?;
    if ready.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "session rejected: {}",
            ready
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("remote rejected Group Bridge session")
        ));
    }

    let (command_tx, mut command_rx) = tokio_mpsc::unbounded_channel();
    register(config, command_tx);
    update_status(home, config, true, "");
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
    unregister(config);
    for (_, waiter) in pending {
        let _ = waiter.send(json!({
            "ok":false,
            "error":{"code":"peer_session_unavailable","message":"Group Bridge session disconnected"}
        }));
    }
    result
}

fn receive_request(home: &HomeLayout, config: &RouteConfig, frame: &Value) -> Value {
    if frame["op"].as_str() != Some("remote_send") {
        return json!({
            "ok":false,
            "error":{"code":"unsupported_op","message":"unsupported Group Bridge session operation"}
        });
    }
    let request = DaemonRequest {
        v: 1,
        op: "group_bridge_receive_remote_send".into(),
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

fn load_routes(home: &HomeLayout) -> HashMap<String, RouteConfig> {
    let _ = group_bridge_legacy::import_if_changed(home);
    let state = integration_state::global_get(home, STORE_KEY).unwrap_or_else(|_| json!({}));
    state["trusts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(route_config)
        .map(|config| (config.trust_id.clone(), config))
        .collect()
}

fn route_config(value: &Value) -> Option<RouteConfig> {
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
    })
}

fn session_url(endpoint: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(endpoint).map_err(|error| error.to_string())?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return Err("Group Bridge endpoint must use http or https".into()),
    };
    url.set_scheme(scheme)
        .map_err(|_| "invalid Group Bridge endpoint scheme".to_owned())?;
    url.set_path("/api/group-bridge/session/ws");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn message_json(message: Message) -> Result<Value, String> {
    match message {
        Message::Text(text) => serde_json::from_str(&text).map_err(|error| error.to_string()),
        Message::Close(_) => Err("session closed".into()),
        _ => Err("unexpected non-text Group Bridge frame".into()),
    }
}

fn route_key(local: &str, remote: &str, peer: &str) -> RouteKey {
    (
        local.trim().into(),
        remote.trim().into(),
        peer.trim().into(),
    )
}

fn live_routes() -> &'static Mutex<HashMap<RouteKey, RouteSender>> {
    LIVE_ROUTES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register(config: &RouteConfig, sender: RouteSender) {
    if let Ok(mut routes) = live_routes().lock() {
        routes.insert(
            route_key(
                &config.local_group_id,
                &config.remote_group_id,
                &config.remote_peer_id,
            ),
            sender,
        );
    }
}

fn unregister(config: &RouteConfig) {
    if let Ok(mut routes) = live_routes().lock() {
        routes.remove(&route_key(
            &config.local_group_id,
            &config.remote_group_id,
            &config.remote_peer_id,
        ));
    }
}

fn update_status(home: &HomeLayout, config: &RouteConfig, connected: bool, error: &str) {
    let now = utc_now();
    let _ = integration_state::global_update(home, STORE_KEY, |state| {
        let Some(trusts) = state["trusts"].as_array_mut() else {
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

fn nonempty(value: &Value, field: &str) -> Option<String> {
    value[field]
        .as_str()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::group_bridge_identity::authenticated_session_peer_id;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::accept_async;

    #[test]
    fn python_session_url_is_derived_from_http_endpoint() {
        assert_eq!(
            session_url("https://remote.example:9443/base?x=1").expect("url"),
            "wss://remote.example:9443/api/group-bridge/session/ws"
        );
    }

    #[test]
    fn inactive_or_incomplete_trust_is_not_started() {
        assert!(route_config(&json!({"status":"revoked"})).is_none());
        assert!(
            route_config(&json!({
                "status":"active","transport":"group_bridge_session",
                "trust_id":"t","group_id":"g","remote_group_id":"r","remote_peer_id":"p"
            }))
            .is_none()
        );
    }

    #[tokio::test]
    async fn python_signed_session_registers_a_live_request_route() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let identity = GroupBridgeIdentity::load_or_create(&home).expect("identity");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = accept_async(stream).await.expect("websocket");
            let hello = message_json(socket.next().await.expect("hello").expect("hello frame"))
                .expect("hello json");
            assert_eq!(
                authenticated_session_peer_id(&hello).as_deref(),
                Some(identity.peer_id.as_str())
            );
            assert_eq!(hello["target_group_id"], "g_remote");
            assert_eq!(hello["src_group_id"], "g_local");
            socket
                .send(Message::Text(
                    json!({"ok":true,"type":"ready"}).to_string().into(),
                ))
                .await
                .expect("ready");
            let request = loop {
                let frame = message_json(
                    socket
                        .next()
                        .await
                        .expect("request")
                        .expect("request frame"),
                )
                .expect("request json");
                if frame["type"] == "ping" {
                    socket
                        .send(Message::Text(json!({"type":"pong"}).to_string().into()))
                        .await
                        .expect("pong");
                    continue;
                }
                break frame;
            };
            assert_eq!(request["type"], "request");
            assert_eq!(request["op"], "remote_send");
            socket
                .send(Message::Text(
                    json!({
                        "type":"response",
                        "response_to":request["request_id"],
                        "result":{"ok":true,"event_id":"remote-event"}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("response");
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let config = RouteConfig {
            trust_id: "trust_test".into(),
            registration_id: "registration_test".into(),
            local_group_id: "g_local".into(),
            remote_group_id: "g_remote".into(),
            remote_peer_id: "peer_remote".into(),
            endpoint,
        };
        let (stop_tx, stop_rx) = watch::channel(false);
        let worker_home = home.clone();
        let worker_config = config.clone();
        let worker =
            tokio::spawn(async move { connect_once(&worker_home, &worker_config, stop_rx).await });
        for _ in 0..100 {
            if live_routes()
                .lock()
                .expect("routes")
                .contains_key(&route_key("g_local", "g_remote", "peer_remote"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let response = tokio::task::spawn_blocking(|| {
            send(
                "g_local",
                "g_remote",
                "peer_remote",
                json!({"op":"remote_send","payload":{"text":"hello"}}),
            )
        })
        .await
        .expect("send task")
        .expect("live response");
        assert_eq!(response["event_id"], "remote-event");
        let _ = stop_tx.send(true);
        let _ = worker.await.expect("worker");
        server.await.expect("server");
    }

    #[tokio::test]
    async fn worker_reconnects_and_projects_connection_health() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let config = RouteConfig {
            trust_id: "trust_reconnect".into(),
            registration_id: "registration_reconnect".into(),
            local_group_id: "g_local".into(),
            remote_group_id: "g_remote".into(),
            remote_peer_id: "peer_remote".into(),
            endpoint,
        };
        integration_state::global_update(&home, STORE_KEY, |state| {
            *state = json!({"trusts":[{
                "trust_id":config.trust_id.clone(),
                "registration_id":config.registration_id.clone(),
                "group_id":config.local_group_id.clone(),
                "remote_group_id":config.remote_group_id.clone(),
                "remote_peer_id":config.remote_peer_id.clone(),
                "remote_endpoint":config.endpoint.clone(),
                "transport":"group_bridge_session",
                "status":"active"
            }]});
            Ok(())
        })
        .expect("state");
        let server = tokio::spawn(async move {
            let (first, _) = listener.accept().await.expect("first accept");
            let mut first = accept_async(first).await.expect("first websocket");
            let _ = first.next().await;
            first.close(None).await.expect("first close");

            let (second, _) = listener.accept().await.expect("second accept");
            let mut second = accept_async(second).await.expect("second websocket");
            let _ = second.next().await.expect("second hello");
            second
                .send(Message::Text(
                    json!({"ok":true,"type":"ready"}).to_string().into(),
                ))
                .await
                .expect("ready");
            while second.next().await.is_some() {}
        });
        let (stop_tx, stop_rx) = watch::channel(false);
        let worker_home = home.clone();
        let worker_config = config.clone();
        let worker = tokio::spawn(async move {
            run_worker(worker_home, worker_config, stop_rx).await;
        });
        let mut connected = false;
        for _ in 0..250 {
            let state = integration_state::global_get(&home, STORE_KEY).expect("bridge state");
            if state["trusts"][0]["session_connected"] == true {
                connected = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(connected, "worker did not reconnect within the test window");
        let _ = stop_tx.send(true);
        worker.await.expect("worker");
        server.await.expect("server");
        let state = integration_state::global_get(&home, STORE_KEY).expect("bridge state");
        assert_eq!(state["trusts"][0]["session_connected"], false);
    }
}
