use cccc_core::{GroupStore, HomeLayout, access_tokens::AccessTokenStore, ledger};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn receive(socket: &mut Socket, matches: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let message = socket.next().await.expect("socket open").expect("frame");
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).expect("JSON frame");
                if matches(&value) {
                    return value;
                }
            }
        }
    })
    .await
    .expect("expected event")
}
async fn subscribe(socket: &mut Socket, id: u64, channel: &str, group: &str, cursor: &str) {
    socket.send(Message::Text(json!({"type":"subscribe","id":id,"channel":channel,"group_id":group,"cursor":cursor,"replay":true}).to_string().into())).await.expect("subscribe");
}
fn append(store: &GroupStore, group: &str) -> String {
    let event = cccc_contracts::Event::new("chat.message", group);
    ledger::append(&store.ledger_path(group).expect("ledger path"), &event).expect("append event");
    event.id
}
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn connect(endpoint: &str, token: &str) -> Socket {
    let mut request = endpoint.into_client_request().expect("request");
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("header"),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .expect("connect")
        .0
}

#[tokio::test]
async fn multiplexed_socket_preserves_replay_snapshots_group_scope_and_revocation() {
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("layout");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let a = store.create("A", "").expect("A").group_id;
    let b = store.create("B", "").expect("B").group_id;
    let denied = store.create("Denied", "").expect("denied").group_id;
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    tokens
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    let token = tokens
        .create("viewer", vec![a.clone(), b.clone()], false, None)
        .expect("viewer");
    let cursor = append(&store, &a);
    let replay = append(&store, &a);
    let path = store
        .state_dir(&a)
        .expect("state")
        .join("headless/events.jsonl");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("headless dir");
    std::fs::write(
        &path,
        "{\"id\":\"old\",\"actor_id\":\"a\",\"type\":\"headless.turn.started\"}\n",
    )
    .expect("snapshot");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!(
        "ws://{}/api/v1/events/ws",
        listener.local_addr().expect("address")
    );
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, cccc_web::app(home))
            .await
            .expect("serve");
    }));
    let mut socket = connect(&endpoint, &token.token).await;
    subscribe(&mut socket, 1, "global", "", "").await;
    receive(&mut socket, |p| p["type"] == "ready" && p["id"] == 1).await;
    subscribe(&mut socket, 2, "ledger", &a, &cursor).await;
    let event = receive(&mut socket, |p| p["message"]["event"] == "ledger").await;
    assert_eq!(event["message"]["id"], replay);
    subscribe(&mut socket, 3, "headless", &a, "").await;
    let snapshot = receive(&mut socket, |p| {
        p["message"]["event"] == "headless.snapshot"
    })
    .await;
    assert_eq!(snapshot["message"]["data"]["events"][0]["id"], "old");
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("tail"),
        "{{\"id\":\"new\",\"actor_id\":\"a\",\"type\":\"headless.message.delta\"}}"
    )
    .expect("delta");
    assert_eq!(
        receive(&mut socket, |p| p["message"]["event"] == "headless").await["message"]["data"]["id"],
        "new"
    );
    subscribe(&mut socket, 4, "ledger", &b, "").await;
    receive(&mut socket, |p| p["type"] == "ready" && p["id"] == 4).await;
    append(&store, &a);
    let latest = append(&store, &b);
    let one = receive(&mut socket, |p| p["message"]["id"] == latest).await;
    let two = receive(&mut socket, |p| p["message"]["id"] == latest).await;
    let next = [&one, &two]
        .into_iter()
        .find(|p| p["channel"] == "ledger")
        .expect("ledger event");
    assert_eq!(next["id"], 4);
    let global = [&one, &two]
        .into_iter()
        .find(|p| p["channel"] == "global")
        .expect("global event");
    assert!(
        global["message"]["data"].get("data").is_none(),
        "global stream exposes metadata only"
    );
    socket.close(None).await.expect("close");
    let missed = append(&store, &b);
    let mut socket = connect(&endpoint, &token.token).await;
    subscribe(&mut socket, 5, "ledger", &b, &latest).await;
    assert_eq!(
        receive(&mut socket, |p| p["message"]["event"] == "ledger").await["message"]["id"],
        missed
    );
    subscribe(&mut socket, 6, "headless", &denied, "").await;
    assert_eq!(
        receive(&mut socket, |p| p["id"] == 6).await["code"],
        "permission_denied"
    );
    let mut invalid_frame = connect(
        &format!("{endpoint}?connect_frame=unverified-frame"),
        &token.token,
    )
    .await;
    assert_eq!(
        receive(&mut invalid_frame, |p| p["type"] == "fatal").await["code"],
        "auth_required"
    );
    tokens.delete(&token.token_id()).expect("revoke");
    let revoked = receive(&mut socket, |p| {
        p["type"] == "fatal" || p["message"]["event"] == "error"
    })
    .await;
    assert!(
        revoked["code"] == "auth_required"
            || revoked["message"]["data"]["error"]["code"] == "auth_required"
    );
}
