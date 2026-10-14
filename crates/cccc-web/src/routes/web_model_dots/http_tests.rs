//! Shared-entrance tests use an owned Home and in-process HTTP router. No
//! browser, provider CLI, callback connection or global environment is used.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, access_tokens::AccessTokenStore, actors, ledger};
use std::time::Duration;
use tower::ServiceExt;

struct Fixture {
    _temp: tempfile::TempDir,
    state: AppState,
    router: Router,
    group_id: String,
    connector_id: String,
    secret: String,
    admin: String,
    daemon: Option<tokio::task::JoinHandle<()>>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Shared ChatGPT", "").expect("group");
        for (id, runtime) in [
            ("browser", ActorRuntime::WebModel),
            ("dots", ActorRuntime::ChatgptDots),
        ] {
            let mut actor = Actor::new(id);
            actor.runtime = runtime;
            actor.enabled = false;
            actors::add(&mut group, actor).expect("actor");
        }
        store.save(&group).expect("save");
        let configured = connectors::configure(&home).expect("connector");
        let connector_id = configured["connector"]["connector_id"]
            .as_str()
            .expect("id")
            .to_owned();
        let secret = configured["secret"].as_str().expect("secret").to_owned();
        connectors::bind_dots(
            &home,
            &connector_id,
            &group.group_id,
            "dots",
            &group.actors[1].generation,
        )
        .expect("Dots binding");
        let metadata =
            json!({"openai/session":"synthetic-browser-session","openai/subject":"synthetic-user"});
        let session_key =
            connectors::session_key(&configured["connector"], &metadata).expect("session key");
        let pair = connectors::begin_pairing(
            &home,
            &connector_id,
            &group.group_id,
            "browser",
            &group.actors[0].generation,
            false,
        )
        .expect("pair");
        connectors::accept_pairing(
            &home,
            &connector_id,
            pair["code"].as_str().expect("code"),
            &session_key,
        )
        .expect("accept");
        connectors::confirm_pairing(
            &home,
            &connector_id,
            &group.group_id,
            "browser",
            &group.actors[0].generation,
            pair["pairing_id"].as_str().expect("pair id"),
            "https://chatgpt.com/c/synthetic-browser",
        )
        .expect("browser binding");
        let admin = AccessTokenStore::new(home.clone())
            .expect("access store")
            .create("admin", Vec::new(), true, None)
            .expect("admin")
            .token;
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (router, _, _, state) = crate::app_with_shutdown(
            home,
            shutdown,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "dots-http-fixture".into(),
        );
        Self {
            _temp: temp,
            state,
            router,
            group_id: group.group_id,
            connector_id,
            secret,
            admin,
            daemon: None,
        }
    }

    async fn start_daemon(&mut self) {
        let home = self.state.home.clone();
        self.daemon = Some(tokio::spawn(async move {
            cccc_daemon::run(home).await.expect("owned daemon");
        }));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self
                    .state
                    .client
                    .call(&DaemonRequest {
                        v: 1,
                        op: "ping".into(),
                        args: Default::default(),
                    })
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("daemon ready");
    }

    fn connector(&self) -> Value {
        connectors::load(&self.state.home)
            .expect("load")
            .into_iter()
            .find(|connector| connector["connector_id"] == self.connector_id)
            .expect("connector")
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&body).expect("JSON response"),
        )
    }

    fn mcp_request(&self, method: &str, mut params: Value, modern: bool) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/mcp/web-model/{}", self.connector_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", self.secret));
        if modern {
            params["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(protocol::VERSION);
            params["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!({});
            builder = builder
                .header("mcp-protocol-version", protocol::VERSION)
                .header("mcp-method", method);
            if method == "tools/call" {
                builder = builder.header("mcp-name", params["name"].as_str().expect("tool name"));
            }
        }
        builder
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":"fixture-request","method":method,"params":params})
                    .to_string(),
            ))
            .expect("request")
    }

    async fn mcp(&self, method: &str, params: Value, modern: bool) -> (StatusCode, Value) {
        self.send(self.mcp_request(method, params, modern)).await
    }

    async fn admin_get(&self, uri: &str) -> (StatusCode, Value) {
        self.send(
            Request::builder()
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", self.admin))
                .body(Body::empty())
                .expect("request"),
        )
        .await
    }

    async fn close(&mut self) {
        let _ = self.state.shutdown.send(());
        if let Some(daemon) = self.daemon.take() {
            self.state
                .client
                .call(&DaemonRequest {
                    v: 1,
                    op: "shutdown".into(),
                    args: Default::default(),
                })
                .await
                .expect("owned shutdown");
            tokio::time::timeout(Duration::from_secs(5), daemon)
                .await
                .expect("shutdown timeout")
                .expect("daemon task");
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.state.shutdown.send(());
        if let Some(daemon) = self.daemon.take() {
            daemon.abort();
        }
    }
}

#[tokio::test]
async fn authenticated_modern_scan_and_legacy_catalog_share_the_same_connector() {
    let mut fixture = Fixture::new();
    for method in ["server/discover", "events/list", "tools/list"] {
        let (status, response) = fixture.mcp(method, json!({}), true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["id"], "fixture-request");
        assert_eq!(response["result"]["resultType"], "complete");
        assert_eq!(response["result"]["cacheScope"], "private");
        match method {
            "server/discover" => {
                assert_eq!(
                    response["result"]["supportedVersions"],
                    json!([protocol::VERSION])
                );
                assert!(response["result"]["capabilities"]["events"].is_object());
            }
            "events/list" => {
                let event = &response["result"]["events"][0];
                assert_eq!(event["name"], EVENT_NAME);
                let description = event["inputSchema"]["properties"]["actor_token"]["description"]
                    .as_str()
                    .expect("subscription credential purpose");
                assert!(description.contains("subscription only"));
                assert!(description.contains("cannot claim work"));
            }
            _ => assert!(
                response["result"]["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
            ),
        }
    }
    let (status, legacy) = fixture.mcp("tools/list", json!({}), false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(legacy["result"]["resultType"].is_null());
    let (_, modern) = fixture.mcp("tools/list", json!({}), true).await;
    assert_eq!(legacy["result"]["tools"], modern["result"]["tools"]);
    for tool in legacy["result"]["tools"].as_array().expect("tools") {
        if matches!(
            tool["name"].as_str(),
            Some("cccc_pair" | "cccc_runtime_wait_next_turn")
        ) {
            assert!(tool["inputSchema"]["properties"]["actor_token"].is_null());
        } else {
            assert!(tool["inputSchema"]["properties"]["actor_token"].is_object());
            assert!(
                !tool["inputSchema"]["required"]
                    .as_array()
                    .is_some_and(|required| required.contains(&json!("actor_token")))
            );
        }
    }
    assert!(fixture.connector()["dots"]["wake"].is_null());
    fixture.close().await;
}

#[tokio::test]
async fn shared_mcp_authentication_is_required_for_discovery_and_event_catalogs() {
    let mut fixture = Fixture::new();
    for method in ["server/discover", "events/list"] {
        let mut request = fixture.mcp_request(method, json!({}), true);
        request.headers_mut().remove(header::AUTHORIZATION);
        let (status, _) = fixture.send(request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let mut request = fixture.mcp_request(method, json!({}), true);
        request.headers_mut().insert(
            header::AUTHORIZATION,
            "Bearer invalid-synthetic-credential"
                .parse()
                .expect("header"),
        );
        let (status, _) = fixture.send(request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    fixture.close().await;
}

#[tokio::test]
async fn invalid_event_credentials_never_fall_back_to_a_paired_browser_session() {
    let mut fixture = Fixture::new();
    for token in [Value::Null, json!("invalid-event-credential")] {
        let arguments = if token.is_null() {
            json!({})
        } else {
            json!({"actor_token":token})
        };
        let metadata = if token.is_null() {
            json!({})
        } else {
            json!({"openai/session":"synthetic-browser-session","openai/subject":"synthetic-user"})
        };
        let (status,response) = fixture.mcp("tools/call",json!({"name":"cccc_runtime_wait_next_turn","arguments":arguments,"_meta":metadata}),true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["structuredContent"]["turn"].is_null());
    }
    let connector = fixture.connector();
    let binding = connectors::binding_for_actor(
        &connector,
        &fixture.group_id,
        "dots",
        connector["dots"]["generation"]
            .as_str()
            .expect("generation"),
    )
    .expect("binding");
    let subscription =
        connectors::dots_subscription_token(&connector, &binding).expect("subscription token");
    let (_, response) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_runtime_wait_next_turn","arguments":{"actor_token":subscription}}),
            true,
        )
        .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(fixture.connector()["dots"]["execution"].is_null());
    fixture.close().await;
}

#[tokio::test]
async fn rejected_pairing_remains_in_shared_diagnostics_after_status_reads() {
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    let pair = connectors::begin_dots_pairing(&fixture.state.home, &fixture.connector_id)
        .expect("setup")
        .expect("new pair");
    connectors::dots_update(&fixture.state.home, &fixture.connector_id, |connector| {
        let key = json!([fixture.group_id, "dots"]).to_string();
        connector["pairings"][key]["expires_at_ms"] = json!(0);
        Ok(())
    })
    .expect("expire fixture code");
    let metadata = json!({"openai/session":"synthetic-dot-session"});
    let (_, rejected) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_pair","arguments":{"code":pair["code"]},"_meta":metadata}),
            true,
        )
        .await;
    assert_eq!(rejected["result"]["isError"], true);
    assert_eq!(fixture.connector()["last_error"], "pairing_expired");
    let (_, observed) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_connector_status","arguments":{},"_meta":metadata}),
            true,
        )
        .await;
    assert_ne!(observed["result"]["isError"], true);
    let (_, listed) = fixture.admin_get("/api/v1/web-model/connectors").await;
    let log = &listed["result"]["connectors"][0]["events_log"];
    assert_eq!(log.as_array().expect("diagnostics").len(), 1);
    assert_eq!(log[0]["tool_name"], "cccc_pair");
    assert_eq!(log[0]["ok"], false);
    assert_eq!(log[0]["reason"], "pairing_expired");
    assert!(
        !listed
            .to_string()
            .contains(pair["code"].as_str().expect("code"))
    );
    assert!(!listed.to_string().contains(&fixture.secret));
    assert!(fixture.connector()["dots"]["execution"].is_null());
    fixture.close().await;
}

#[tokio::test]
async fn paired_dot_status_waits_for_subscription_without_creating_work() {
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    let pair = connectors::begin_dots_pairing(&fixture.state.home, &fixture.connector_id)
        .expect("setup")
        .expect("new Dot pair");
    let metadata = json!({"openai/session":"synthetic-dot-session"});
    let (_, paired) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_pair",
        "arguments":{"code":pair["code"]},"_meta":metadata}),
            true,
        )
        .await;
    assert_eq!(paired["result"]["structuredContent"]["state"], "bound");
    let groups = GroupStore::new(fixture.state.home.clone()).expect("store");
    groups
        .mutate(&fixture.group_id, |group| {
            group.running = true;
            group.state = cccc_contracts::GroupState::Active;
            group.actors[1].enabled = true;
            Ok(())
        })
        .expect("enable Dot");
    let before = fixture.connector()["dots"].clone();
    let ledger_before = std::fs::read(groups.ledger_path(&fixture.group_id).expect("ledger"))
        .expect("ledger bytes");
    let (_, response) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_connector_status",
        "arguments":{},"_meta":metadata}),
            true,
        )
        .await;
    assert_eq!(
        response["result"]["structuredContent"]["state"],
        "waiting_subscription"
    );
    assert_eq!(response["result"]["structuredContent"]["paired"], true);
    assert!(fixture.connector()["dots"] == before);
    assert_eq!(
        std::fs::read(groups.ledger_path(&fixture.group_id).expect("ledger"))
            .expect("after status"),
        ledger_before
    );
    fixture.close().await;
}

#[tokio::test]
async fn paired_dot_claims_successive_turns_without_saved_credentials_or_event_payload() {
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    let store = GroupStore::new(fixture.state.home.clone()).expect("store");
    store
        .mutate(&fixture.group_id, |group| {
            group.running = true;
            group.state = cccc_contracts::GroupState::Active;
            group.actors[1].enabled = true;
            Ok(())
        })
        .expect("enable Dots");
    connectors::dots_update(&fixture.state.home, &fixture.connector_id, |connector| {
        connector["dots"]["subscription"] = json!({"subscription_id":"fixture-sub","enabled":true,
            "callback_url":"https://callback.invalid/fixture","signing_secret":"synthetic-only-secret","expires_at_ms":null});
        Ok(())
    }).expect("subscription");
    // A fresh background run only receives the saved handler. Subscription
    // arguments, setup chat history and event data are not available to it.
    let (_, setup) = fixture
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/web-model/dots")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                .body(Body::from(
                    json!({"group_id":fixture.group_id,"actor_id":"dots","action":"connect"})
                        .to_string(),
                ))
                .expect("setup request"),
        )
        .await;
    assert_eq!(setup["ok"], true);
    let instructions = setup["result"]["instructions"]
        .as_str()
        .expect("instructions");
    assert!(
        instructions.contains("cccc_pair"),
        "setup must explicitly pair the Dot conversation before background claims"
    );
    let handler = instructions
        .split_once("BEGIN CCCC DOTS HANDLER\n")
        .expect("setup must provide a self-contained handler")
        .1
        .split_once("\nEND CCCC DOTS HANDLER")
        .expect("handler boundary")
        .0;
    assert!(
        handler.lines().any(|line| line == "{}"),
        "saved handler must claim without a literal persistent credential"
    );
    let saved_task = serde_json::to_vec(&json!({"instructions":handler})).expect("save handler");
    let saved: Value = serde_json::from_slice(&saved_task).expect("restore handler");
    let claim_args: Value = serde_json::from_str(
        saved["instructions"]
            .as_str()
            .expect("saved instructions")
            .lines()
            .find(|line| *line == "{}")
            .expect("credential-free claim arguments"),
    )
    .expect("claim arguments");
    assert_eq!(claim_args, json!({}));
    let connector = fixture.connector();
    let route = connectors::binding_for_actor(
        &connector,
        &fixture.group_id,
        "dots",
        connector["dots"]["generation"]
            .as_str()
            .expect("generation"),
    )
    .expect("Dots route");
    let grant =
        connectors::dots_subscription_token(&connector, &route).expect("subscription grant");
    assert!(
        !handler.contains(&grant),
        "saved task instructions must contain no permanent credential"
    );
    let metadata =
        json!({"openai/session":"synthetic-dot-session","openai/subject":"synthetic-user"});
    let (_, unpaired) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_runtime_wait_next_turn","arguments":{},"_meta":metadata}),
            true,
        )
        .await;
    assert_eq!(unpaired["result"]["isError"], true);
    let pairing_args: Value = serde_json::from_str(
        instructions
            .lines()
            .find(|line| line.starts_with("{\"code\":"))
            .expect("setup-only pairing code"),
    )
    .expect("pairing arguments");
    // The operator may pause/resume access while following the copied setup.
    // That revokes task execution authority, not this unexpired Actor pairing.
    let issued_epoch = fixture.connector()["dots"]["epoch"].clone();
    for op in ["actor_stop", "actor_start"] {
        let response = fixture
            .state
            .client
            .call(&DaemonRequest {
                v: 1,
                op: op.into(),
                args: json!({"group_id":fixture.group_id,"actor_id":"dots","by":"user"})
                    .as_object()
                    .expect("lifecycle request")
                    .clone(),
            })
            .await
            .expect("owned lifecycle response");
        assert!(response.ok, "{op} must succeed");
    }
    assert!(fixture.connector()["dots"]["epoch"] != issued_epoch);
    let ledger_before =
        std::fs::read(store.ledger_path(&fixture.group_id).expect("ledger")).expect("ledger bytes");
    let (_, paired) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_pair","arguments":pairing_args,"_meta":metadata}),
            true,
        )
        .await;
    assert_ne!(
        paired["result"]["isError"], true,
        "explicit setup pairs the Dot without a browser"
    );
    assert_eq!(paired["result"]["structuredContent"]["state"], "bound");
    assert_eq!(
        paired["result"]["structuredContent"]["routing_mode"],
        "mcp_events"
    );
    assert!(fixture.connector()["dots"]["execution"].is_null());
    assert_eq!(
        std::fs::read(store.ledger_path(&fixture.group_id).expect("ledger"))
            .expect("after pairing"),
        ledger_before
    );
    for tool in [
        "cccc_help",
        "cccc_context_get",
        "cccc_code_exec",
        "cccc_runtime_complete_turn",
    ] {
        for arguments in [json!({}), json!({"actor_token":grant})] {
            let (_, response) = fixture
                .mcp(
                    "tools/call",
                    json!({"name":tool,"arguments":arguments,"_meta":metadata}),
                    true,
                )
                .await;
            assert_eq!(
                response["result"]["isError"], true,
                "pairing or subscription cannot authorize {tool}"
            );
        }
    }
    for invalid_metadata in [
        json!({}),
        json!({"openai/session":"unpaired-dot"}),
        json!({"openai/session":"synthetic-dot-session","openai/subject":"another-user"}),
    ] {
        let (_, rejected) = fixture.mcp("tools/call", json!({"name":"cccc_runtime_wait_next_turn","arguments":{},"_meta":invalid_metadata}), true).await;
        assert_eq!(rejected["result"]["isError"], true);
        assert!(rejected["result"]["structuredContent"]["turn"].is_null());
    }
    let mut tokens = Vec::new();
    for text in ["first Dots packet", "second Dots packet"] {
        let mut event = Event::new("chat.message", &fixture.group_id);
        event.by = "user".into();
        event.data.insert("to".into(), json!(["dots"]));
        event.data.insert("message_mode".into(), json!("send"));
        event.data.insert("text".into(), json!(text));
        ledger::append(
            &store.ledger_path(&fixture.group_id).expect("ledger"),
            &event,
        )
        .expect("seed");
        let wake = fixture
            .state
            .client
            .call(&DaemonRequest {
                v: 1,
                op: "dots_events_next_delivery".into(),
                args: json!({"by":"user"}).as_object().expect("args").clone(),
            })
            .await
            .expect("wake");
        let delivery = &wake.result["delivery"];
        assert!(
            delivery["event"]["data"]["actor_token"].is_null(),
            "events must carry no execution credential"
        );
        assert!(delivery["event"]["data"]["event_id"].is_string());
        // Getting corrected instructions for an already subscribed, woken Actor
        // must preserve the binding, queued source, wake and execution epoch.
        let before = fixture.connector()["dots"].clone();
        let ledger_before = std::fs::read(store.ledger_path(&fixture.group_id).expect("ledger"))
            .expect("ledger bytes");
        let (_, restored) = fixture
            .send(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/web-model/dots")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                    .body(Body::from(
                        json!({"group_id":fixture.group_id,"actor_id":"dots","action":"connect"})
                            .to_string(),
                    ))
                    .expect("restore request"),
            )
            .await;
        assert_eq!(restored["ok"], true);
        let restored_handler = restored["result"]["instructions"]
            .as_str()
            .expect("restored instructions")
            .split_once("BEGIN CCCC DOTS HANDLER\n")
            .expect("handler start")
            .1
            .split_once("\nEND CCCC DOTS HANDLER")
            .expect("handler end")
            .0;
        assert!(restored_handler == handler);
        assert!(
            !restored["result"]["instructions"]
                .as_str()
                .expect("instructions")
                .contains("wm_pair_")
        );
        assert!(fixture.connector()["dots"] == before);
        assert_eq!(
            std::fs::read(store.ledger_path(&fixture.group_id).expect("ledger"))
                .expect("ledger bytes"),
            ledger_before
        );
        // Load the handler afresh for every independent run; never inject the
        // private connector's subscription arguments into the runtime context.
        let restored_task: Value = serde_json::from_slice(&saved_task).expect("new background run");
        let arguments: Value = serde_json::from_str(
            restored_task["instructions"]
                .as_str()
                .expect("saved instructions")
                .lines()
                .find(|line| *line == "{}")
                .expect("credential-free saved claim arguments"),
        )
        .expect("claim arguments");
        let (_, response) = fixture
            .mcp(
                "tools/call",
                json!({"name":"cccc_runtime_wait_next_turn",
            "arguments":arguments,"_meta":metadata}),
                true,
            )
            .await;
        assert_ne!(
            response["result"]["isError"], true,
            "claim must work without receiving event data"
        );
        let packet = &response["result"]["structuredContent"];
        assert_eq!(packet["status"], "work_available");
        let token = packet["actor_token"]
            .as_str()
            .expect("execution capability")
            .to_owned();
        assert_eq!(token.len(), 43);
        assert!(
            token != grant,
            "execution and subscription credentials must differ"
        );
        assert!(packet["turn"].to_string().contains(text));
        let (_, duplicate) = fixture
            .mcp(
                "tools/call",
                json!({"name":"cccc_runtime_wait_next_turn",
            "arguments":{},"_meta":metadata}),
                true,
            )
            .await;
        assert_eq!(
            duplicate["result"]["structuredContent"]["status"],
            "turn_in_progress"
        );
        assert!(duplicate["result"]["structuredContent"]["turn"].is_null());
        assert!(duplicate["result"]["structuredContent"]["actor_token"].is_null());
        if let Some(old) = tokens.last() {
            let (_, stale) = fixture
                .mcp(
                    "tools/call",
                    json!({"name":"cccc_context_get",
                "arguments":{"actor_token":old}}),
                    true,
                )
                .await;
            assert_eq!(stale["result"]["isError"], true);
        }
        let (_, completed) = fixture
            .mcp(
                "tools/call",
                json!({"name":"cccc_runtime_complete_turn",
            "arguments":{"actor_token":token,"turn_id":packet["turn"]["turn_id"],
                "event_ids":packet["turn"]["event_ids"],"status":"done"}}),
                true,
            )
            .await;
        assert_ne!(completed["result"]["isError"], true);
        tokens.push(token);
    }
    assert!(
        tokens[0] != tokens[1],
        "each turn needs a distinct credential"
    );
    let (_, public) = fixture
        .admin_get(&format!(
            "/api/v1/web-model/dots?group_id={}&actor_id=dots",
            fixture.group_id
        ))
        .await;
    let ledger = std::fs::read_to_string(store.ledger_path(&fixture.group_id).expect("ledger"))
        .expect("read ledger");
    let (_, diagnostics) = fixture.admin_get("/api/v1/web-model/connectors").await;
    let log = diagnostics["result"]["connectors"][0]["events_log"]
        .as_array()
        .expect("shared call history");
    assert!(log.len() <= 10);
    let last = log.last().expect("completion recorded");
    assert_eq!(last["tool_name"], "cccc_runtime_complete_turn");
    assert_eq!(last["ok"], true);
    for private in tokens.iter().chain(std::iter::once(&grant)) {
        assert!(!public.to_string().contains(private));
        assert!(!diagnostics.to_string().contains(private));
        assert!(!ledger.contains(private));
    }
    fixture.close().await;
}

#[tokio::test]
async fn paired_chatgpt_tools_still_route_normally_when_dots_uses_the_same_plugin() {
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    let store = GroupStore::new(fixture.state.home.clone()).expect("store");
    store
        .mutate(&fixture.group_id, |group| {
            group.actors[0].enabled = true;
            Ok(())
        })
        .expect("enable browser binding");
    let mut event = Event::new("chat.message", &fixture.group_id);
    event.by = "user".into();
    event.data.insert("to".into(), json!(["browser"]));
    event
        .data
        .insert("text".into(), json!("ordinary-browser-route-sentinel"));
    ledger::append(
        &store.ledger_path(&fixture.group_id).expect("ledger"),
        &event,
    )
    .expect("seed");
    let metadata =
        json!({"openai/session":"synthetic-browser-session","openai/subject":"synthetic-user"});
    let (status, response) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_message_history","arguments":{"limit":10},"_meta":metadata}),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(response["result"]["isError"], true);
    assert!(
        response["result"]["structuredContent"]
            .to_string()
            .contains("ordinary-browser-route-sentinel")
    );
    assert_eq!(response["result"]["resultType"], "complete");
    let (_, legacy) = fixture
        .mcp(
            "tools/call",
            json!({"name":"cccc_message_history","arguments":{"limit":10},"_meta":metadata}),
            false,
        )
        .await;
    assert_ne!(legacy["result"]["isError"], true);
    assert!(legacy["result"]["resultType"].is_null());
    assert!(fixture.connector()["dots"]["execution"].is_null());
    fixture.close().await;
}

#[tokio::test]
async fn status_and_connector_reads_keep_private_state_and_do_not_create_work() {
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    connectors::dots_update(&fixture.state.home, &fixture.connector_id, |connector| {
        connector["dots"]["subscription"] = json!({"subscription_id":"fixture-sub","enabled":true,
            "callback_url":"https://private-callback.invalid/?secret=synthetic-private-url",
            "signing_secret":"synthetic-signing-secret","expires_at_ms":null});
        Ok(())
    })
    .expect("private fixture");
    let before = fixture.connector()["dots"].clone();
    let group_path = GroupStore::new(fixture.state.home.clone())
        .expect("store")
        .group_dir(&fixture.group_id)
        .expect("dir")
        .join("group.yaml");
    let group_before = std::fs::read(&group_path).expect("group bytes");
    let ledger_path = GroupStore::new(fixture.state.home.clone())
        .expect("store")
        .ledger_path(&fixture.group_id)
        .expect("ledger");
    let ledger_before = std::fs::read(&ledger_path).unwrap_or_default();
    for _ in 0..3 {
        for uri in [
            format!(
                "/api/v1/web-model/dots?group_id={}&actor_id=dots",
                fixture.group_id
            ),
            "/api/v1/web-model/connectors".into(),
        ] {
            let (status, response) = fixture.admin_get(&uri).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(response["ok"], true);
            let text = response.to_string();
            for private in [
                "private-callback.invalid",
                "synthetic-private-url",
                "synthetic-signing-secret",
                fixture.secret.as_str(),
            ] {
                assert!(
                    !text.contains(private),
                    "public reads must not contain private state"
                );
            }
            assert!(!text.contains("subscription_token"));
            assert!(!text.contains("instructions"));
        }
    }
    assert_eq!(fixture.connector()["dots"], before);
    assert_eq!(
        std::fs::read(group_path).expect("group bytes"),
        group_before
    );
    assert_eq!(
        std::fs::read(ledger_path).unwrap_or_default(),
        ledger_before
    );
    assert!(
        fixture
            .state
            .browser_surfaces
            .sessions
            .lock()
            .await
            .is_empty()
    );
    fixture.close().await;
}

#[tokio::test]
async fn event_setup_does_not_start_an_actor_or_create_a_wake() {
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    let (status, response) = fixture
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/web-model/dots")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                .body(Body::from(
                    json!({"group_id":fixture.group_id,"actor_id":"dots","action":"connect"})
                        .to_string(),
                ))
                .expect("connect request"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["ok"], true);
    assert!(response["result"]["subscription_token"].is_null());
    assert!(
        response["result"]["instructions"]
            .as_str()
            .is_some_and(|text| {
                text.contains("native event-triggered task in Dots")
                    && text.contains("this Dot's ChatGPT account")
                    && text.contains("Refresh its tools and events")
                    && text.contains("permissions")
                    && text.contains("cccc_runtime_wait_next_turn")
                    && text.contains("cccc_runtime_complete_turn")
            })
    );
    let group = GroupStore::new(fixture.state.home.clone())
        .expect("store")
        .load(&fixture.group_id)
        .expect("group");
    assert!(!group.actors[1].enabled);
    assert!(!group.running);
    assert!(fixture.connector()["dots"]["wake"].is_null());
    assert!(
        fixture
            .state
            .browser_surfaces
            .sessions
            .lock()
            .await
            .is_empty()
    );
    GroupStore::new(fixture.state.home.clone())
        .expect("store")
        .mutate(&fixture.group_id, |group| {
            group.running = true;
            group.actors[1].enabled = true;
            Ok(())
        })
        .expect("enable Dots without starting a provider");
    let (status, repeated) = fixture
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/web-model/dots")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                .body(Body::from(
                    json!({"group_id":fixture.group_id,"actor_id":"dots","action":"connect"})
                        .to_string(),
                ))
                .expect("repeat setup request"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(repeated["result"]["subscription_token"].is_null());
    // An explicit unpaired setup request replaces only the expiring code;
    // its subscription credential and saved handler stay the same.
    let handler = |value: &Value| {
        value["result"]["instructions"]
            .as_str()
            .expect("instructions")
            .split_once("BEGIN CCCC DOTS HANDLER\n")
            .expect("handler start")
            .1
            .split_once("\nEND CCCC DOTS HANDLER")
            .expect("handler end")
            .0
            .to_owned()
    };
    assert!(handler(&repeated) == handler(&response));
    let connector = fixture.connector();
    let route = connectors::binding_for_actor(
        &connector,
        &fixture.group_id,
        "dots",
        connector["dots"]["generation"]
            .as_str()
            .expect("generation"),
    )
    .expect("binding");
    let subscription =
        connectors::dots_subscription_token(&connector, &route).expect("subscription grant");
    for setup in [&response, &repeated] {
        assert!(
            setup["result"]["instructions"]
                .as_str()
                .expect("instructions")
                .contains(&subscription)
        );
    }
    assert!(fixture.connector()["dots"]["wake"].is_null());
    assert!(
        fixture
            .state
            .browser_surfaces
            .sessions
            .lock()
            .await
            .is_empty()
    );
    fixture.close().await;
}

#[tokio::test]
async fn dots_is_excluded_from_browser_auto_delivery_even_with_an_active_binding() {
    let mut fixture = Fixture::new();
    GroupStore::new(fixture.state.home.clone())
        .expect("store")
        .mutate(&fixture.group_id, |group| {
            group.running = true;
            group.actors[1].enabled = true;
            Ok(())
        })
        .expect("active Dots");
    assert!(!super::super::web_model_supervisor::actor_delivery_enabled(
        &fixture.state,
        &fixture.group_id,
        "dots"
    ));
    let pairings_before = fixture.connector()["pairings"].clone();
    let (status, response) = fixture
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/web-model/pairing")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                .body(Body::from(
                    json!({"group_id":fixture.group_id,"actor_id":"dots","action":"connect"})
                        .to_string(),
                ))
                .expect("pairing request"),
        )
        .await;
    assert!(status.is_client_error());
    assert_eq!(response["ok"], false);
    assert_eq!(fixture.connector()["pairings"], pairings_before);
    assert!(
        fixture
            .state
            .browser_surfaces
            .sessions
            .lock()
            .await
            .is_empty()
    );
    fixture.close().await;
}

#[tokio::test]
async fn grok_modern_discovery_does_not_advertise_chatgpt_events() {
    let mut fixture = Fixture::new();
    let configured =
        connectors::configure_provider(&fixture.state.home, "grok_web").expect("Grok connector");
    let id = configured["connector"]["connector_id"]
        .as_str()
        .expect("Grok id");
    let secret = configured["secret"].as_str().expect("Grok secret");
    let mut request = fixture.mcp_request("server/discover", json!({}), true);
    *request.uri_mut() = format!("/mcp/web-model/{id}").parse().expect("URI");
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {secret}").parse().expect("header"),
    );
    let (status, response) = fixture.send(request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(response["result"]["capabilities"]["tools"].is_object());
    assert!(response["result"]["capabilities"]["events"].is_null());
    fixture.close().await;
}

#[tokio::test]
async fn modern_unknown_methods_and_unsupported_versions_have_protocol_statuses() {
    let mut fixture = Fixture::new();
    let (status, response) = fixture.mcp("unknown/method", json!({}), true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(response["error"]["code"], -32601);
    let mut request = fixture.mcp_request("tools/list", json!({}), true);
    let body = json!({"jsonrpc":"2.0","id":"fixture-request","method":"tools/list","params":{"_meta":{
        "io.modelcontextprotocol/protocolVersion":"2099-01-01","io.modelcontextprotocol/clientCapabilities":{}}}});
    *request.body_mut() = Body::from(body.to_string());
    request.headers_mut().insert(
        "mcp-protocol-version",
        "2099-01-01".parse().expect("version"),
    );
    let (status, response) = fixture.send(request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["error"]["code"], -32022);
    fixture.close().await;
}

#[tokio::test]
async fn subscription_refresh_and_unsubscribe_use_modern_wire_contract_and_fixed_verification_cache()
 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    let current = fixture.connector();
    let binding = connectors::dots_subscription_binding(
        &current,
        &connectors::dots_subscription_token(
            &current,
            current["bindings"]
                .as_object()
                .expect("routes")
                .values()
                .find(|b| b["transport"] == "mcp_events")
                .expect("route"),
        )
        .expect("token"),
    )
    .expect("route");
    let token = connectors::dots_subscription_token(&current, &binding).expect("token");
    let callback = "https://example.com/owned-verified-callback";
    let secret = format!("whsec_{}", STANDARD.encode([3_u8; 32]));
    let id = format!(
        "dots_sub_{:x}",
        Sha256::digest(
            json!([
                fixture.connector_id,
                callback,
                binding["revision"],
                EVENT_NAME
            ])
            .to_string()
            .as_bytes()
        )
    );
    let verified = chrono::Utc::now().timestamp_millis() - 1000;
    connectors::dots_update(&fixture.state.home, &fixture.connector_id, |c| {
        c["dots"]["subscription"] = json!({"subscription_id":id,"callback_url":callback,"signing_secret":secret,
            "enabled":true,"expires_at_ms":null,"verified_at_ms":verified});
        Ok(())
    }).expect("verified synthetic subscription");
    let base = json!({"name":EVENT_NAME,"arguments":{"actor_token":token},"delivery":{"mode":"webhook","url":callback,"secret":secret},"cursor":null});
    for ttl in [None, Some(json!(30_000)), Some(Value::Null)] {
        let mut params = base.clone();
        if let Some(ttl) = ttl.clone() {
            params["ttlMs"] = ttl;
        }
        let (status, response) = fixture.mcp("events/subscribe", params, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["result"]["id"], id);
        assert_eq!(response["result"]["resultType"], "complete");
        assert_eq!(response["result"]["cursor"], Value::Null);
        assert_eq!(response["result"]["truncated"], false);
        assert!(response["result"].get("subscription_id").is_none());
        let saved = fixture.connector();
        assert_eq!(
            saved["dots"]["subscription"]["verified_at_ms"], verified,
            "refresh does not slide the verification cache expiry"
        );
        if ttl != Some(json!(30_000)) {
            assert_eq!(response["result"]["refreshBefore"], Value::Null);
            assert_eq!(saved["dots"]["subscription"]["expires_at_ms"], Value::Null);
        } else {
            let expires = chrono::DateTime::parse_from_rfc3339(
                response["result"]["refreshBefore"]
                    .as_str()
                    .expect("finite expiry"),
            )
            .expect("ISO expiry")
            .timestamp_millis();
            assert_eq!(saved["dots"]["subscription"]["expires_at_ms"], expires);
            let max = 30_000;
            let remaining = expires - chrono::Utc::now().timestamp_millis();
            assert!((max - 5000..=max).contains(&remaining));
        }
    }
    let mut invalid = base.clone();
    invalid["delivery"]["url"] = json!("https://127.0.0.1/");
    let (status, response) = fixture.mcp("events/subscribe", invalid.clone(), true).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["error"]["code"], -32015);
    assert_eq!(response["error"]["data"]["reason"], "non_public_callback");
    let (status, response) = fixture.mcp("events/unsubscribe", base, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["result"]["resultType"], "complete");
    assert!(response["result"].get("unsubscribed").is_none());
    assert_eq!(
        fixture.connector()["dots"]["subscription"]["enabled"],
        false
    );
    // A later call does not hide why an earlier subscription was refused, and
    // the log names no credential or callback address.
    let log = fixture.connector()["events_log"].clone();
    let calls = log.as_array().expect("event call log");
    assert_eq!(calls.len(), 5);
    assert_eq!(
        calls[3],
        json!({"at":calls[3]["at"],"method":"events/subscribe","ok":false,"reason":"non_public_callback"})
    );
    assert_eq!(
        calls[4],
        json!({"at":calls[4]["at"],"method":"events/unsubscribe","ok":true,"reason":null})
    );
    assert!(
        calls
            .iter()
            .all(|call| call["ok"] == (call["reason"] == Value::Null))
    );
    let text = log.to_string();
    assert!(!text.contains(&token) && !text.contains(&secret) && !text.contains(callback));
    // The settings page reads the same log and the bound Actor from the list,
    // and the list carries no subscription credential or callback.
    let (status, listed) = fixture
        .send(
            Request::builder()
                .uri("/api/v1/web-model/connectors")
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                .body(Body::empty())
                .expect("list request"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let listed = &listed["result"]["connectors"][0];
    assert_eq!(listed["events_log"], log);
    assert_eq!(
        listed["dots_actor"],
        json!({"group_id":fixture.group_id,"actor_id":"dots"})
    );
    let listed = listed.to_string();
    assert!(!listed.contains(&token) && !listed.contains(&secret) && !listed.contains(callback));
    // Exercise the bound through HTTP failures rejected before any network I/O.
    // Thirteen calls retain the last ten: the three oldest successful refreshes
    // leave the log, while the earlier failure and unsubscribe remain visible.
    for _ in 0..8 {
        let (status, response) = fixture.mcp("events/subscribe", invalid.clone(), true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response["error"]["data"]["reason"], "non_public_callback");
    }
    let bounded_log = fixture.connector()["events_log"].clone();
    let retained = bounded_log.as_array().expect("bounded event call log");
    assert_eq!(retained.len(), 10);
    assert!(retained[0] == calls[3], "keep the oldest retained failure");
    assert!(retained[1] == calls[4], "keep the retained unsubscribe");
    assert!(retained[2..].iter().all(|call| {
        call["method"] == "events/subscribe"
            && call["ok"] == false
            && call["reason"] == "non_public_callback"
    }));
    let bounded_text = bounded_log.to_string();
    assert!(
        !bounded_text.contains(&token)
            && !bounded_text.contains(&secret)
            && !bounded_text.contains(callback)
            && !bounded_text.contains("127.0.0.1"),
        "retained diagnostics must not contain credentials or callback addresses"
    );
    fixture.close().await;
}

#[tokio::test]
async fn one_click_create_refuses_to_rotate_an_existing_connector() {
    let mut fixture = Fixture::new();
    fixture.start_daemon().await;
    let before = fixture.connector()["secret_hash"].clone();
    let (status, response) = fixture
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/web-model/connectors")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                .body(Body::from(
                    json!({"provider":"chatgpt_web","create_only":true}).to_string(),
                ))
                .expect("create-only request"),
        )
        .await;
    assert_ne!(status, StatusCode::OK);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "connector_exists");
    // A mistyped flag is refused rather than read as "rotate".
    for flag in [json!("true"), json!(1), Value::Null] {
        let (status, _) = fixture
            .send(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/web-model/connectors")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin))
                    .body(Body::from(
                        json!({"provider":"chatgpt_web","create_only":flag}).to_string(),
                    ))
                    .expect("mistyped request"),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{flag}");
    }
    assert!(!response.to_string().contains("wmcs_"));
    // The credential already added to ChatGPT keeps working.
    assert_eq!(fixture.connector()["secret_hash"], before);
    fixture.close().await;
}
