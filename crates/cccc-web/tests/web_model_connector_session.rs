use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupStore, HomeLayout, web_model_connectors as store};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: String,
    connector: Value,
    secret: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("valid test fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("valid test fixture");
        home.initialize().expect("valid test fixture");
        let groups = GroupStore::new(home.clone()).expect("valid test fixture");
        let group = groups.create("fixture", "").expect("valid test fixture");
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).expect("valid test fixture");
        std::fs::write(root.join("sample.txt"), "local fixture content")
            .expect("valid test fixture");
        let mut group = cccc_core::group_scope::attach(
            &groups,
            &group.group_id,
            cccc_core::Scope {
                scope_key: "s_fixture".into(),
                url: root.to_string_lossy().into(),
                label: "fixture".into(),
                git_remote: String::new(),
            },
        )
        .expect("valid test fixture");
        for id in ["alpha", "beta"] {
            let mut a = Actor::new(id);
            a.runtime = ActorRuntime::WebModel;
            a.enabled = true;
            cccc_core::actors::add(&mut group, a).expect("valid test fixture");
        }
        groups.save(&group).expect("valid test fixture");
        assert!(!group.running);
        let configured = store::configure(&home).expect("valid test fixture");
        Self {
            _temp: temp,
            home,
            group: group.group_id,
            connector: configured["connector"].clone(),
            secret: configured["secret"]
                .as_str()
                .expect("valid test fixture")
                .into(),
        }
    }
    fn bind(&self, actor: &str, session: &str) {
        let actor_doc = GroupStore::new(self.home.clone())
            .expect("valid test fixture")
            .load(&self.group)
            .expect("valid test fixture")
            .actors
            .into_iter()
            .find(|a| a.id == actor)
            .expect("valid test fixture");
        let id = self.connector["connector_id"]
            .as_str()
            .expect("valid test fixture");
        let generation = cccc_core::actors::generation_identity(&actor_doc);
        let pair = store::begin_pairing(&self.home, id, &self.group, actor, &generation, false)
            .expect("valid test fixture");
        let key = store::session_key(&self.connector, &meta(session)).expect("valid test fixture");
        store::accept_pairing(
            &self.home,
            id,
            pair["code"].as_str().expect("valid test fixture"),
            &key,
        )
        .expect("valid test fixture");
        store::confirm_pairing(
            &self.home,
            id,
            &self.group,
            actor,
            &generation,
            pair["pairing_id"].as_str().expect("valid test fixture"),
            &format!("https://chatgpt.com/c/{actor}"),
        )
        .expect("valid test fixture");
    }
    async fn request(
        &self,
        body: Value,
        secret: Option<&str>,
        path_token: bool,
    ) -> (StatusCode, Value) {
        let id = self.connector["connector_id"]
            .as_str()
            .expect("valid test fixture");
        let path = if path_token {
            format!("/mcp/web-model/{id}/token/{}", secret.unwrap_or("wrong"))
        } else {
            format!("/mcp/web-model/{id}")
        };
        let mut request = Request::post(path).header(header::CONTENT_TYPE, "application/json");
        if !path_token && let Some(secret) = secret {
            request = request.header(header::AUTHORIZATION, format!("Bearer {secret}"));
        }
        // Recreating the Web app proves identity does not depend on an HTTP session.
        let response = cccc_web::app(self.home.clone())
            .oneshot(
                request
                    .body(Body::from(body.to_string()))
                    .expect("valid test fixture"),
            )
            .await
            .expect("valid test fixture");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("valid test fixture")
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).expect("valid test fixture"),
        )
    }
    async fn call(&self, name: &str, args: Value, metadata: Value) -> Value {
        let (status,value)=self.request(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args,"_meta":metadata}}),Some(&self.secret),true).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }
}
struct DaemonGuard(tokio::task::JoinHandle<anyhow::Result<()>>);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn start_daemon(home: &HomeLayout) -> DaemonGuard {
    let daemon_home = home.clone();
    let task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if client
                .call(&cccc_contracts::DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture daemon ready");
    DaemonGuard(task)
}

fn meta(session: &str) -> Value {
    json!({"openai/session":session,"openai/subject":"fixture-user"})
}

#[tokio::test]
async fn one_catalog_before_pairing_and_strict_authentication() {
    let f = Fixture::new();
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    for secret in [None, Some("incorrect")] {
        assert_eq!(
            f.request(body.clone(), secret, false).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let first = f.request(body.clone(), Some(&f.secret), false).await.1;
    let tools = first["result"]["tools"]
        .as_array()
        .expect("valid test fixture");
    for name in [
        "cccc_pair",
        "cccc_connector_status",
        "cccc_file",
        "cccc_file_send",
        "cccc_code_exec",
    ] {
        assert!(tools.iter().any(|t| t["name"] == name));
    }
    for (name, read_only) in [
        ("cccc_file", true),
        ("cccc_file_send", false),
        ("cccc_capability_search", true),
    ] {
        let tool = tools.iter().find(|t| t["name"] == name).expect("tool");
        assert_eq!(tool["annotations"]["readOnlyHint"], read_only);
        assert!(
            tool["inputSchema"]["properties"]["actor_id"]["description"]
                .as_str()
                .expect("description")
                .contains("paired CCCC Actor")
        );
    }
    for (name, read_only) in [("cccc_pair", false), ("cccc_connector_status", true)] {
        let tool = tools.iter().find(|t| t["name"] == name).expect("tool");
        assert_eq!(
            tool["annotations"],
            json!({"readOnlyHint":read_only,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}),
            "pairing must disclose its state change; status remains read-only"
        );
    }
    f.bind("alpha", "chat-a");
    f.bind("beta", "chat-b");
    assert_eq!(first, f.request(body, Some(&f.secret), true).await.1);
    let generic = cccc_mcp::handle_request(
        &f.home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
    assert!(
        !generic["result"]["tools"]
            .as_array()
            .expect("valid test fixture")
            .iter()
            .any(|t| t["name"] == "cccc_pair" || t["name"] == "cccc_connector_status")
    );
}

#[tokio::test]
async fn file_read_and_send_keep_distinct_effects_and_bound_identity() {
    let f = Fixture::new();
    let _daemon = start_daemon(&f.home).await;
    f.bind("alpha", "chat-a");
    let groups = GroupStore::new(f.home.clone()).expect("groups");
    let ledger = groups.ledger_path(&f.group).expect("ledger");
    let before = std::fs::read(&ledger).expect("ledger bytes");
    let read = f
        .call(
            "cccc_file",
            json!({"rel_path":"sample.txt"}),
            meta("chat-a"),
        )
        .await;
    assert_eq!(
        read["result"]["structuredContent"]["content"],
        "local fixture content"
    );
    for args in [
        json!({"action":"send","path":"sample.txt","to":"user","mode":"send"}),
        json!({"path":"sample.txt","to":"user","mode":"send"}),
        json!({"action":null,"path":"sample.txt","to":"user","mode":"send"}),
        json!({"action":"read","rel_path":"../private.txt"}),
    ] {
        let denied = f.call("cccc_file", args, meta("chat-a")).await;
        assert_eq!(denied["result"]["isError"], true, "{denied}");
    }
    let rejected = f
        .call(
            "cccc_file_send",
            json!({"path":"sample.txt","to":"beta","mode":"mail"}),
            meta("chat-a"),
        )
        .await;
    assert_eq!(
        rejected["result"]["structuredContent"]["error"]["code"],
        "peer_insight_required"
    );
    assert_eq!(
        rejected["result"]["structuredContent"]["error"]["details"]["new_side_effects"],
        false
    );
    let rejected = f
        .call(
            "cccc_file_send",
            json!({"action":"read","path":"sample.txt","to":"user","mode":"send"}),
            meta("chat-a"),
        )
        .await;
    assert_eq!(rejected["result"]["isError"], true);
    let rejected = f
        .call(
            "cccc_file_send",
            json!({"path":"sample.txt","to":"user","mode":"send","group_id":"forged"}),
            meta("chat-a"),
        )
        .await;
    assert_eq!(rejected["result"]["isError"], true);
    assert!(
        rejected
            .to_string()
            .contains("connector cannot access another group")
    );
    assert_eq!(std::fs::read(&ledger).expect("ledger bytes"), before);
    let blobs = groups.state_dir(&f.group).expect("state").join("blobs");
    assert!(!blobs.exists() || std::fs::read_dir(&blobs).expect("blobs").next().is_none());

    let nested = f.call("cccc_code_exec", json!({
        "source":"await tools.cccc_file({action:'send', path:'sample.txt', to:'user', mode:'send'});",
        "yield_time_ms":10000
    }), meta("chat-a")).await;
    assert_eq!(
        nested["result"]["structuredContent"]["status"], "failed",
        "{nested}"
    );
    assert!(
        nested["result"]["structuredContent"]["error_text"]
            .as_str()
            .expect("nested error")
            .contains("use cccc_file_send")
    );
    assert_eq!(std::fs::read(&ledger).expect("ledger bytes"), before);

    let sent = f
        .call(
            "cccc_file_send",
            json!({"path":"sample.txt","to":"user","mode":"send","actor_id":"beta","by":"user"}),
            meta("chat-a"),
        )
        .await;
    assert_ne!(sent["result"]["isError"], true, "{sent}");
    let event = &sent["result"]["structuredContent"]["result"]["event"];
    assert_eq!(event["by"], "alpha");
    assert_eq!(event["group_id"], f.group);
    assert_eq!(event["data"]["to"], json!(["user"]));
    assert_eq!(
        event["data"]["attachments"]
            .as_array()
            .expect("attachments")
            .len(),
        1
    );
    assert!(
        sent["result"]["structuredContent"]
            .get("post_message_nudge")
            .is_none()
    );

    let nested = f.call("cccc_code_exec", json!({
        "source":"const sent = await tools.cccc_file_send({path:'sample.txt', to:'user', mode:'send', by:'user', actor_id:'beta'}); text(sent.result.event.by);",
        "yield_time_ms":10000
    }), meta("chat-a")).await;
    assert_eq!(
        nested["result"]["structuredContent"]["status"], "completed",
        "{nested}"
    );
    assert_eq!(nested["result"]["structuredContent"]["output"], "alpha");
    cccc_mcp::shutdown(&f.home).await;
}
#[tokio::test]
async fn routes_two_conversations_across_refresh_without_exposing_raw_metadata() {
    let f = Fixture::new();
    let _daemon = start_daemon(&f.home).await;
    f.bind("alpha", "chat-a");
    f.bind("beta", "chat-b");
    for (session, actor) in [("chat-a", "alpha"), ("chat-b", "beta"), ("chat-a", "alpha")] {
        let response = f
            .call("cccc_connector_status", json!({}), meta(session))
            .await;
        let payload = &response["result"]["structuredContent"];
        assert_eq!(payload["state"], "ready");
        assert_eq!(payload["actor_id"], actor);
        assert_eq!(payload["group_id"], f.group);
        assert!(!response.to_string().contains(session));
        assert!(!response.to_string().contains("fixture-user"));
        let read = f
            .call(
                "cccc_file",
                json!({"action":"read","rel_path":"sample.txt","by":"user","actor_id":"other"}),
                meta(session),
            )
            .await;
        assert_ne!(read["result"]["isError"], true, "{read}");
        assert!(read.to_string().contains("local fixture content"));
    }
    let connector = store::load(&f.home).expect("valid test fixture").remove(0);
    for actor in ["alpha", "beta"] {
        assert_eq!(
            connector["bindings"][json!([f.group, actor]).to_string()]["last_tool_name"],
            "cccc_file"
        );
    }
    let raw = std::fs::read_to_string(f.home.root().join("web_model_connectors.yaml"))
        .expect("valid test fixture");
    assert!(!raw.contains("chat-a"));
    assert!(!raw.contains("fixture-user"));
    assert!(!raw.contains(&f.secret));
}

#[tokio::test]
async fn legacy_actor_binding_authorizes_its_conversation_without_changing_identity() {
    let f = Fixture::new();
    let groups = GroupStore::new(f.home.clone()).expect("groups");
    groups
        .mutate(&f.group, |group| {
            group.actors[0].generation.clear();
            Ok(())
        })
        .expect("legacy actor");
    let _daemon = start_daemon(&f.home).await;
    f.bind("alpha", "legacy-chat");
    let status = f
        .call("cccc_connector_status", json!({}), meta("legacy-chat"))
        .await;
    assert_eq!(status["result"]["structuredContent"]["state"], "ready");
    assert_eq!(status["result"]["structuredContent"]["actor_id"], "alpha");
    let read = f
        .call(
            "cccc_file",
            json!({"action":"read","rel_path":"sample.txt"}),
            meta("legacy-chat"),
        )
        .await;
    assert_ne!(read["result"]["isError"], true, "{read}");
    assert!(read.to_string().contains("local fixture content"));
    assert!(
        groups.load(&f.group).expect("group").actors[0]
            .generation
            .is_empty(),
        "MCP routing must not migrate an Actor's identity"
    );
}
#[tokio::test]
async fn missing_forged_changed_or_unpaired_metadata_never_falls_back() {
    let f = Fixture::new();
    f.bind("alpha", "chat-a");
    for metadata in [
        Value::Null,
        json!({}),
        json!({"openai/session":null}),
        meta("other-chat"),
        json!({"openai/session":"chat-a"}),
        json!({"openai/session":"chat-a","openai/subject":"other-user"}),
    ] {
        let response=f.call("cccc_file",json!({"action":"read","rel_path":"sample.txt","actor_id":"alpha","group_id":f.group}),metadata).await;
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(!response.to_string().contains("local fixture content"));
    }
    let wrong_group = f
        .call(
            "cccc_file",
            json!({"action":"read","rel_path":"sample.txt","group_id":"g_other"}),
            meta("chat-a"),
        )
        .await;
    assert_eq!(wrong_group["result"]["isError"], true);
    let forged = f
        .call(
            "cccc_connector_status",
            json!({"openai/session":"chat-a"}),
            Value::Null,
        )
        .await;
    assert_eq!(forged["error"]["code"], -32602);
}
#[tokio::test]
async fn stopped_deleted_replaced_and_revoked_actors_lose_access() {
    let f = Fixture::new();
    f.bind("alpha", "chat-a");
    f.bind("beta", "chat-b");
    let groups = GroupStore::new(f.home.clone()).expect("valid test fixture");
    for mutation in ["stop", "generation", "runtime", "remove"] {
        groups
            .mutate(&f.group, |g| {
                let a = g
                    .actors
                    .iter_mut()
                    .find(|a| a.id == "alpha")
                    .expect("valid test fixture");
                a.enabled = true;
                match mutation {
                    "stop" => a.enabled = false,
                    "generation" => a.generation = "replacement".into(),
                    "runtime" => a.runtime = ActorRuntime::Codex,
                    _ => g.actors.retain(|a| a.id != "alpha"),
                };
                Ok(())
            })
            .expect("valid test fixture");
        let response = f
            .call(
                "cccc_file",
                json!({"action":"read","rel_path":"sample.txt"}),
                meta("chat-a"),
            )
            .await;
        assert_eq!(
            response["result"]["isError"], true,
            "{mutation}: {response}"
        );
        assert_eq!(
            f.call("cccc_connector_status", json!({}), meta("chat-b"))
                .await["result"]["structuredContent"]["state"],
            "ready"
        );
    }
    store::revoke(
        &f.home,
        f.connector["connector_id"]
            .as_str()
            .expect("valid test fixture"),
    )
    .expect("valid test fixture");
    assert_eq!(
        f.request(
            json!({"id":1,"method":"tools/list"}),
            Some(&f.secret),
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn credential_rotation_keeps_routes_but_invalidates_the_old_secret() {
    let mut f = Fixture::new();
    f.bind("alpha", "chat-a");
    let before = f
        .call("cccc_connector_status", json!({}), meta("chat-a"))
        .await;
    let new = store::configure(&f.home).expect("valid test fixture");
    assert_eq!(
        f.request(json!({"id":1,"method":"ping"}), Some(&f.secret), true)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    f.secret = new["secret"].as_str().expect("valid test fixture").into();
    assert_eq!(
        before,
        f.call("cccc_connector_status", json!({}), meta("chat-a"))
            .await
    );
}

#[tokio::test]
async fn pairing_tool_receipt_requires_host_metadata_and_grants_no_business_tools() {
    let f = Fixture::new();
    let _daemon = start_daemon(&f.home).await;
    let groups = GroupStore::new(f.home.clone()).expect("valid test fixture");
    groups
        .mutate(&f.group, |g| {
            g.actors[0].enabled = false;
            Ok(())
        })
        .expect("valid test fixture");
    let client = cccc_client::DaemonClient::new(f.home.clone());
    let pair=client.call(&cccc_contracts::DaemonRequest{v:1,op:"web_model_pairing_begin".into(),args:json!({"by":"user","connector_id":f.connector["connector_id"],"group_id":f.group,"actor_id":"alpha"}).as_object().expect("valid test fixture").clone()}).await.expect("valid test fixture");
    assert!(pair.ok, "{pair:?}");
    let received = f
        .call(
            "cccc_pair",
            json!({"code":pair.result["code"]}),
            meta("chat-a"),
        )
        .await;
    assert_eq!(
        received["result"]["structuredContent"]["state"], "awaiting_confirmation",
        "{received}"
    );
    assert_eq!(
        f.call(
            "cccc_pair",
            json!({"code":pair.result["code"]}),
            meta("chat-b")
        )
        .await["result"]["isError"],
        true
    );
    assert_eq!(
        f.call(
            "cccc_file",
            json!({"action":"read","rel_path":"sample.txt"}),
            meta("chat-a")
        )
        .await["result"]["isError"],
        true
    );
    assert_eq!(
        f.call("cccc_connector_status", json!({}), meta("chat-a"))
            .await["result"]["structuredContent"]["state"],
        "unpaired"
    );
    let code = f
        .call(
            "cccc_code_exec",
            json!({"code":"text('pending pairing must not authorize code execution');"}),
            meta("chat-a"),
        )
        .await;
    assert_eq!(code["result"]["isError"], true, "{code}");
    assert!(
        code.to_string().contains("conversation_not_paired"),
        "{code}"
    );
}

#[tokio::test]
async fn shared_browser_close_blocks_paired_enabled_actors_only() {
    let f = Fixture::new();
    async fn close(f: &Fixture) -> StatusCode {
        cccc_web::app(f.home.clone())
            .oneshot(
                Request::post("/api/v1/web-model/shared-browser/close")
                    .extension(axum::extract::ConnectInfo(
                        "127.0.0.1:12345"
                            .parse::<std::net::SocketAddr>()
                            .expect("loopback"),
                    ))
                    .header(header::HOST, "localhost")
                    .header(header::ORIGIN, "http://localhost")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response")
            .status()
    }
    assert_eq!(
        close(&f).await,
        StatusCode::OK,
        "unpaired enabled Actors do not own the shared browser"
    );
    f.bind("alpha", "chat-a");
    assert_eq!(
        close(&f).await,
        StatusCode::BAD_REQUEST,
        "a paired enabled Actor must be stopped first"
    );
    let store = GroupStore::new(f.home.clone()).expect("groups");
    let mut group = store.load(&f.group).expect("group");
    group
        .actors
        .iter_mut()
        .find(|a| a.id == "alpha")
        .expect("actor")
        .enabled = false;
    store.save(&group).expect("save");
    assert_eq!(
        close(&f).await,
        StatusCode::OK,
        "an unrelated unpaired Actor must not block close"
    );
}
