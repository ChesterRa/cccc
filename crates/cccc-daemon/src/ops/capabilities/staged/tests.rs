use super::*;
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use cccc_contracts::{ActorRuntime, DaemonResponse};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, Semaphore, watch};

const CAP: &str = "mcp:test:staged";
const KEY: &str = "CCCC_STAGED_FIXTURE_TOKEN";

struct Remote {
    held: Mutex<String>,
    entered: Notify,
    release: Semaphore,
    calls: AtomicUsize,
}

impl Default for Remote {
    fn default() -> Self {
        Self {
            held: Mutex::new(String::new()),
            entered: Notify::new(),
            release: Semaphore::new(0),
            calls: AtomicUsize::new(0),
        }
    }
}

async fn endpoint(
    State(state): State<Arc<Remote>>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let method = request["method"].as_str().unwrap_or("");
    if method == "notifications/initialized" {
        return StatusCode::ACCEPTED.into_response();
    }
    if method == "tools/call" {
        state.calls.fetch_add(1, Ordering::SeqCst);
    }
    let held = state.held.lock().expect("held method").as_str() == method;
    if held {
        state.entered.notify_one();
        state
            .release
            .acquire()
            .await
            .expect("release fixture")
            .forget();
    }
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"staged-fixture","version":"1"}})
        }
        "tools/list" => {
            json!({"tools":[{"name":"echo","description":"local fixture","inputSchema":{"type":"object"}}]})
        }
        "tools/call" => {
            json!({"content":[{"type":"text","text":"fixture-result"}],"isError":request["params"]["arguments"]["fail"] == true})
        }
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: String,
    other: String,
    record: Value,
    locks: DispatchLocks,
    remote: Arc<Remote>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new(bearer: bool) -> Self {
        let temp = tempfile::tempdir().expect("temporary fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("isolated home");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        let mut group = groups.create("staged fixture", "").expect("group");
        group.state = cccc_contracts::GroupState::Paused;
        group.running = false;
        for id in ["one", "two"] {
            let mut actor = Actor::new(id);
            actor.runtime = ActorRuntime::Custom;
            actor.generation = uuid::Uuid::new_v4().to_string();
            group.actors.push(actor);
        }
        groups.save(&group).expect("actors");
        let mut other = groups.create("other group", "").expect("other group");
        other.state = cccc_contracts::GroupState::Paused;
        other.running = false;
        groups.save(&other).expect("paused other group");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("private HTTP port");
        let url = format!("http://{}/mcp", listener.local_addr().expect("address"));
        let remote = Arc::new(Remote {
            release: Semaphore::new(0),
            ..Default::default()
        });
        let app = Router::new()
            .route(
                "/mcp",
                post(endpoint)
                    .get(|| async { StatusCode::METHOD_NOT_ALLOWED })
                    .delete(|| async { StatusCode::OK }),
            )
            .with_state(remote.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("HTTP fixture");
        });
        let mut record = json!({"capability_id":CAP,"kind":"mcp_toolpack","name":"Staged fixture","install_mode":"remote_only","install_spec":{"url":url},"support":{"form":"remote_http","auth":"none"}});
        if bearer {
            record["support"]["auth"] = json!("bearer");
            record["support"]["credential"] = json!({"kind":"bearer","env":KEY});
            crate::ops::actor_secrets::replace(
                &home,
                &group.group_id,
                "one",
                [(
                    KEY.to_owned(),
                    "fixture-secret-not-a-provider-token".to_owned(),
                )]
                .into_iter()
                .collect(),
            )
            .expect("fixture credential");
        }
        CapabilityStore::new(home.clone())
            .import_record(record.clone())
            .expect("fixture record");
        Self {
            _temp: temp,
            home,
            group: group.group_id,
            other: other.group_id,
            record,
            locks: DispatchLocks::default(),
            remote,
            server,
        }
    }

    fn request(&self, op: &str, args: Value) -> DaemonRequest {
        let mut envelope = json!({"group_id":self.group,"by":"one","actor_id":"one"})
            .as_object()
            .expect("envelope")
            .clone();
        envelope.extend(args.as_object().expect("arguments").clone());
        DaemonRequest {
            v: 1,
            op: op.into(),
            args: envelope,
        }
    }

    async fn rpc(&self, op: &str, args: Value) -> DaemonResponse {
        rpc(
            self.home.clone(),
            self.locks.clone(),
            self.request(op, args),
        )
        .await
    }

    fn start(&self, op: &str, args: Value) -> tokio::task::JoinHandle<DaemonResponse> {
        tokio::spawn(rpc(
            self.home.clone(),
            self.locks.clone(),
            self.request(op, args),
        ))
    }

    fn hold(&self, method: &str) {
        *self.remote.held.lock().expect("held") = method.into();
    }

    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.remote.entered.notified())
            .await
            .expect("external request reached barrier");
    }

    fn release(&self) {
        self.remote.release.add_permits(1);
    }

    async fn enable(&self) {
        let response = self
            .rpc(
                "capability_enable",
                json!({"capability_id":CAP,"scope":"actor"}),
            )
            .await;
        assert!(response.ok, "{:?}", response.error);
    }

    fn runtime(&self) -> Value {
        cccc_core::fs::read_json(&self.home.root().join("state/capabilities/runtime.json"))
            .unwrap_or_else(|error| {
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                json!({})
            })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.remote.release.add_permits(32);
        self.server.abort();
    }
}

// Exercise byte framing, operation selection and the shared dispatch locks,
// rather than invoking the legacy synchronous handler without its permits.
async fn rpc(home: HomeLayout, locks: DispatchLocks, request: DaemonRequest) -> DaemonResponse {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("private IPC port");
    let client = tokio::net::TcpStream::connect(listener.local_addr().expect("IPC address"))
        .await
        .expect("IPC connection");
    let (server, _) = listener.accept().await.expect("accept IPC");
    let (shutdown, _) = watch::channel(false);
    let task = crate::server_connection::spawn_connection(server, home, shutdown, locks);
    let mut client = BufReader::new(client);
    let mut bytes = serde_json::to_vec(&request).expect("request JSON");
    bytes.push(b'\n');
    client
        .get_mut()
        .write_all(&bytes)
        .await
        .expect("write request");
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), client.read_line(&mut line))
        .await
        .expect("request returned without releasing external barrier")
        .expect("read response");
    drop(client);
    task.await.expect("connection finished");
    serde_json::from_str(&line).expect("daemon response")
}

#[tokio::test]
async fn id_only_reads_keep_complete_skill_text_without_enabling_or_calling() {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("groups")
        .create("skill read", "")
        .expect("group");
    let id = "skill:test:complete-rules";
    let text = "\n  keep complete rules\n".repeat(100);
    let store = CapabilityStore::new(home.clone());
    store
        .import_record(json!({"capability_id":id,"kind":"skill","capsule_text":text}))
        .expect("skill");
    for tool_name in [None, Some(json!("")), Some(Value::Null)] {
        let mut args = json!({"group_id":group.group_id,"by":"user",
            "capability_id":id,"input":{"literal":"unchanged"}})
        .as_object()
        .expect("request args")
        .clone();
        if let Some(name) = tool_name {
            args.insert("tool_name".into(), name);
        }
        let request = DaemonRequest {
            v: 1,
            op: "capability_tool_call".into(),
            args,
        };
        assert!(
            !handles(&request),
            "ID reads use the ordinary dispatch path"
        );
        let response = rpc(home.clone(), DispatchLocks::default(), request).await;
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result["capability"]["capsule_text"], text);
        assert_eq!(response.result["input"], json!({"literal":"unchanged"}));
        assert_eq!(response.result["ready"], true);
    }
    assert!(!home.root().join("state/capabilities/runtime.json").exists());
    for scope in ["group", "actor", "session"] {
        assert!(
            !store
                .is_enabled_for(id, &group.group_id, "user", scope)
                .expect("capability binding")
        );
    }
}

#[tokio::test]
async fn a_failed_phase_does_not_poison_the_runtime_start_gate() {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let locks = DispatchLocks::default();
    let request = DaemonRequest {
        v: 1,
        op: "ping".into(),
        args: Map::new(),
    };
    let error = phase(&home, &request, &locks, |_, _| -> Result<(), OpError> {
        panic!("a failed request must not disable unrelated runtime starts");
    })
    .await
    .expect_err("the failed request is still reported");
    assert_eq!(error.code, "dispatch_failed");
    let permit = crate::runtime_start_gate::permit(&home).expect("runtime starts remain available");
    drop(permit);
    tokio::time::timeout(
        Duration::from_secs(5),
        phase(&home, &request, &locks, |_, _| Ok(())),
    )
    .await
    .expect("the failed request released dispatch")
    .expect("another phase can execute");
}

#[tokio::test]
async fn direct_calls_keep_recovery_guidance_and_distinguish_a_missing_invoker() {
    let fixture = Fixture::new(false).await;
    fixture.enable().await;
    let missing_name = "cccc_ext_00000000_missing";
    let missing = fixture
        .rpc("capability_tool_call", json!({"tool_name":missing_name}))
        .await;
    let error = missing.error.expect("unknown direct name");
    assert_eq!(error.code, "capability_tool_not_found");
    assert_eq!(
        error.message,
        format!(
            "tool not found or not enabled: {missing_name}; prepare with cccc_capability_use(capability_id=...) before retrying a direct tool name"
        )
    );

    let mut runtime = fixture.runtime();
    let artifact = runtime["capability_artifacts"][CAP]
        .as_str()
        .expect("installed artifact")
        .to_owned();
    runtime["artifacts"][&artifact]
        .as_object_mut()
        .expect("artifact")
        .remove("invoker");
    cccc_core::fs::write_json(
        &fixture.home.root().join("state/capabilities/runtime.json"),
        &runtime,
    )
    .expect("fixture missing invoker");
    let broken = fixture
        .rpc(
            "capability_tool_call",
            json!({"capability_id":CAP,"tool_name":"echo"}),
        )
        .await;
    let error = broken.error.expect("incomplete installation");
    assert_eq!(error.code, "capability_runtime_unavailable");
    assert_eq!(error.message, "external capability invoker is missing");
    assert_eq!(fixture.remote.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_enable_releases_dispatch_and_noop_disable_prevents_publication() {
    let fixture = Fixture::new(false).await;
    fixture.hold("tools/list");
    let slow = fixture.start(
        "capability_enable",
        json!({"capability_id":CAP,"scope":"actor"}),
    );
    fixture.entered().await;
    for (op, args) in [
        ("ping", json!({"group_id":fixture.other,"by":"user"})),
        (
            "send",
            json!({"by":"user","to":["one"],"text":"fixture mail","message_mode":"mail"}),
        ),
        ("inbox_read", json!({})),
    ] {
        assert!(fixture.rpc(op, args).await.ok, "{op}");
    }
    let disabled = fixture
        .rpc(
            "capability_enable",
            json!({"capability_id":CAP,"enabled":false,"scope":"actor"}),
        )
        .await;
    assert!(disabled.ok);
    assert!(!slow.is_finished());
    fixture.release();
    let response = slow.await.expect("slow request");
    assert_eq!(
        response.error.expect("superseded").code,
        "capability_conflict"
    );
    assert_eq!(fixture.runtime()["capability_artifacts"][CAP], Value::Null);
    assert!(
        !CapabilityStore::new(fixture.home.clone())
            .is_enabled_for(CAP, &fixture.group, "one", "actor")
            .expect("enabled")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_equivalent_enables_reuse_the_winner_without_cross_member_revocation() {
    let fixture = Fixture::new(false).await;
    fixture.hold("tools/list");
    let first = fixture.start(
        "capability_enable",
        json!({"capability_id":CAP,"scope":"actor"}),
    );
    fixture.entered().await;
    let second = fixture.start(
        "capability_enable",
        json!({"capability_id":CAP,"scope":"actor","by":"two","actor_id":"two"}),
    );
    fixture.entered().await;
    // A no-op revocation for an unrelated group must not reject either member.
    let other = fixture.rpc("capability_enable", json!({"group_id":fixture.other,"by":"user","actor_id":"user","capability_id":CAP,"enabled":false,"scope":"actor"})).await;
    assert!(other.ok);
    fixture.release();
    fixture.release();
    for request in [first, second] {
        let response = request.await.expect("enable result");
        assert!(response.ok, "{:?}", response.error);
    }
    let store = CapabilityStore::new(fixture.home.clone());
    for actor in ["one", "two"] {
        assert!(
            store
                .is_enabled_for(CAP, &fixture.group, actor, "actor")
                .expect("enabled")
        );
    }
    assert_eq!(
        fixture.runtime()["artifacts"]
            .as_object()
            .expect("artifacts")
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_bearer_discovery_rechecks_revocation_before_any_business_call() {
    let fixture = Fixture::new(true).await;
    let enabled = fixture
        .rpc(
            "capability_enable",
            json!({"capability_id":CAP,"scope":"actor","prepare":false}),
        )
        .await;
    assert!(enabled.ok, "{:?}", enabled.error);
    fixture.hold("tools/list");
    let slow = fixture.start(
        "capability_tool_call",
        json!({"capability_id":CAP,"tool_name":"echo","arguments":{}}),
    );
    fixture.entered().await;
    let blocked = fixture
        .rpc(
            "capability_block",
            json!({"capability_id":CAP,"by":"user","scope":"group","blocked":true}),
        )
        .await;
    assert!(blocked.ok);
    let unblocked = fixture
        .rpc(
            "capability_block",
            json!({"capability_id":CAP,"by":"user","scope":"group","blocked":false}),
        )
        .await;
    assert!(unblocked.ok);
    fixture.release();
    let response = slow.await.expect("call result");
    assert_eq!(response.error.expect("revoked").code, "capability_conflict");
    assert_eq!(fixture.remote.calls.load(Ordering::SeqCst), 0);
    let record =
        credentials::installed_record(&fixture.home, &fixture.record).expect("installed recipe");
    assert!(credentials::cached_tools(&fixture.home, &fixture.group, "one", &record).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_call_releases_own_group_and_global_writers_before_recording_result() {
    let fixture = Fixture::new(false).await;
    fixture.enable().await;
    fixture.hold("tools/call");
    let slow = fixture.start(
        "capability_tool_call",
        json!({"capability_id":CAP,"tool_name":"echo","arguments":{}}),
    );
    fixture.entered().await;
    // Awaiting the actual global mutation proves it completed while the call
    // remains held; a sleep cannot prove that a writer was ever queued.
    assert!(
        fixture
            .rpc(
                "capability_visibility",
                json!({"capability_id":CAP,"hidden":false})
            )
            .await
            .ok
    );
    for (op, args) in [
        (
            "send",
            json!({"by":"user","to":["one"],"text":"during call","message_mode":"mail"}),
        ),
        ("inbox_read", json!({})),
        ("ping", json!({"group_id":fixture.other,"by":"user"})),
    ] {
        assert!(fixture.rpc(op, args).await.ok, "{op}");
    }
    assert!(!slow.is_finished());
    fixture.release();
    let response = slow.await.expect("call");
    assert!(response.ok, "{:?}", response.error);
    assert_eq!(response.result["result"]["isError"], false);
    assert!(!response.result.contains_key("state_update_skipped"));
    assert_eq!(fixture.runtime()["recent_success"][CAP]["success_count"], 1);
    assert_eq!(fixture.remote.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inflight_call_cannot_restore_an_uninstalled_or_replaced_binding() {
    for replace_member in [false, true] {
        let fixture = Fixture::new(false).await;
        fixture.enable().await;
        fixture.hold("tools/call");
        let slow = fixture.start(
            "capability_tool_call",
            json!({"capability_id":CAP,"tool_name":"echo","arguments":{}}),
        );
        fixture.entered().await;
        assert!(
            fixture
                .rpc(
                    "capability_uninstall",
                    json!({"capability_id":CAP,"by":"user"})
                )
                .await
                .ok
        );
        if replace_member {
            cccc_core::GroupStore::new(fixture.home.clone())
                .expect("groups")
                .mutate(&fixture.group, |group| {
                    group
                        .actors
                        .iter_mut()
                        .find(|actor| actor.id == "one")
                        .expect("member")
                        .generation = uuid::Uuid::new_v4().to_string();
                    Ok(())
                })
                .expect("recreate member identity");
        }
        let after = fixture.runtime();
        fixture.release();
        let response = slow.await.expect("call");
        if replace_member {
            let error = response
                .error
                .expect("new member cannot receive old result");
            assert_eq!(error.code, "permission_denied");
            assert_eq!(error.details["external_action_may_have_occurred"], true);
            assert!(response.result.is_empty());
        } else {
            assert!(response.ok, "{:?}", response.error);
            assert_eq!(response.result["state_update_skipped"], true);
            assert_eq!(response.result["state_update_skipped_reason"], "revoked");
            assert_eq!(
                response.result["result"]["content"][0]["text"],
                "fixture-result"
            );
        }
        assert_eq!(
            fixture.runtime(),
            after,
            "no old binding, artifact, or success history is recreated"
        );
        assert_eq!(fixture.remote.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopped_preparation_has_no_late_publication_and_tool_errors_are_not_verified() {
    let fixture = Fixture::new(false).await;
    fixture.hold("tools/list");
    let slow = fixture.start(
        "capability_enable",
        json!({"capability_id":CAP,"scope":"actor"}),
    );
    fixture.entered().await;
    crate::runtime_start_gate::prevent(&fixture.home).expect("close publication gate");
    fixture.release();
    let response = slow.await.expect("enable");
    assert_eq!(
        response.error.expect("stopped").code,
        "daemon_shutting_down"
    );
    assert_eq!(fixture.runtime(), json!({}));
    crate::runtime_start_gate::allow(&fixture.home).expect("reopen fixture gate");
    fixture.hold("");
    fixture.enable().await;
    let response = fixture
        .rpc(
            "capability_tool_call",
            json!({"capability_id":CAP,"tool_name":"echo","arguments":{"fail":true}}),
        )
        .await;
    assert!(response.ok);
    assert_eq!(response.result["result"]["isError"], true);
    assert_eq!(fixture.runtime()["recent_success"][CAP], Value::Null);
    assert_eq!(
        fixture.runtime()["actor_instances"][&fixture.group]["one"][CAP]["state"],
        "tool_call_failed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bearer_rotation_and_group_replacement_discard_old_discovery() {
    for replace_group in [false, true] {
        let fixture = Fixture::new(true).await;
        fixture.hold("tools/list");
        let slow = fixture.start(
            "capability_enable",
            json!({"capability_id":CAP,"scope":"actor"}),
        );
        fixture.entered().await;
        if replace_group {
            cccc_core::GroupStore::new(fixture.home.clone())
                .expect("groups")
                .mutate(&fixture.group, |group| {
                    group.generation = uuid::Uuid::new_v4().to_string();
                    Ok(())
                })
                .expect("replacement group identity");
        } else {
            crate::ops::actor_secrets::replace(
                &fixture.home,
                &fixture.group,
                "one",
                [(KEY.to_owned(), "fixture-rotated-private-token".to_owned())]
                    .into_iter()
                    .collect(),
            )
            .expect("rotate fixture credential");
        }
        fixture.release();
        let response = slow.await.expect("enable result");
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("superseded").code,
            if replace_group {
                "permission_denied"
            } else {
                "capability_conflict"
            }
        );
        assert_eq!(fixture.runtime(), json!({}));
        assert!(
            credentials::cached_tools(&fixture.home, &fixture.group, "one", &fixture.record)
                .is_empty()
        );
        assert_eq!(fixture.remote.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_update_during_a_call_keeps_new_installation_and_skips_old_proof() {
    let fixture = Fixture::new(false).await;
    fixture.enable().await;
    fixture.hold("tools/call");
    let slow = fixture.start(
        "capability_tool_call",
        json!({"capability_id":CAP,"tool_name":"echo","arguments":{}}),
    );
    fixture.entered().await;
    let mut record = fixture.record.clone();
    let url = record["install_spec"]["url"].as_str().expect("URL");
    record["install_spec"]["url"] = json!(format!("{url}?revision=2"));
    CapabilityStore::new(fixture.home.clone())
        .import_record(record.clone())
        .expect("replace recipe");
    let updated = fixture
        .rpc(
            "capability_update",
            json!({"by":"user","capability_id":CAP,
        "expected_recipe_fingerprint":recipe::fingerprint(&record).expect("recipe fingerprint")}),
        )
        .await;
    assert!(updated.ok, "{:?}", updated.error);
    assert_eq!(updated.result["updated"], true);
    let after = fixture.runtime();
    fixture.release();
    let response = slow.await.expect("original call result");
    assert!(response.ok, "{:?}", response.error);
    assert_eq!(response.result["state_update_skipped"], true);
    assert_eq!(fixture.runtime(), after);
    assert_eq!(fixture.remote.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_closed_start_gate_refuses_stdio_before_spawning_a_process() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    crate::runtime_start_gate::prevent(&home).expect("closed gate");
    // If spawned this executable only prints its test-runner help; it cannot
    // recursively execute the suite. The closed gate must win before spawn.
    let command = vec![
        std::env::current_exe()
            .expect("test executable")
            .to_string_lossy()
            .into_owned(),
        "--help".into(),
    ];
    let error = super::super::mcp_stdio::call_tool_guarded(
        &home,
        &command,
        &Map::new(),
        "echo",
        Map::new(),
        Duration::from_secs(2),
    )
    .expect_err("must not start a tool after shutdown");
    assert!(error.message.contains("shutting down"), "{}", error.message);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_phase_waiting_for_a_runtime_start_does_not_keep_a_dispatch_permit() {
    use std::future::{Future, poll_fn};
    use std::task::Poll;
    let fixture = Fixture::new(false).await;
    let home = fixture.home.clone();
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let launch = tokio::task::spawn_blocking(move || {
        let _gate =
            crate::runtime_start_gate::permit(&home).expect("existing launch owns start gate");
        held_tx.send(()).expect("gate acquired");
        release_rx
            .recv_timeout(Duration::from_secs(8))
            .expect("release simulated launch");
    });
    held_rx.await.expect("start gate held");
    let request = fixture.request("capability_enable", json!({"capability_id":CAP}));
    let reader = fixture.locks.global_read().await;
    let mut short = Box::pin(phase(
        &fixture.home,
        &request,
        &fixture.locks,
        |_, _| Ok(()),
    ));
    // Poll the production phase until it really queues its global writer. A
    // subsequent read must get past that writer while the launch stays held.
    poll_fn(|cx| {
        assert!(short.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(reader);
    let ping = async {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            fixture.rpc("ping", json!({"group_id":fixture.other})),
        )
        .await;
        release_tx
            .send(())
            .expect("release launch even when the read timed out");
        result
            .expect("MCP read needed by an existing launch must not deadlock")
            .ok
    };
    let (phase, ping) = tokio::join!(short, ping);
    assert!(phase.is_ok());
    assert!(ping);
    launch.await.expect("simulated launch exited");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replaced_native_conversation_cannot_receive_the_previous_call_result() {
    use cccc_contracts::local_session::{LocalSessionBinding, LocalSessionRuntime};
    use cccc_core::local_sessions::{self as native, NativeHost, NativeSession};
    let fixture = Fixture::new(false).await;
    let host = NativeHost::capture(std::process::id()).expect("fixture process identity");
    let runtime = LocalSessionRuntime::Claude;
    let mut session = NativeSession {
        source_id: runtime.source_id(&host.key()),
        runtime,
        native_session_id: "fixture-native-one".into(),
        host,
        cwd: fixture._temp.path().to_string_lossy().into_owned(),
        title: "Fixture session".into(),
        origin: None,
        revision: "fixture-revision-one".into(),
        context_epoch: 0,
        codex_home: None,
        codex_can_receive: false,
        codex_program: None,
        claude_config_dir: None,
        pi_agent_dir: None,
        opencode_config_dir: None,
        opencode_server: None,
        retired: None,
    };
    native::write(&fixture.home, session.clone()).expect("register fixture session");
    let groups = cccc_core::GroupStore::new(fixture.home.clone()).expect("groups");
    groups
        .mutate(&fixture.group, |group| {
            let actor = group
                .actors
                .iter_mut()
                .find(|actor| actor.id == "one")
                .expect("member");
            actor.runtime = ActorRuntime::Claude;
            actor.external_session_id = session.source_id.clone();
            Ok(())
        })
        .expect("bind fixture member");
    fixture.enable().await;
    let group = groups.load(&fixture.group).expect("group");
    let binding = LocalSessionBinding {
        group_id: fixture.group.clone(),
        actor_id: "one".into(),
        actor_generation: cccc_core::actors::generation_identity(&group.actors[0]),
        source_id: session.source_id.clone(),
        session_revision: session.revision.clone(),
    };
    native::validate_binding(&fixture.home, &binding).expect("initial port authority");
    fixture.hold("tools/call");
    let slow = fixture.start("capability_tool_call", json!({"capability_id":CAP,"tool_name":"echo","arguments":{},"_cccc_native_binding":binding}));
    fixture.entered().await;
    session.native_session_id = "fixture-native-two".into();
    session.revision = "fixture-revision-two".into();
    native::write(&fixture.home, session).expect("replace native conversation");
    let after = fixture.runtime();
    fixture.release();
    let response = slow.await.expect("original call");
    let error = response.error.expect("old port authority rejected");
    assert_eq!(error.code, "permission_denied");
    assert_eq!(error.details["external_action_may_have_occurred"], true);
    assert!(response.result.is_empty());
    assert_eq!(fixture.runtime(), after);
    assert_eq!(fixture.remote.calls.load(Ordering::SeqCst), 1);
}
