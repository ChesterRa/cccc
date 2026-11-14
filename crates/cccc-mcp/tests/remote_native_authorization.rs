//! Real MCP dispatcher, native binding validation, IPC and daemon authorization.
//! Only the IPC listener is test-owned; no remote gateway or runtime is started.
use cccc_contracts::{
    Actor, ActorRuntime, DaemonRequest, Event, local_session::*, session_node::*,
};
use cccc_core::{GroupStore, HomeLayout, actors, local_sessions::NativeHost, session_nodes};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn call(home: &HomeLayout, binding: &LocalSessionBinding, name: &str, args: Value) -> Value {
    deadline(cccc_mcp::handle_request_for_native_binding(home, &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}), binding)).await
}
fn payload(response: &Value) -> &Value {
    assert!(response.get("error").is_none(), "{response}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    &response["result"]["structuredContent"]
}
fn denied(response: &Value, reason: &str) {
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert!(response.to_string().contains(reason), "{response}");
}

struct Fixture {
    _server: AbortTask,
    _temp: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    group: cccc_core::GroupDoc,
    peer: Actor,
    root: std::path::PathBuf,
    binding: LocalSessionBinding,
}
struct AbortTask(tokio::task::JoinHandle<()>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn deadline<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(10), future)
        .await
        .expect("test I/O deadline")
}
impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        session_nodes::register_endpoint(
            &home,
            SessionNodeEndpoint {
                endpoint_id: "hub".into(),
                addresses: vec!["127.0.0.1:2345".into()],
                relay_url: None,
            },
            NativeHost::capture(std::process::id()).expect("owner"),
            "gateway",
        )
        .expect("endpoint");
        let invite = session_nodes::create_invite(&home).expect("invite");
        session_nodes::pair(
            &home,
            "gateway",
            "node",
            "Node",
            Some((&invite.invite_id, &invite.secret)),
        )
        .expect("pair");
        let source = session_nodes::update_sources(
            &home,
            "gateway",
            "node",
            vec![SessionNodeSource {
                source: LocalSessionSource {
                    source_id: "codex:fixture".into(),
                    runtime: LocalSessionRuntime::Codex,
                    native_session_id: "fixture".into(),
                    cwd: "/remote/only".into(),
                    title: "fixture".into(),
                    origin: None,
                    connected: true,
                },
                session_revision: "revision".into(),
                alive: true,
            }],
        )
        .expect("advertise")
        .remove(0);
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Remote authorization", "").expect("group");
        actors::add(&mut group, Actor::new("lead")).expect("foreman");
        let mut peer = Actor::new("remote-peer");
        peer.runtime = ActorRuntime::Codex;
        peer.external_session_id = source.source.source_id.clone();
        let peer = actors::add(&mut group, peer).expect("peer");
        let mut receiver = Actor::new("receiver");
        receiver.runtime = ActorRuntime::Codex;
        receiver.external_session_id = source.source.source_id.clone();
        actors::add(&mut group, receiver).expect("remote recipient");
        let root = temp.path().join("workspace");
        std::fs::create_dir(&root).expect("workspace");
        group.active_scope_key = "project".into();
        group.scopes.push(cccc_core::Scope {
            scope_key: "project".into(),
            url: root.to_string_lossy().into(),
            label: String::new(),
            git_remote: String::new(),
        });
        store.save(&group).expect("save");
        let binding = LocalSessionBinding {
            group_id: group.group_id.clone(),
            actor_id: peer.id.clone(),
            actor_generation: actors::generation_identity(&peer),
            source_id: source.source.source_id,
            session_revision: source.session_revision,
        };
        cccc_core::local_sessions::validate_binding(&home, &binding).expect("valid remote binding");
        let listener = deadline(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("IPC");
        cccc_core::fs::write_json(&cccc_daemon::DaemonPaths::new(home.clone()).address,
        &json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1","port":listener.local_addr().expect("port").port(),"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":chrono::Utc::now().to_rfc3339()})).expect("address");
        let server_home = home.clone();
        let server = tokio::spawn(async move {
            loop {
                let (stream, _) = deadline(listener.accept()).await.expect("accept");
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                deadline(stream.read_line(&mut line)).await.expect("read");
                let request: DaemonRequest = serde_json::from_str(&line).expect("request");
                let response = cccc_daemon::handle_request(&server_home, &request);
                deadline(stream.get_mut().write_all(
                    format!("{}\n", serde_json::to_string(&response).expect("response")).as_bytes(),
                ))
                .await
                .expect("write");
            }
        });
        Self {
            _server: AbortTask(server),
            _temp: temp,
            home,
            store,
            group,
            peer,
            root,
            binding,
        }
    }
    async fn lead(&self, name: &str, args: Value) {
        let response = deadline(cccc_mcp::handle_request_for_actor(&self.home,
 &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
 &self.group.group_id, "lead")).await;
        payload(&response);
    }
    fn messages(&self) -> (std::path::PathBuf, Event, Event) {
        let ledger = self
            .store
            .ledger_path(&self.group.group_id)
            .expect("ledger");
        let mut own = Event::new("chat.message", &self.group.group_id);
        own.by = self.peer.id.clone();
        own.data = json!({"text":"request","message_mode":"request_reply","to":["receiver"]})
            .as_object()
            .expect("message data object")
            .clone();
        cccc_core::ledger::append(&ledger, &own).expect("own message");
        let mut other = own.clone();
        other.id = uuid::Uuid::new_v4().simple().to_string();
        other.by = "lead".into();
        cccc_core::ledger::append(&ledger, &other).expect("other message");
        (ledger, own, other)
    }
}

#[tokio::test]
async fn remote_coordination_reads_and_preserves_parameter_authority() {
    let f = Fixture::new().await;
    let home = &f.home;
    let binding = &f.binding;

    f.lead(
        "cccc_coordination",
        json!({"action":"update_brief","objective":"Authorized objective"}),
    )
    .await;
    let forged = json!({"by":"user","actor_id":"lead","group_id":"forged"});
    let coordination = call(home, binding, "cccc_coordination", forged.clone()).await;
    assert_eq!(
        payload(&coordination)["coordination"]["brief"]["objective"],
        "Authorized objective"
    );
    denied(&call(home,binding,"cccc_coordination",json!({"action":"update_brief","objective":"forged","by":"user","actor_id":"lead","group_id":"forged"})).await,"permission_denied");
    let after = call(home, binding, "cccc_coordination", json!({})).await;
    assert_eq!(
        payload(&after)["coordination"]["brief"]["objective"],
        "Authorized objective"
    );
}

#[tokio::test]
async fn remote_backlog_reads_and_preserves_parameter_authority() {
    let f = Fixture::new().await;
    let home = &f.home;
    let binding = &f.binding;
    let root = &f.root;

    f.lead("cccc_backlog",json!({"action":"create","path":"SELECTED.md","scope_key":"project","scope_url":root,"keep_local":false})).await;
    let forged = json!({"by":"user","actor_id":"lead","group_id":"forged"});
    let backlog = call(home, binding, "cccc_backlog", forged.clone()).await;
    assert_eq!(payload(&backlog)["configured"], true);
    assert_eq!(payload(&backlog)["path"], "SELECTED.md");
    assert_eq!(
        std::fs::read_to_string(root.join("SELECTED.md")).expect("backlog file"),
        "# Backlog\n\n## Open\n\n## Closed\n"
    );
    denied(&call(home,binding,"cccc_backlog",json!({"action":"create","path":"BACKLOG.md","scope_key":"project","scope_url":root,"by":"user","actor_id":"lead","group_id":"forged"})).await,"permission_denied");
    assert!(!root.join("BACKLOG.md").exists());
    let after = call(home, binding, "cccc_backlog", json!({})).await;
    assert_eq!(payload(&after)["path"], "SELECTED.md");
}

#[tokio::test]
async fn remote_delivery_persists_only_own_message() {
    let f = Fixture::new().await;
    let home = &f.home;
    let binding = &f.binding;

    let (ledger, own, other) = f.messages();
    denied(&call(home,binding,"cccc_message_deliver",json!({"source_event_id":other.id,"actor_ids":["receiver"],"by":"user","actor_id":"lead","group_id":"forged"})).await,"only the source sender or user");
    let delivered = call(home, binding, "cccc_message_deliver", json!({"source_event_id":own.id,"actor_ids":["receiver"],"by":"user","actor_id":"lead","group_id":"forged"})).await;
    assert_eq!(payload(&delivered)["event"]["id"], own.id);
    assert_eq!(payload(&delivered)["actor_ids"], json!(["receiver"]));
    assert_eq!(payload(&delivered)["delivery_state"], "claimed");
    let events = cccc_core::ledger::read_all(&ledger).expect("persisted delivery");
    assert!(events.iter().any(|event| event.kind == "runtime.delivery"
        && event.data["source_event_id"] == own.id
        && event.data["actor_id"] == "receiver"
        && event.data["state"] == "claimed"));
}

#[tokio::test]
async fn remote_cancellation_persists_and_is_idempotent() {
    let f = Fixture::new().await;
    let home = &f.home;
    let binding = &f.binding;
    let peer = &f.peer;
    let group = &f.group;

    let (ledger, own, other) = f.messages();
    denied(&call(home,binding,"cccc_reply_request_cancel",json!({"source_event_id":other.id,"actor_ids":["receiver"],"by":"user","actor_id":"lead","group_id":"forged"})).await,"only the source sender or user");
    let cancelled = call(
        home,
        binding,
        "cccc_reply_request_cancel",
        json!({"source_event_id":own.id,"by":"user","actor_id":"lead","group_id":"forged"}),
    )
    .await;
    assert_eq!(payload(&cancelled)["event"]["by"], peer.id);
    assert_eq!(payload(&cancelled)["event"]["group_id"], group.group_id);
    assert_eq!(
        payload(&cancelled)["event"]["kind"],
        "chat.reply_request.cancelled"
    );
    let repeated = call(
        home,
        binding,
        "cccc_reply_request_cancel",
        json!({"source_event_id":own.id}),
    )
    .await;
    assert_eq!(payload(&repeated)["duplicate"], true);
    assert_eq!(
        payload(&repeated)["event"]["id"],
        payload(&cancelled)["event"]["id"]
    );
    let events = cccc_core::ledger::read_all(&ledger).expect("persisted cancellation");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "chat.reply_request.cancelled"
                && event.data["source_event_id"] == own.id)
            .count(),
        1
    );
}

#[tokio::test]
async fn remote_tools_require_live_membership_and_collaboration_scope() {
    let f = Fixture::new().await;
    let home = &f.home;
    let store = &f.store;
    let peer = &f.peer;

    let mut binding = f.binding.clone();
    let mut group = f.group.clone();
    for name in [
        "cccc_file",
        "cccc_file_send",
        "cccc_capability_use",
        "cccc_connect",
        "cccc_actor",
        "cccc_shell",
    ] {
        denied(
            &call(home, &binding, name, json!({})).await,
            "collaboration tools only",
        );
    }
    denied(
        &call(
            home,
            &binding,
            "cccc_message_deliver",
            json!({"refs":[{"path":"secret"}]}),
        )
        .await,
        "File references",
    );
    let revision = binding.session_revision.clone();
    binding.session_revision = "stale".into();
    denied(
        &call(home, &binding, "cccc_coordination", json!({})).await,
        "no longer valid",
    );
    binding.session_revision = revision;
    let generation = binding.actor_generation.clone();
    binding.actor_generation = "stale".into();
    denied(
        &call(home, &binding, "cccc_backlog", json!({})).await,
        "no longer valid",
    );
    binding.actor_generation = generation;
    group
        .actors
        .iter_mut()
        .find(|actor| actor.id == peer.id)
        .expect("peer")
        .enabled = false;
    store.save(&group).expect("pause");
    denied(
        &call(home, &binding, "cccc_coordination", json!({})).await,
        "no longer valid",
    );
}
