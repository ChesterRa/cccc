//! cccc_file_send to another Group on this instance, through real daemon IPC.
use cccc_client::DaemonClient;
use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout, actors, blobs};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn file_send_with_only_a_destination_group_delivers_the_file_locally() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("project");
    std::fs::write(root.join("diagram.png"), b"diagram").expect("file");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut source = store.create("Source", "").expect("source");
    actors::add(&mut source, Actor::new("worker")).expect("worker");
    source.scopes.push(cccc_core::group::Scope {
        scope_key: "project".into(),
        url: root.to_string_lossy().into(),
        label: String::new(),
        git_remote: String::new(),
    });
    source.active_scope_key = "project".into();
    store.save(&source).expect("source");
    let mut destination = store.create("Destination", "").expect("destination");
    actors::add(&mut destination, Actor::new("lead")).expect("lead");
    destination.state = cccc_contracts::GroupState::Paused;
    store.save(&destination).expect("destination");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("IPC listener");
    let paths = cccc_daemon::DaemonPaths::new(home.clone());
    cccc_core::fs::write_json(&paths.address,&json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1","port":listener.local_addr().expect("address").port(),"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":chrono::Utc::now().to_rfc3339()})).expect("IPC address");
    let server_home = home.clone();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.expect("request");
            let request: DaemonRequest = serde_json::from_str(&line).expect("JSON request");
            let response = cccc_daemon::handle_request(&server_home, &request);
            let mut bytes = serde_json::to_vec(&response).expect("JSON response");
            bytes.push(b'\n');
            stream.get_mut().write_all(&bytes).await.expect("response");
        }
    });
    let client = DaemonClient::new(home.clone());
    let sent: Value = crate::router::call_with_context(
        &home,
        &client,
        "cccc_file_send",
        json!({
            "path":"diagram.png","dst_group_id":destination.group_id,"to":["lead"],
            "mode":"send","insight":"The diagram for the shared design."
        })
        .as_object()
        .expect("args")
        .clone(),
        Some(crate::RequestContext {
            native_binding: None,
            binding: None,
            group_id: &source.group_id,
            actor_id: "worker",
        }),
        false,
    )
    .await
    .expect("file send");
    server.abort();

    let delivered = &sent["structuredContent"]["result"]["dst_event"];
    assert_eq!(delivered["group_id"], destination.group_id);
    assert_eq!(delivered["data"]["attachments"][0]["kind"], "image");
    let path = delivered["data"]["attachments"][0]["path"]
        .as_str()
        .expect("path");
    assert_eq!(
        std::fs::read(blobs::resolve(&home, &destination.group_id, path).expect("blob"))
            .expect("read"),
        b"diagram"
    );
}
