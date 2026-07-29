use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct RouteKey {
    home: String,
    group_id: String,
    remote_group_id: String,
    remote_peer_id: String,
}

#[derive(Debug)]
struct PendingRequest {
    request_id: String,
    frame: Value,
}

#[derive(Debug)]
struct Session {
    generation: String,
    queue: VecDeque<PendingRequest>,
    in_flight: HashSet<String>,
    responses: HashMap<String, Value>,
}

#[derive(Default)]
struct Runtime {
    sessions: HashMap<RouteKey, Session>,
}

fn runtime() -> &'static (Mutex<Runtime>, Condvar) {
    static RUNTIME: OnceLock<(Mutex<Runtime>, Condvar)> = OnceLock::new();
    RUNTIME.get_or_init(|| (Mutex::new(Runtime::default()), Condvar::new()))
}

fn key(home: &HomeLayout, request: &DaemonRequest) -> Result<RouteKey, OpError> {
    Ok(RouteKey {
        home: home.root().to_string_lossy().into_owned(),
        group_id: required_arg(request, "group_id")?,
        remote_group_id: required_arg(request, "remote_group_id")?,
        remote_peer_id: required_arg(request, "remote_peer_id")?,
    })
}

pub(super) fn open(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let key = key(home, request)?;
    let generation = Uuid::new_v4().simple().to_string();
    let (lock, changed) = runtime();
    let mut state = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.sessions.insert(
        key,
        Session {
            generation: generation.clone(),
            queue: VecDeque::new(),
            in_flight: HashSet::new(),
            responses: HashMap::new(),
        },
    );
    changed.notify_all();
    object(json!({"generation":generation,"ready":true}))
}

pub(super) fn close(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let key = key(home, request)?;
    let generation = required_arg(request, "generation")?;
    let (lock, changed) = runtime();
    let mut state = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let closed = state
        .sessions
        .get(&key)
        .is_some_and(|session| session.generation == generation);
    if closed {
        state.sessions.remove(&key);
        changed.notify_all();
    }
    object(json!({"closed":closed,"ready":false}))
}

pub(super) fn ready(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let key = key(home, request)?;
    let state = runtime()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    object(json!({"ready":state.sessions.contains_key(&key)}))
}

pub(super) fn poll(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let key = key(home, request)?;
    let generation = required_arg(request, "generation")?;
    let timeout = bounded_timeout(request, 250, 1_000);
    let deadline = Instant::now() + timeout;
    let (lock, changed) = runtime();
    let mut state = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        let Some(session) = state.sessions.get_mut(&key) else {
            return Err(OpError::new(
                "peer_session_unavailable",
                "no active Group Bridge WebSocket session",
            ));
        };
        if session.generation != generation {
            return Err(OpError::new(
                "peer_session_failed",
                "Group Bridge session was replaced",
            ));
        }
        if let Some(pending) = session.queue.pop_front() {
            return object(json!({"request":pending.frame}));
        }
        let now = Instant::now();
        if now >= deadline {
            return object(json!({"request":Value::Null}));
        }
        let wait = deadline.saturating_duration_since(now);
        let result = changed
            .wait_timeout(state, wait)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state = result.0;
    }
}

pub(super) fn complete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let key = key(home, request)?;
    let generation = required_arg(request, "generation")?;
    let response_to = required_arg(request, "response_to")?;
    let result = request
        .args
        .get("result")
        .cloned()
        .unwrap_or_else(|| json!({"ok":false}));
    let (lock, changed) = runtime();
    let mut state = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let session = state.sessions.get_mut(&key).ok_or_else(|| {
        OpError::new(
            "peer_session_unavailable",
            "no active Group Bridge WebSocket session",
        )
    })?;
    if session.generation != generation {
        return Err(OpError::new(
            "peer_session_failed",
            "Group Bridge session was replaced",
        ));
    }
    let completed = session.in_flight.contains(&response_to);
    if completed {
        session.responses.insert(response_to, result);
    }
    changed.notify_all();
    object(json!({"completed":completed}))
}

pub(super) fn deliver(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let key = key(home, request)?;
    let timeout = bounded_timeout(request, 5_000, 30_000);
    let request_id = Uuid::new_v4().simple().to_string();
    let payload = request
        .args
        .get("payload")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let idempotency_key = request
        .args
        .get("idempotency_key")
        .cloned()
        .unwrap_or(Value::Null);
    let frame = json!({
        "type":"request","request_id":request_id,"op":"remote_send",
        "target_group_id":key.remote_group_id,"src_group_id":key.group_id,
        "idempotency_key":idempotency_key,"payload":payload
    });
    let deadline = Instant::now() + timeout;
    let (lock, changed) = runtime();
    let mut state = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let session = state.sessions.get_mut(&key).ok_or_else(|| {
        OpError::new(
            "peer_session_unavailable",
            "no active Group Bridge WebSocket session",
        )
    })?;
    let generation = session.generation.clone();
    session.in_flight.insert(request_id.clone());
    session.queue.push_back(PendingRequest {
        request_id: request_id.clone(),
        frame,
    });
    changed.notify_all();
    loop {
        let Some(session) = state.sessions.get_mut(&key) else {
            return Err(OpError::new(
                "peer_session_unavailable",
                "Group Bridge WebSocket session disconnected",
            ));
        };
        if session.generation != generation {
            return Err(OpError::new(
                "peer_session_failed",
                "Group Bridge session was replaced",
            ));
        }
        if let Some(result) = session.responses.remove(&request_id) {
            session.in_flight.remove(&request_id);
            return result.as_object().cloned().ok_or_else(|| {
                OpError::new(
                    "peer_session_failed",
                    "Group Bridge session returned an invalid response",
                )
            });
        }
        let now = Instant::now();
        if now >= deadline {
            session
                .queue
                .retain(|pending| pending.request_id != request_id);
            session.in_flight.remove(&request_id);
            return Err(OpError::new(
                "peer_session_timeout",
                "Group Bridge WebSocket session timed out",
            ));
        }
        let result = changed
            .wait_timeout(state, deadline.saturating_duration_since(now))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state = result.0;
    }
}

fn bounded_timeout(request: &DaemonRequest, default_ms: u64, max_ms: u64) -> Duration {
    Duration::from_millis(
        request
            .args
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(default_ms)
            .clamp(1, max_ms),
    )
}

pub(super) fn route_ready(home: &HomeLayout, trust: &Value) -> bool {
    let has_credential = trust["credential"]
        .as_str()
        .map(str::trim)
        .is_some_and(|credential| !credential.is_empty());
    if has_credential
        && trust["remote_endpoint"]
            .as_str()
            .map(str::trim)
            .is_some_and(|endpoint| {
                endpoint.starts_with("http://") || endpoint.starts_with("https://")
            })
    {
        return true;
    }
    let Some(group_id) = trust["group_id"].as_str() else {
        return false;
    };
    let Some(remote_group_id) = trust["remote_group_id"].as_str() else {
        return false;
    };
    let Some(remote_peer_id) = trust["remote_peer_id"].as_str() else {
        return false;
    };
    let key = RouteKey {
        home: home.root().to_string_lossy().into_owned(),
        group_id: group_id.trim().into(),
        remote_group_id: remote_group_id.trim().into(),
        remote_peer_id: remote_peer_id.trim().into(),
    };
    runtime()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .contains_key(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn home() -> (tempfile::TempDir, HomeLayout) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        (temp, home)
    }

    fn request(op: &str, value: Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: op.into(),
            args: value.as_object().cloned().unwrap_or_default(),
        }
    }

    fn route() -> Value {
        json!({"group_id":"g_local","remote_group_id":"g_remote","remote_peer_id":"peer"})
    }

    #[test]
    fn readiness_matrix_uses_endpoint_or_live_session() {
        let (_temp, home) = home();
        let no_endpoint =
            json!({"group_id":"g_local","remote_group_id":"g_remote","remote_peer_id":"peer"});
        let endpoint = json!({"group_id":"g_local","remote_group_id":"g_remote","remote_peer_id":"peer","remote_endpoint":"https://remote.example","credential":"secret"});
        assert!(!route_ready(&home, &no_endpoint));
        assert!(route_ready(&home, &endpoint));
        let opened = open(&home, &request("open", route())).expect("open");
        assert!(route_ready(&home, &no_endpoint));
        assert!(route_ready(&home, &endpoint));
        let mut close_args = route();
        close_args["generation"] = opened["generation"].clone();
        close(&home, &request("close", close_args)).expect("close");
        assert!(!route_ready(&home, &no_endpoint));
        assert!(route_ready(&home, &endpoint));
    }

    #[test]
    fn disconnect_and_reconnect_are_generation_guarded_and_immediately_deliverable() {
        let (_temp, home) = home();
        let first = open(&home, &request("open", route())).expect("first");
        let second = open(&home, &request("open", route())).expect("second");
        let mut stale_close = route();
        stale_close["generation"] = first["generation"].clone();
        assert_eq!(
            close(&home, &request("close", stale_close)).expect("stale")["closed"],
            false
        );
        assert!(route_ready(&home, &route()));
        let mut active_close = route();
        active_close["generation"] = second["generation"].clone();
        close(&home, &request("close", active_close)).expect("active close");
        assert!(!route_ready(&home, &route()));
        open(&home, &request("open", route())).expect("reopen");
        assert!(route_ready(&home, &route()));
    }

    #[test]
    fn delivery_maps_unavailable_timeout_and_replacement_failure() {
        let (_temp, home) = home();
        let mut delivery = route();
        delivery["payload"] = json!({"text":"hello"});
        delivery["timeout_ms"] = json!(20);
        let unavailable =
            deliver(&home, &request("deliver", delivery.clone())).expect_err("unavailable");
        assert_eq!(unavailable.code, "peer_session_unavailable");
        let opened = open(&home, &request("open", route())).expect("open");
        let timeout = deliver(&home, &request("deliver", delivery.clone())).expect_err("timeout");
        assert_eq!(timeout.code, "peer_session_timeout");

        let home_for_delivery = home.clone();
        let mut slow = delivery;
        slow["timeout_ms"] = json!(1_000);
        let task = thread::spawn(move || deliver(&home_for_delivery, &request("deliver", slow)));
        let mut poll_args = route();
        poll_args["generation"] = opened["generation"].clone();
        poll_args["timeout_ms"] = json!(500);
        poll(&home, &request("poll", poll_args)).expect("queued request");
        open(&home, &request("open", route())).expect("replace");
        let failed = task.join().expect("join").expect_err("replacement failure");
        assert_eq!(failed.code, "peer_session_failed");
    }

    #[test]
    fn unknown_or_late_responses_are_not_retained() {
        let (_temp, home) = home();
        let route_key = key(&home, &request("key", route())).expect("route key");
        let opened = open(&home, &request("open", route())).expect("open");
        let mut completion = route();
        completion["generation"] = opened["generation"].clone();
        completion["response_to"] = json!("unknown-request");
        completion["result"] = json!({"ok":true});
        assert_eq!(
            complete(&home, &request("complete", completion)).expect("complete")["completed"],
            false
        );

        let state = runtime()
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = state.sessions.get(&route_key).expect("session");
        assert!(session.responses.is_empty());
    }
}
