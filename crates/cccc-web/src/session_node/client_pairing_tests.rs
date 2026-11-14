//! Temporary Homes, real iroh Hello and real daemon dispatch over test-owned IPC.
use super::*;
use cccc_contracts::DaemonRequest;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    task::JoinHandle,
};

async fn deadline<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("test I/O deadline")
}
struct AbortTask<T>(JoinHandle<T>);
impl<T> Drop for AbortTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl<T> std::future::Future for AbortTask<T> {
    type Output = std::result::Result<T, tokio::task::JoinError>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::pin::Pin::new(&mut self.0).poll(cx)
    }
}
impl<T> AbortTask<T> {
    fn abort(&self) {
        self.0.abort();
    }
}
// Replace the destination only after commit has read and compared the descriptor.
// Renaming a file over a directory fails even for root; Drop restores the old file.
struct BlockReplacement {
    path: std::path::PathBuf,
    backup: std::path::PathBuf,
}
impl BlockReplacement {
    fn new(path: std::path::PathBuf) -> Self {
        let backup = path.with_extension("before-write");
        std::fs::rename(&path, &backup).expect("preserve descriptor");
        let guard = Self { path, backup };
        std::fs::create_dir(&guard.path).expect("block replacement");
        guard
    }
}
impl Drop for BlockReplacement {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
        std::fs::rename(&self.backup, &self.path).expect("restore descriptor");
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    old: Value,
    hub: iroh::Endpoint,
    ipc: AbortTask<()>,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("isolated Home");
        let home = HomeLayout::from_path(temp.path().join("node")).expect("home");
        home.initialize().expect("initialize");
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("owner");
        let owner = NativeHost::capture(child.id()).expect("capture");
        child.kill().expect("kill");
        child.wait().expect("reap");
        let old = ClientDescriptor {
            endpoint: SessionNodeEndpoint {
                endpoint_id: "old-hub".into(),
                addresses: vec!["127.0.0.1:1".into()],
                relay_url: None,
            },
            node_id: "old-node".into(),
            grant_id: "old-grant".into(),
            owner,
            address: "127.0.0.1:1".parse().expect("address"),
            secret: "old-secret".into(),
            connected: false,
            memberships: vec![membership("old-hub")],
        };
        session_nodes::set_client(&home, &old).expect("old reconnect state");
        let listener = deadline(TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("IPC");
        cccc_core::fs::write_json(&cccc_daemon::DaemonPaths::new(home.clone()).address,
            &json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1","port":listener.local_addr().expect("port").port(),"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":chrono::Utc::now().to_rfc3339()})).expect("address");
        let server_home = home.clone();
        let ipc = AbortTask(tokio::spawn(async move {
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
        }));
        let hub = deadline(
            network::builder(iroh::SecretKey::generate(), 0, None, None)
                .expect("builder")
                .bind(),
        )
        .await
        .expect("hub");
        Self {
            _temp: temp,
            home,
            old: serde_json::to_value(old).expect("snapshot"),
            hub,
            ipc,
        }
    }

    fn saved(&self) -> Value {
        serde_json::to_value(session_nodes::saved_client(&self.home).expect("saved")).expect("json")
    }

    async fn hello(
        &self,
    ) -> (
        AbortTask<Result<()>>,
        iroh::endpoint::Connection,
        iroh::endpoint::SendStream,
    ) {
        self.hello_with_write_fault(None).await
    }
    async fn hello_with_write_fault(
        &self,
        fault: Option<pairing_state::WriteFault>,
    ) -> (
        AbortTask<Result<()>>,
        iroh::endpoint::Connection,
        iroh::endpoint::SendStream,
    ) {
        let ticket = SessionNodeTicket {
            v: SESSION_NODE_PROTOCOL,
            endpoint_id: self.hub.id().to_string(),
            addresses: network::address(&self.hub).expect("route").addresses,
            relay_url: None,
            relay_token: None,
            invite_id: "test-invite".into(),
            secret: "test-secret".into(),
            expires_at: "future".into(),
        };
        let home = self.home.clone();
        let task = AbortTask(tokio::spawn(
            pairing_state::BEFORE_WRITE.scope(std::cell::RefCell::new(fault), async move {
                connect(&home, Some(ticket), "test-node").await
            }),
        ));
        let connection = tokio::time::timeout(Duration::from_secs(10), async {
            self.hub
                .accept()
                .await
                .expect("incoming")
                .await
                .expect("connection")
        })
        .await
        .expect("connect deadline");
        let (send, mut recv) = deadline(connection.accept_bi())
            .await
            .expect("Hello stream");
        assert!(matches!(
            deadline(protocol::read::<Request>(&mut recv))
                .await
                .expect("Hello"),
            Request::Hello { .. }
        ));
        (task, connection, send)
    }

    async fn accept(
        &self,
        connection: &iroh::endpoint::Connection,
        send: &mut iroh::endpoint::SendStream,
    ) {
        deadline(protocol::write(send, &json!({"peer":SessionNodePeer {
            node_id:connection.remote_id().to_string(), name:"node".into(), grant_id:"new-grant".into(), enabled:true,
            connected:true, granted_at:"now".into(), last_seen_at:"now".into(),
        },"memberships":[membership(&self.hub.id().to_string())] }))).await.expect("Hello response");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.ipc.abort();
    }
}
fn membership(hub: &str) -> SessionNodeMembership {
    SessionNodeMembership {
        hub_id: hub.into(),
        source_id: "source".into(),
        source_revision: "revision".into(),
        group_id: "group".into(),
        group_title: "Group".into(),
        actor_id: "actor".into(),
        actor_generation: "generation".into(),
        role: "peer".into(),
        scope_key: String::new(),
        enabled: true,
    }
}
async fn finished(task: AbortTask<Result<()>>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("exit deadline")
        .expect("task")
}

#[tokio::test]
async fn rejected_hello_preserves_old_reconnect_state() {
    let f = Fixture::new().await;
    let (task, _connection, mut send) = f.hello().await;
    deadline(protocol::write(&mut send, &json!({"error":"rejected"})))
        .await
        .expect("reject");
    assert!(finished(task).await.is_err());
    assert_eq!(f.saved(), f.old);
}

#[tokio::test]
async fn cancelled_hello_preserves_old_reconnect_state() {
    let f = Fixture::new().await;
    let (task, _connection, _send) = f.hello().await;
    task.abort();
    assert!(deadline(task).await.expect_err("cancelled").is_cancelled());
    assert_eq!(f.saved(), f.old);
}

#[tokio::test]
async fn successful_hello_commits_route_grant_and_memberships_together() {
    let f = Fixture::new().await;
    let (task, connection, mut send) = f.hello().await;
    assert_eq!(f.saved(), f.old, "Hello not accepted yet");
    f.accept(&connection, &mut send).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if f.saved()["grant_id"] == "new-grant" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("commit");
    let saved = f.saved();
    assert_eq!(saved["endpoint"]["endpoint_id"], f.hub.id().to_string());
    assert_eq!(
        saved["memberships"],
        json!([membership(&f.hub.id().to_string())])
    );
    assert_eq!(saved["node_id"], connection.remote_id().to_string());
    assert_ne!(saved["secret"], f.old["secret"]);
    assert_eq!(saved["connected"], true);
    task.abort();
    let _ = deadline(task).await;
    let offline = f.saved();
    assert_eq!(offline["connected"], false);
    assert_eq!(offline["grant_id"], "new-grant");
    assert_eq!(offline["endpoint"], saved["endpoint"]);
}

#[tokio::test]
async fn concurrent_update_is_not_overwritten_by_hello_or_cleanup() {
    let f = Fixture::new().await;
    let (task, connection, mut send) = f.hello().await;
    let mut replacement: ClientDescriptor = serde_json::from_value(f.old.clone()).expect("old");
    replacement.grant_id = "user-updated".into();
    session_nodes::set_client(&f.home, &replacement).expect("concurrent update");
    f.accept(&connection, &mut send).await;
    assert!(finished(task).await.is_err());
    assert_eq!(
        f.saved(),
        serde_json::to_value(replacement).expect("replacement")
    );
}

#[tokio::test]
async fn persistence_lock_failure_preserves_old_reconnect_state() {
    let f = Fixture::new().await;
    let (task, connection, mut send) = f.hello().await;
    let lock = f.home.root().join("state/session_nodes/state.lock");
    std::fs::remove_file(&lock).expect("remove idle lock");
    std::fs::create_dir(&lock).expect("force lock open failure");
    f.accept(&connection, &mut send).await;
    assert!(finished(task).await.is_err());
    assert_eq!(f.saved(), f.old);
}

#[tokio::test]
async fn cancellation_does_not_roll_back_a_concurrent_update() {
    let f = Fixture::new().await;
    let (task, _connection, _send) = f.hello().await;
    let mut replacement: ClientDescriptor = serde_json::from_value(f.old.clone()).expect("old");
    replacement.grant_id = "user-updated".into();
    session_nodes::set_client(&f.home, &replacement).expect("concurrent update");
    task.abort();
    assert!(deadline(task).await.expect_err("cancelled").is_cancelled());
    assert_eq!(
        f.saved(),
        serde_json::to_value(replacement).expect("replacement")
    );
}

#[tokio::test]
async fn descriptor_write_failure_preserves_old_reconnect_state() {
    let f = Fixture::new().await;
    let path = f.home.root().join("state/session_nodes/client.json");
    let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = reached.clone();
    let fault = Box::new(move || {
        observed.store(true, std::sync::atomic::Ordering::SeqCst);
        Box::new(BlockReplacement::new(path)) as Box<dyn Send>
    });
    let (task, connection, mut send) = f.hello_with_write_fault(Some(fault)).await;
    f.accept(&connection, &mut send).await;
    let error = finished(task)
        .await
        .expect_err("atomic replacement must fail");
    assert!(
        error.downcast_ref::<std::io::Error>().is_some(),
        "{error:#}"
    );
    assert!(
        reached.load(std::sync::atomic::Ordering::SeqCst),
        "real commit reached filesystem write boundary"
    );
    assert_eq!(f.saved(), f.old);
}
