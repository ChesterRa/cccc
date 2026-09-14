//! Real MCP mapping and IPC with daemon handlers; no account/peer network worker.
use cccc_client::DaemonClient;
use cccc_contracts::{Actor, DaemonRequest, connect::*};
use cccc_core::{
    GroupStore, HomeLayout, actors, connect, connect_catalog, instance_identity::InstanceIdentity,
    membership,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn connect_tools_use_local_actor_context_and_canonical_queue_through_ipc() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let local = InstanceIdentity::load_or_create(&home).expect("local identity");
    let remote_home = HomeLayout::from_path(temp.path().join("remote")).expect("remote");
    let remote = InstanceIdentity::load_or_create(&remote_home).expect("remote identity");
    let now = chrono::Utc::now();
    let instances = [&local, &remote]
        .into_iter()
        .enumerate()
        .map(|(index, key)| ConnectInstance {
            instance_id: key.peer_id.clone(),
            public_key: key.public_key_b64.clone(),
            device_id: format!("device-{index}"),
            client_version: env!("CARGO_PKG_VERSION").into(),
            public_origin: Some(format!("https://instance-{index}.example.invalid")),
            display_name: format!("Instance {index}"),
            registered_at: now.to_rfc3339(),
        })
        .collect::<Vec<_>>();
    membership::save(
        &home,
        &membership::MembershipState {
            logged_in: true,
            account_origin: Some("https://account.example.invalid".into()),
            device_id: Some("device-0".into()),
            device_token: Some("fixture-only".into()),
            ..Default::default()
        },
    )
    .expect("membership");
    connect::save(
        &home,
        &connect::ConnectSnapshot {
            account_origin: "https://account.example.invalid".into(),
            device_id: "device-0".into(),
            instance_id: local.peer_id.clone(),
            directory: Some(ConnectDirectory {
                protocol_version: 1,
                account_id: "account".into(),
                device_id: "device-0".into(),
                issued_at: now.to_rfc3339(),
                expires_at: (now + chrono::Duration::seconds(120)).to_rfc3339(),
                instances,
            }),
            ..Default::default()
        },
    )
    .expect("directory");
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("project");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Local", "").expect("group");
    actors::add(&mut group, Actor::new("lead")).expect("lead");
    actors::add(&mut group, Actor::new("worker")).expect("peer");
    group.scopes.push(cccc_core::group::Scope {
        scope_key: "project".into(),
        url: root.to_string_lossy().into(),
        label: String::new(),
        git_remote: String::new(),
    });
    group.active_scope_key = "project".into();
    store.save(&group).expect("group");
    let remote_actor = ConnectActor {
        id: "worker".into(),
        title: "Remote worker".into(),
        generation: uuid::Uuid::new_v4().to_string(),
        enabled: true,
        role: Some(cccc_contracts::ActorRole::Foreman),
    };
    connect_catalog::save(
        &home,
        &connect_catalog::PeerCatalog {
            connection_id: None,
            account_origin: "https://account.example.invalid".into(),
            account_id: "account".into(),
            local_device_id: "device-0".into(),
            remote_instance_id: remote.peer_id.clone(),
            remote_device_id: "device-1".into(),
            remote_origin: "https://instance-1.example.invalid".into(),
            checked_at: now.to_rfc3339(),
            groups: vec![
                ConnectGroup {
                    group_id: group.group_id.clone(),
                    title: "Same ID, remote Group".into(),
                    actors: vec![remote_actor],
                },
                ConnectGroup {
                    group_id: "zz-last".into(),
                    title: "Last".into(),
                    actors: vec![],
                },
            ],
        },
    )
    .expect("catalog");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("IPC listener");
    let paths = cccc_daemon::DaemonPaths::new(home.clone());
    cccc_core::fs::write_json(&paths.address,&json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1","port":listener.local_addr().expect("address").port(),"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":now.to_rfc3339()})).expect("IPC address");
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
    let call = |name, args: Value| {
        crate::router::call_with_context(
            &home,
            &client,
            name,
            args.as_object().expect("args").clone(),
            Some(crate::RequestContext {
                group_id: &group.group_id,
                actor_id: "worker",
            }),
            false,
        )
    };
    let directory = call("cccc_connect", json!({"by":"user","group_id":"forged"}))
        .await
        .expect("directory");
    assert_eq!(
        directory["structuredContent"]["instances"][0]["instance_id"],
        remote.peer_id
    );
    let first = call(
        "cccc_connect",
        json!({"instance_id":remote.peer_id,"limit":1}),
    )
    .await
    .expect("first page");
    assert_eq!(
        first["structuredContent"]["catalog"]["groups"]
            .as_array()
            .expect("groups")
            .len(),
        1
    );
    let second = call(
        "cccc_connect",
        json!({"instance_id":remote.peer_id,"after":first["structuredContent"]["next"],"limit":1}),
    )
    .await
    .expect("next page");
    assert_eq!(
        second["structuredContent"]["catalog"]["groups"][0]["group_id"],
        "zz-last"
    );
    assert!(
        cccc_core::connect_delivery::pending_ids(&home)
            .expect("queue")
            .is_empty(),
        "discovery must not enqueue work"
    );
    let arguments = json!({"by":"user","dst_instance_id":remote.peer_id,"dst_group_id":group.group_id,"text":"Remote task","mode":"send","insight":"This task belongs to the other instance.","idempotency_key":"stable-send"});
    let sent = call("cccc_message_send", arguments.clone())
        .await
        .expect("send");
    assert_eq!(sent["structuredContent"]["accepted"], true);
    assert_eq!(sent["structuredContent"]["queued"], true);
    let event = &sent["structuredContent"]["source_event"];
    assert_eq!(
        event["by"], "worker",
        "context overrides caller impersonation"
    );
    assert_eq!(event["data"]["client_id"], "stable-send");
    assert_eq!(event["data"]["dst_actor_titles"]["worker"], "Remote worker");
    let retry = call("cccc_message_send", arguments).await.expect("retry");
    assert_eq!(
        retry["structuredContent"]["delivery_id"],
        sent["structuredContent"]["delivery_id"]
    );
    let file = root.join("note.txt");
    std::fs::write(&file, "file payload").expect("file");
    let args = json!({"action":"send","path":"note.txt","dst_instance_id":remote.peer_id,"dst_group_id":group.group_id,"idempotency_key":"stable-file","insight":"The remote task needs this attachment."});
    let file_sent = call("cccc_file", args.clone()).await.expect("file send");
    assert_eq!(file_sent["structuredContent"]["accepted"], true);
    assert!(
        file_sent["structuredContent"]["result"]["delivery_id"]
            .as_str()
            .is_some()
    );
    assert_eq!(
        file_sent["structuredContent"]["result"]["source_event"]["data"]["attachments"][0]["bytes"],
        12
    );
    std::fs::remove_file(file).expect("remove original");
    let file_retry = call("cccc_file", args)
        .await
        .expect("file retry after original removed");
    assert_eq!(
        file_retry["structuredContent"]["result"]["delivery_id"],
        file_sent["structuredContent"]["result"]["delivery_id"]
    );
    assert_eq!(
        cccc_core::connect_delivery::pending_ids(&home)
            .expect("queue")
            .len(),
        2
    );
    server.abort();
}
