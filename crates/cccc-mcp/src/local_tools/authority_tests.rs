//! Local authority fences using real daemon handlers and a delayed IPC reply.
use cccc_client::DaemonClient;
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, GroupState};
use cccc_core::{GroupStore, HomeLayout, web_model_connectors as bindings};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

pub(crate) struct Fixture {
    _temp: tempfile::TempDir,
    pub(crate) home: HomeLayout,
    pub(crate) root: PathBuf,
    pub(crate) group_id: String,
    pub(crate) actor_id: String,
    pub(crate) binding: Value,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let root = temp.path().join("project");
        std::fs::create_dir(&root).expect("project");
        std::fs::write(root.join("private.txt"), "retired-task-content").expect("file");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Dots local authority", "").expect("group");
        let mut actor = Actor::new("dots");
        actor.runtime = ActorRuntime::ChatgptDots;
        actor.enabled = true;
        cccc_core::actors::add(&mut group, actor).expect("actor");
        group.running = true;
        group.state = GroupState::Active;
        group.scopes.push(cccc_core::group::Scope {
            scope_key: "project".into(),
            url: root.to_string_lossy().into_owned(),
            label: String::new(),
            git_remote: String::new(),
        });
        group.active_scope_key = "project".into();
        group.extra.insert(
            "runtime_states".into(),
            json!({"dots":{"active_turn_id":"turn-one","active_event_ids":["source-one"]}}),
        );
        store.save(&group).expect("save");
        let configured = bindings::configure(&home).expect("configure");
        let id = configured["connector"]["connector_id"]
            .as_str()
            .expect("id");
        let bound = bindings::bind_dots(
            &home,
            id,
            &group.group_id,
            "dots",
            &group.actors[0].generation,
        )
        .expect("bind");
        bindings::dots_update(&home, id, |connector| {
            connector["dots"]["wake"] = json!({"event_id":"wake-one","state":"claimed"});
            connector["dots"]["execution"] = json!({"epoch":connector["dots"]["epoch"],
                "event_id":"wake-one","turn_id":"turn-one","event_ids":["source-one"],"state":"active"});
            Ok(())
        })
        .expect("execution");
        let connector = bindings::load(&home).expect("load").remove(0);
        let token = bindings::dots_wake_token(&connector, &bound, "wake-one").expect("token");
        let binding = bindings::dots_binding_for_token(&connector, &token, "cccc_repo_edit")
            .expect("execution binding");
        bindings::validate_binding(&home, &binding).expect("current authority");
        Self {
            _temp: temp,
            home,
            root,
            group_id: group.group_id,
            actor_id: "dots".into(),
            binding,
        }
    }

    pub(crate) fn args(&self, extra: Value) -> Map<String, Value> {
        let mut args = extra.as_object().expect("args").clone();
        args.insert("group_id".into(), json!(self.group_id));
        args.insert("by".into(), json!(self.actor_id));
        args.insert("_cccc_web_binding".into(), self.binding.clone());
        args
    }

    pub(crate) fn stop(&self) {
        let response = cccc_daemon::handle_request(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: "actor_stop".into(),
                args: json!({"group_id":self.group_id,"actor_id":self.actor_id,"by":"user"})
                    .as_object()
                    .expect("args")
                    .clone(),
            },
        );
        assert!(response.ok, "owned Actor Stop must succeed");
        assert!(
            bindings::validate_binding(&self.home, &self.binding).is_err(),
            "Stop must retire the captured task authority"
        );
    }
}

struct ReplyGate {
    client: DaemonClient,
    observed: oneshot::Receiver<DaemonRequest>,
    release: oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}

async fn reply_gate(fixture: &Fixture, expected_op: &'static str) -> ReplyGate {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let paths = cccc_daemon::DaemonPaths::new(fixture.home.clone());
    cccc_core::fs::write_json(&paths.address, &json!({"v":1,"transport":"tcp","path":"",
        "host":"127.0.0.1","port":listener.local_addr().expect("address").port(),
        "pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":cccc_contracts::utc_now()}))
        .expect("IPC address");
    let home = fixture.home.clone();
    let (observed_tx, observed) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.expect("request");
        let request: DaemonRequest = serde_json::from_str(&line).expect("JSON request");
        assert_eq!(request.op, expected_op, "expected the preparation read");
        // Prepare a valid canonical response before Stop. Delaying only its
        // transmission models revocation after the daemon read has finished.
        let response = cccc_daemon::handle_request(&home, &request);
        assert!(response.ok, "owned preparation read must succeed");
        observed_tx.send(request).expect("observe request");
        release_rx.await.expect("release response");
        let mut payload = serde_json::to_vec(&response).expect("JSON response");
        payload.push(b'\n');
        stream
            .get_mut()
            .write_all(&payload)
            .await
            .expect("response");
    });
    ReplyGate {
        client: DaemonClient::new(fixture.home.clone()),
        observed,
        release,
        server,
    }
}

fn local_calls() -> Vec<(&'static str, Value)> {
    #[allow(unused_mut)]
    let mut calls = vec![
        (
            "cccc_repo_edit",
            json!({"action":"write","path":"marker.txt","content":"effect"}),
        ),
        (
            "cccc_apply_patch",
            json!({"patch":"*** Begin Patch\n*** Add File: marker.txt\n+effect\n*** End Patch"}),
        ),
        ("cccc_file", json!({"path":"private.txt"})),
    ];
    #[cfg(unix)]
    calls.extend([
        (
            "cccc_shell",
            json!({"command":["sh","-c","printf effect > marker.txt"]}),
        ),
        (
            "cccc_exec_command",
            json!({"command":["sh","-c","printf effect > marker.txt"],"yield_time_ms":100}),
        ),
    ]);
    calls
}

async fn delayed_scope_call(
    fixture: &Fixture,
    name: &str,
    args: Value,
    stop: bool,
) -> (Result<Value, crate::ToolCallError>, DaemonRequest) {
    let ReplyGate {
        client,
        observed,
        release,
        server,
    } = reply_gate(fixture, "group_show").await;
    let call = crate::router::call_with_context(
        &fixture.home,
        &client,
        name,
        args.as_object().expect("args").clone(),
        Some(crate::RequestContext {
            binding: Some(&fixture.binding),
            group_id: &fixture.group_id,
            actor_id: &fixture.actor_id,
        }),
        false,
    );
    let revoke = async {
        let request = observed.await.expect("scope read reached");
        if stop {
            fixture.stop();
        }
        release.send(()).expect("release");
        request
    };
    let (result, request) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(call, revoke)
    })
    .await
    .expect("bounded local call");
    server.await.expect("server");
    if !stop
        && name == "cccc_exec_command"
        && let Ok(started) = &result
        && started["structuredContent"]["status"]["running"] == true
    {
        let id = started["structuredContent"]["session_id"]
            .as_str()
            .expect("session");
        // A first poll may finish before the shell runs. Wait for the owned
        // process to exit instead of racing its marker write with cleanup.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let page = crate::local_sessions::write(
                    &fixture.home,
                    &fixture.args(json!({"session_id":id,"yield_time_ms":100})),
                )
                .await
                .expect("current owner can finish polling");
                if page["status"]["running"] == false {
                    break;
                }
            }
        })
        .await
        .expect("owned command exited");
    }
    crate::local_sessions::shutdown(&fixture.home).expect("owned commands cleaned up");
    (result, request)
}

#[tokio::test]
async fn dots_local_tools_reject_retired_authority_after_scope_reply() {
    let mut executed = Vec::new();
    let mut forwarded = true;
    for (name, args) in local_calls() {
        let fixture = Fixture::new();
        let (result, request) = delayed_scope_call(&fixture, name, args, true).await;
        if result.is_ok() || fixture.root.join("marker.txt").exists() {
            executed.push(name);
        }
        forwarded &= request.args.get("by") == Some(&json!(fixture.actor_id))
            && request.args.get("_cccc_web_binding") == Some(&fixture.binding)
            && request.args.get("group_id") == Some(&json!(fixture.group_id));
    }
    assert!(
        executed.is_empty(),
        "retired authority executed local tools: {executed:?}"
    );
    assert!(
        forwarded,
        "scope must preserve Group, caller and captured task authority"
    );
}

#[tokio::test]
async fn dots_local_tools_preserve_valid_authority_and_forward_scope_identity() {
    for (name, args) in local_calls() {
        let fixture = Fixture::new();
        let (result, request) = delayed_scope_call(&fixture, name, args, false).await;
        assert!(
            result.is_ok(),
            "current authority must still execute {name}"
        );
        if name == "cccc_file" {
            assert!(
                result
                    .expect("read")
                    .to_string()
                    .contains("retired-task-content")
            );
        } else {
            assert!(
                fixture.root.join("marker.txt").exists(),
                "valid {name} must perform its local effect"
            );
        }
        assert!(
            request.args.get("by") == Some(&json!(fixture.actor_id)),
            "scope must preserve caller"
        );
        assert!(
            request.args.get("_cccc_web_binding") == Some(&fixture.binding),
            "scope must preserve captured task authority"
        );
        assert!(
            request.args.get("group_id") == Some(&json!(fixture.group_id)),
            "scope must preserve Group"
        );
    }
}

#[test]
fn dots_file_worker_rechecks_authority_when_dequeued() {
    use std::future::Future;
    use std::task::Poll;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .expect("owned single-worker runtime");
    runtime.block_on(async {
        let fixture = Fixture::new();
        let ReplyGate {
            client,
            observed,
            release,
            server,
        } = reply_gate(&fixture, "group_show").await;
        let args = fixture.args(json!({"path":"private.txt"}));
        let scope = super::scope(&client, &args);
        let send_scope = async {
            observed.await.expect("scope read reached");
            release.send(()).expect("release scope");
        };
        let (root, ()) = tokio::join!(scope, send_scope);
        let root = root.expect("current scope");
        server.await.expect("scope server");
        super::revalidate(&fixture.home, &args).expect("current post-scope authority");

        let (occupied_tx, occupied_rx) = oneshot::channel();
        let (release_worker, worker_gate) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            occupied_tx.send(()).expect("worker occupied");
            // Dropping the sender on a failed assertion also releases this
            // owned worker, so runtime cleanup never waits on a leaked gate.
            let _ = worker_gate.recv();
        });
        occupied_rx.await.expect("sole worker started");
        let mut read = std::pin::pin!(super::file(&fixture.home, &client, &root, &args, "read"));
        // This read path has no await before spawn_blocking. Its first poll
        // therefore queues the real worker after the valid scope check.
        std::future::poll_fn(|cx| {
            assert!(read.as_mut().poll(cx).is_pending(), "read must be queued");
            Poll::Ready(())
        })
        .await;
        fixture.stop();
        release_worker.send(()).expect("release worker");
        blocker.await.expect("owned blocker ended");
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), read)
            .await
            .expect("bounded dequeued read");
        assert!(
            result.is_err_and(|error| error.code == "permission_denied"),
            "a retired queued worker must not read or return file content"
        );
    });
}

#[tokio::test]
async fn dots_code_start_rechecks_authority_after_catalog_preparation() {
    if tokio::process::Command::new("node")
        .arg("--version")
        .output()
        .await
        .is_err()
    {
        return;
    }
    let fixture = Fixture::new();
    let ReplyGate {
        client,
        observed,
        release,
        server,
    } = reply_gate(&fixture, "capability_state").await;
    let args = fixture.args(json!({"source":"text('started after Stop')","yield_time_ms":100}));
    let call = crate::code_mode::start(&fixture.home, &client, &fixture.root, &args);
    let revoke = async {
        observed.await.expect("catalog preparation reached");
        fixture.stop();
        release.send(()).expect("release");
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(call, revoke)
    })
    .await
    .expect("bounded CodeMode preparation");
    server.await.expect("server");
    crate::code_mode::shutdown(&fixture.home).await;
    assert!(
        result.is_err(),
        "retired preparation must not start a code cell"
    );
}
