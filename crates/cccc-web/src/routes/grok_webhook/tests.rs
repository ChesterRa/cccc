use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event, GroupState, RunnerKind};
use cccc_core::{
    GroupStore, HomeLayout, access_tokens::AccessTokenStore, ledger, web_model_connectors as store,
};
use tower::ServiceExt;

struct Fixture {
    _temp: tempfile::TempDir,
    state: AppState,
    router: Router,
    group: String,
    id: String,
    secret: String,
    admin: String,
    daemon: Option<tokio::task::JoinHandle<()>>,
}
impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("isolated Grok fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("isolated Grok fixture");
        home.initialize().expect("isolated Grok fixture");
        let groups = GroupStore::new(home.clone()).expect("isolated Grok fixture");
        let mut g = groups
            .create("Native Grok", "")
            .expect("isolated Grok fixture");
        let mut a = Actor::new("bot");
        a.runtime = ActorRuntime::GrokWebModel;
        a.runner = RunnerKind::Headless;
        a.enabled = false;
        cccc_core::actors::add(&mut g, a).expect("isolated Grok fixture");
        g.running = true;
        g.state = GroupState::Active;
        groups.save(&g).expect("isolated Grok fixture");
        let configured =
            store::configure_provider(&home, "grok_web").expect("isolated Grok fixture");
        let id = configured["connector"]["connector_id"]
            .as_str()
            .expect("isolated Grok fixture")
            .to_owned();
        store::bind_grok(
            &home,
            &id,
            &g.group_id,
            "bot",
            &cccc_core::actors::generation_identity(&g.actors[0]),
            "https://grok.com/bot/00000000-0000-4000-8000-000000000001",
        )
        .expect("isolated Grok fixture");
        let admin = AccessTokenStore::new(home.clone())
            .expect("isolated Grok fixture")
            .create("admin", Vec::new(), true, None)
            .expect("isolated Grok fixture")
            .token;
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (router, _, _, state) = crate::app_with_shutdown(
            home.clone(),
            shutdown,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "grok-http-fixture".into(),
        );
        let daemon = tokio::spawn(async move {
            cccc_daemon::run(home).await.expect("isolated Grok fixture");
        });
        let f = Self {
            _temp: temp,
            state,
            router,
            group: g.group_id,
            id,
            secret: configured["secret"]
                .as_str()
                .expect("isolated Grok fixture")
                .into(),
            admin,
            daemon: Some(daemon),
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if f.state
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
        .expect("isolated Grok fixture");
        f
    }
    async fn op(&self, op: &str, args: Value) -> Value {
        let mut args = args.as_object().expect("isolated Grok fixture").clone();
        args.entry("group_id").or_insert(json!(self.group));
        args.entry("actor_id").or_insert(json!("bot"));
        args.entry("by").or_insert(json!("user"));
        let r = self
            .state
            .client
            .call(&DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            })
            .await
            .expect("isolated Grok fixture");
        assert!(r.ok, "{op}: {:?}", r.error);
        Value::Object(r.result)
    }
    async fn send(&self, r: Request<Body>) -> (StatusCode, Value) {
        let r = self
            .router
            .clone()
            .oneshot(r)
            .await
            .expect("isolated Grok fixture");
        let status = r.status();
        let bytes = to_bytes(r.into_body(), 2 * 1024 * 1024)
            .await
            .expect("isolated Grok fixture");
        (
            status,
            serde_json::from_slice(&bytes).expect("isolated Grok fixture"),
        )
    }
    async fn api(&self, action: Value) -> (StatusCode, Value) {
        let mut body = action;
        body["group_id"] = json!(self.group);
        body["actor_id"] = json!("bot");
        self.send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/web-model/grok-webhook")
                .header(header::AUTHORIZATION, format!("Bearer {}", self.admin))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("isolated Grok fixture"),
        )
        .await
    }
    async fn tool(&self, name: &str, args: Value) -> Value {
        self.send(Request::builder().method("POST").uri(format!("/mcp/web-model/{}",self.id)).header(header::AUTHORIZATION,format!("Bearer {}",self.secret)).header(header::CONTENT_TYPE,"application/json").body(Body::from(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string())).expect("isolated Grok fixture")).await.1
    }
    fn source(&self, text: &str) -> String {
        let mut e = Event::new("chat.message", &self.group);
        e.by = "user".into();
        e.data = json!({"to":["bot"],"text":text,"message_mode":"send"})
            .as_object()
            .expect("isolated Grok fixture")
            .clone();
        ledger::append(
            &GroupStore::new(self.state.home.clone())
                .expect("isolated Grok fixture")
                .ledger_path(&self.group)
                .expect("isolated Grok fixture"),
            &e,
        )
        .expect("isolated Grok fixture");
        e.id
    }
    async fn close(mut self) {
        self.state
            .client
            .call(&DaemonRequest {
                v: 1,
                op: "shutdown".into(),
                args: Default::default(),
            })
            .await
            .expect("isolated Grok fixture");
        tokio::time::timeout(
            Duration::from_secs(5),
            self.daemon.take().expect("isolated Grok fixture"),
        )
        .await
        .expect("isolated Grok fixture")
        .expect("isolated Grok fixture");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.state.shutdown.send(());
        if let Some(d) = self.daemon.take() {
            d.abort();
        }
    }
}

#[tokio::test]
async fn native_http_claim_reply_complete_and_next_wake_use_scoped_authority() {
    let f = Fixture::new().await;
    let (_, setup) = f.api(json!({"action":"instructions"})).await;
    let handler = setup["result"]["instructions"]
        .as_str()
        .expect("isolated Grok fixture");
    assert!(handler.contains("BEGIN CCCC GROK HANDLER"));
    assert!(handler.contains("Host JavaScript orchestration"));
    assert!(handler.contains("this same run's original claim tool result"));
    assert!(handler.contains("cccc_code_exec source/code"));
    assert!(!handler.contains("synthetic-provider"));
    let (http,configured)=f.api(json!({"action":"configure","url":"https://api2.cursor.sh/automations/webhook/fixture","key":"synthetic-provider"})).await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(configured["result"]["configured"], true);
    assert!(!configured.to_string().contains("synthetic-provider"));
    f.op("actor_start", json!({})).await;
    let source = f.source("native synthetic task");
    let d = f.op("grok_webhook_next_delivery", json!({})).await["delivery"].clone();
    let claim = d["body"]["actor_token"].clone();
    assert_eq!(
        f.tool(
            "cccc_message_send",
            json!({"actor_token":claim,"to":["user"],"text":"forbidden"})
        )
        .await["result"]["isError"],
        true
    );
    assert_eq!(
        f.tool(
            "cccc_code_exec",
            json!({"actor_token":claim,"code":"return 1;"})
        )
        .await["result"]["isError"],
        true
    );
    let first = f
        .tool("cccc_runtime_wait_next_turn", json!({"actor_token":claim}))
        .await;
    assert_eq!(first["result"]["isError"], false);
    let c = &first["result"]["structuredContent"];
    assert_eq!(c["status"], "work_available");
    assert_eq!(c["turn"]["event_ids"], json!([source]));
    let duplicate = f
        .tool("cccc_runtime_wait_next_turn", json!({"actor_token":claim}))
        .await;
    assert_eq!(
        duplicate["result"]["structuredContent"]["status"],
        "turn_in_progress"
    );
    let token = &c["actor_token"];
    assert_ne!(*token, claim);
    let reply = f
        .tool(
            "cccc_message_reply",
            json!({"actor_token":token,"event_id":source,"text":"Synthetic result","mode":"send","insight":"Synthetic native transport verification"}),
        )
        .await;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    assert!(
        reply["result"]["structuredContent"]["event"]["data"]
            .get("_cccc_web_binding")
            .is_none()
    );
    let later = f.source("next synthetic task");
    assert!(f.op("grok_webhook_next_delivery", json!({})).await["delivery"].is_null());
    let done = json!({"actor_token":token,"turn_id":c["turn"]["turn_id"],"event_ids":c["turn"]["event_ids"]});
    for _ in 0..2 {
        assert_ne!(
            f.tool("cccc_runtime_complete_turn", done.clone()).await["result"]["isError"],
            true
        );
    }
    assert_eq!(
        f.tool("cccc_bootstrap", json!({"actor_token":token})).await["result"]["isError"],
        true
    );
    let next = f.op("grok_webhook_next_delivery", json!({})).await["delivery"].clone();
    let c2 = f
        .tool(
            "cccc_runtime_wait_next_turn",
            json!({"actor_token":next["body"]["actor_token"]}),
        )
        .await;
    assert_eq!(
        c2["result"]["structuredContent"]["turn"]["event_ids"],
        json!([later])
    );
    let newer = &c2["result"]["structuredContent"];
    let ledger_path = GroupStore::new(f.state.home.clone())
        .expect("store")
        .ledger_path(&f.group)
        .expect("ledger");
    let forged = f.tool("cccc_runtime_complete_turn", json!({"actor_token":token,"turn_id":newer["turn"]["turn_id"],"event_ids":newer["turn"]["event_ids"]})).await;
    assert_eq!(forged["result"]["isError"], true);
    assert!(
        !ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .any(|event| {
                event.kind == "runtime.turn.completed"
                    && event.data["turn_id"] == newer["turn"]["turn_id"]
            })
    );
    assert_ne!(
        f.tool("cccc_runtime_complete_turn", done).await["result"]["isError"],
        true
    );
    assert_eq!(
        f.op("grok_webhook_status", json!({})).await["active_turn_id"],
        newer["turn"]["turn_id"]
    );
    let (http, info) = f
        .send(
            Request::builder()
                .uri(format!(
                    "/api/v1/web-model/grok-webhook?group_id={}&actor_id=bot",
                    f.group
                ))
                .header(header::AUTHORIZATION, format!("Bearer {}", f.admin))
                .body(Body::empty())
                .expect("isolated Grok fixture"),
        )
        .await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(info["result"]["phase"], "working");
    assert!(info["result"].get("browser_surface").is_none());
    let (http, _) = f
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/web-model/browser-session/open")
                .header(header::AUTHORIZATION, format!("Bearer {}", f.admin))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"group_id":f.group,"actor_id":"bot"}).to_string(),
                ))
                .expect("isolated Grok fixture"),
        )
        .await;
    assert_ne!(http, StatusCode::OK);
    assert_ne!(f.tool("cccc_runtime_complete_turn", json!({"actor_token":newer["actor_token"],"turn_id":newer["turn"]["turn_id"],"event_ids":newer["turn"]["event_ids"]})).await["result"]["isError"], true);
    f.close().await;
}

#[tokio::test]
async fn idle_apply_and_claim_are_serialized_without_interrupting_work() {
    let f = Fixture::new().await;
    let config = json!({"action":"configure","url":"https://api2.cursor.sh/automations/webhook/fixture","key":"synthetic-key"});
    assert_eq!(f.api(config.clone()).await.0, StatusCode::OK);
    f.op("actor_start", json!({})).await;
    let source = f.source("claim racing with connection apply");
    let old = f.op("grok_webhook_next_delivery", json!({})).await["delivery"].clone();
    let (configured, claimed) = tokio::join!(
        f.api(config.clone()),
        f.tool(
            "cccc_runtime_wait_next_turn",
            json!({"actor_token":old["body"]["actor_token"]})
        )
    );
    let task = if configured.0 == StatusCode::OK {
        assert_eq!(configured.1["result"]["enabled"], true);
        assert_eq!(claimed["result"]["isError"], true);
        let next = f.op("grok_webhook_next_delivery", json!({})).await["delivery"].clone();
        f.tool(
            "cccc_runtime_wait_next_turn",
            json!({"actor_token":next["body"]["actor_token"]}),
        )
        .await
    } else {
        assert_eq!(configured.1["error"]["code"], "delivery_unresolved");
        claimed
    };
    assert_ne!(task["result"]["isError"], true);
    let c = &task["result"]["structuredContent"];
    assert_eq!(c["turn"]["event_ids"], json!([source]));
    for action in [
        config,
        json!({"action":"bind","url":"https://grok.com/bot/00000000-0000-4000-8000-000000000002"}),
    ] {
        let (http, rejected) = f.api(action).await;
        assert_eq!(http, StatusCode::BAD_REQUEST);
        assert_eq!(rejected["error"]["code"], "delivery_unresolved");
        assert_eq!(
            f.op("grok_webhook_status", json!({})).await["enabled"],
            true
        );
    }
    let completed = f.tool("cccc_runtime_complete_turn", json!({"actor_token":c["actor_token"],"turn_id":c["turn"]["turn_id"],"event_ids":c["turn"]["event_ids"]})).await;
    assert_ne!(completed["result"]["isError"], true);
    let (http, applied) = f.api(json!({"action":"bind","url":"https://grok.com/bot/00000000-0000-4000-8000-000000000002"})).await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(applied["result"]["enabled"], true);
    assert_eq!(applied["result"]["configured"], false);
    f.close().await;
}

#[tokio::test]
async fn native_connection_ui_can_bind_configure_and_disconnect_without_browser_state() {
    let f = Fixture::new().await;
    let next = "https://grok.com/bot/00000000-0000-4000-8000-000000000002";
    let (http, bound) = f.api(json!({"action":"bind","url":next})).await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(bound["result"]["bot_url"], next);
    assert_eq!(bound["result"]["phase"], "unconfigured");
    let (http, _) = f
        .api(json!({"action":"configure","transport":"browser"}))
        .await;
    assert_eq!(http, StatusCode::BAD_REQUEST);
    let (http, configured) = f.api(json!({"action":"configure","url":"https://api2.cursor.sh/automations/webhook/fixture","key":"synthetic-key"})).await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(configured["result"]["configured"], true);
    assert_eq!(configured["result"]["phase"], "stopped");
    let (http, disconnected) = f.api(json!({"action":"disconnect"})).await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(disconnected["result"]["bound"], false);
    assert_eq!(disconnected["result"]["configured"], false);
    assert_eq!(disconnected["result"]["phase"], "unconfigured");
    let (http, _) = f
        .send(
            Request::builder()
                .uri("/api/v1/web-model/shared-browser?provider=grok_web")
                .header(header::AUTHORIZATION, format!("Bearer {}", f.admin))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
    assert_eq!(http, StatusCode::BAD_REQUEST);
    f.close().await;
}

#[tokio::test]
async fn bearer_delivery_posts_exact_json_and_does_not_follow_redirects() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let app = Router::new()
        .route(
            "/wake",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, Json(body): Json<Value>| async move {
                    assert_eq!(headers[header::AUTHORIZATION], "Bearer synthetic-key");
                    assert_eq!(
                        body,
                        json!({"event":"cccc.grok.ready","actor_token":"synthetic-claim"})
                    );
                    (
                        StatusCode::TEMPORARY_REDIRECT,
                        [(header::LOCATION, "/forbidden")],
                    )
                },
            ),
        )
        .route(
            "/forbidden",
            get(move || {
                count.fetch_add(1, Ordering::SeqCst);
                async { StatusCode::OK }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("isolated Grok fixture");
    let address = listener.local_addr().expect("isolated Grok fixture");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("isolated Grok fixture");
    });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("isolated Grok fixture");
    assert_eq!(
        post(
            &client,
            &format!("http://{address}/wake"),
            "synthetic-key",
            &json!({"event":"cccc.grok.ready","actor_token":"synthetic-claim"})
        )
        .await,
        Some(307)
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    server.abort();
}
