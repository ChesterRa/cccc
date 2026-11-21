use super::{call_tool, list_tools};
use crate::ops::codex_voice_analyst::managed_runtime;
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::post,
};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Capture {
    requests: Vec<Value>,
    sse: bool,
    expire_call: bool,
    rpc_error: Option<String>,
    hold_call: bool,
    release: Arc<tokio::sync::Notify>,
}

async fn endpoint(
    State(state): State<Arc<Mutex<Capture>>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let accept = headers
        .get("accept")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !accept.contains("application/json") || !accept.contains("text/event-stream") {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    let method = request["method"].as_str().unwrap_or("");
    let (sse, expire_call, rpc_error, hold_call, release) = {
        let mut captured = state.lock().expect("capture");
        captured.requests.push(json!({"request":request,
            "session":headers.get("mcp-session-id").and_then(|v|v.to_str().ok()),
            "authorization":headers.get("authorization").and_then(|v|v.to_str().ok())}));
        (
            captured.sse,
            captured.expire_call,
            captured.rpc_error.clone(),
            captured.hold_call,
            captured.release.clone(),
        )
    };
    if method == "notifications/initialized" {
        return StatusCode::ACCEPTED.into_response();
    }
    if method == "tools/call" && hold_call {
        release.notified().await;
        return StatusCode::GATEWAY_TIMEOUT.into_response();
    }
    if method == "tools/call" && expire_call {
        return StatusCode::NOT_FOUND.into_response();
    }
    if method == "tools/call" {
        if let Some(message) = &rpc_error {
            return Json(json!({"jsonrpc":"2.0","id":request["id"],
                "error":{"code":-32603,"message":message}}))
            .into_response();
        }
    }
    let result = match method {
        "initialize" => json!({"protocolVersion":request["params"]["protocolVersion"],
            "capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
        "tools/list" if request["params"]["cursor"] == "second" => json!({"tools":[{
            "name":"second-page","description":"Last discovery page", "inputSchema":{"type":"object"}}]}),
        "tools/list" => json!({"tools":[{"name":"echo","description":"Fixture read",
            "inputSchema":{"type":"object","properties":{"value":{"type":"integer"}}}}],"nextCursor":"second"}),
        "tools/call" => json!({"content":[{"type":"text","text":"fixture-marker"}],
            "structuredContent":{"value":request["params"]["arguments"]["value"]},"isError":false}),
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let payload = json!({"jsonrpc":"2.0","id":request["id"],"result":result});
    let mut builder = Response::builder().status(StatusCode::OK);
    if method == "initialize" {
        builder = builder.header("Mcp-Session-Id", "fixture-session");
    }
    if sse {
        builder
            .header("Content-Type", "text/event-stream")
            .body(Body::from(format!("event: message\ndata: {payload}\n\n")))
            .expect("SSE response")
            .into_response()
    } else {
        builder
            .header("Content-Type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("JSON response")
            .into_response()
    }
}

struct Fixture {
    url: String,
    state: Arc<Mutex<Capture>>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn new(sse: bool, expire_call: bool) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listen");
        listener.set_nonblocking(true).expect("nonblocking");
        let url = format!("http://{}/mcp", listener.local_addr().expect("address"));
        let state = Arc::new(Mutex::new(Capture {
            sse,
            expire_call,
            ..Default::default()
        }));
        let app = Router::new()
            .route(
                "/mcp",
                post(endpoint)
                    .get(|| async { StatusCode::METHOD_NOT_ALLOWED })
                    .delete(|| async { StatusCode::OK }),
            )
            .with_state(state.clone());
        let task = managed_runtime().spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("async listener");
            axum::serve(listener, app).await.expect("server");
        });
        Self { url, state, task }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.state.lock().expect("capture").release.notify_waiters();
        self.task.abort();
    }
}

#[test]
fn json_and_sse_round_trip_share_initialization_and_forward_the_session() {
    for sse in [false, true] {
        let fixture = Fixture::new(sse, false);
        let tools = list_tools(&fixture.url, Duration::from_secs(3)).expect("tools");
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[1]["name"], "second-page");
        assert_eq!(tools.len(), 2);
        let result = call_tool(
            &fixture.url,
            None,
            "echo",
            json!({"value":42}).as_object().expect("args").clone(),
            Duration::from_secs(3),
        )
        .expect("call");
        assert_eq!(result["structuredContent"]["value"], 42);
        assert_eq!(result["isError"], false);
        let captured = fixture.state.lock().expect("capture");
        let calls = captured
            .requests
            .iter()
            .filter(|row| row["request"]["method"] == "tools/call")
            .count();
        assert_eq!(calls, 1);
        for row in &captured.requests {
            if row["request"]["method"] != "initialize" {
                assert_eq!(row["session"], "fixture-session");
            }
        }
    }
}

#[test]
fn an_expired_tool_request_is_not_reinitialized_or_replayed() {
    let fixture = Fixture::new(true, true);
    let result = call_tool(
        &fixture.url,
        None,
        "echo",
        Map::new(),
        Duration::from_secs(3),
    );
    assert!(result.is_err());
    let captured = fixture.state.lock().expect("capture");
    for method in ["initialize", "tools/call"] {
        assert_eq!(
            captured
                .requests
                .iter()
                .filter(|row| row["request"]["method"] == method)
                .count(),
            1,
            "{method}"
        );
    }
}

#[test]
fn remote_errors_preserve_the_error_kind_without_returning_known_credentials() {
    let fixture = Fixture::new(false, false);
    let url = format!("{}?key=fixture-query-secret", fixture.url);
    fixture.state.lock().expect("capture").rpc_error = Some(format!(
        "remote rejected {url}; bearer fixture-bearer-secret"
    ));
    let error = call_tool(
        &url,
        Some("fixture-bearer-secret"),
        "echo",
        Map::new(),
        Duration::from_secs(3),
    )
    .expect_err("remote error");
    assert!(error.remote, "{error:?}");
    assert!(!error.message.contains("fixture-query-secret"));
    assert!(!error.message.contains("fixture-bearer-secret"));
    assert!(error.message.contains("remote rejected"));
    let captured = fixture.state.lock().expect("capture");
    assert!(
        captured
            .requests
            .iter()
            .all(|row| row["authorization"] == "Bearer fixture-bearer-secret")
    );
}

#[test]
fn a_hung_tool_call_times_out_without_a_second_post() {
    let fixture = Fixture::new(true, false);
    fixture.state.lock().expect("capture").hold_call = true;
    let started = Instant::now();
    let error = call_tool(
        &fixture.url,
        None,
        "echo",
        Map::new(),
        Duration::from_secs(1),
    )
    .expect_err("deadline");
    assert!(!error.remote);
    assert!(
        started.elapsed() < Duration::from_secs(9),
        "operation and cleanup must be bounded"
    );
    let captured = fixture.state.lock().expect("capture");
    assert_eq!(
        captured
            .requests
            .iter()
            .filter(|row| row["request"]["method"] == "tools/call")
            .count(),
        1
    );
}

#[test]
fn a_short_url_value_inside_the_token_cannot_leave_pieces_of_it() {
    // Replacing the query value first would cut the token apart and expose the pieces.
    let url = "https://mcp.example.test/mcp?v=2";
    let token = "ghp_2Ab2Cd2";
    let message = format!("remote rejected {url}; bearer {token}");
    assert_eq!(
        super::redact_transport_error(&message, url, Some(token)),
        "remote rejected <MCP endpoint>; bearer <redacted>"
    );
}
