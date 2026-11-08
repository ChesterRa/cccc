//! cccc_file_send with several paths and retry keys, through real daemon IPC.
use cccc_client::DaemonClient;
use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout, actors};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    client: DaemonClient,
    root: PathBuf,
    outside: PathBuf,
    group_id: String,
    destination_id: String,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let root = temp.path().join("project");
        std::fs::create_dir_all(root.join("shots")).expect("project");
        for name in ["desktop", "phone", "tablet"] {
            std::fs::write(root.join(format!("shots/{name}.png")), name).expect("file");
        }
        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, b"outside").expect("file");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Files", "").expect("group");
        actors::add(&mut group, Actor::new("worker")).expect("worker");
        actors::add(&mut group, Actor::new("lead")).expect("lead");
        group.scopes.push(cccc_core::group::Scope {
            scope_key: "project".into(),
            url: root.to_string_lossy().into(),
            label: String::new(),
            git_remote: String::new(),
        });
        group.active_scope_key = "project".into();
        group.state = cccc_contracts::GroupState::Paused;
        store.save(&group).expect("group");
        let mut destination = store.create("Destination", "").expect("destination");
        actors::add(&mut destination, Actor::new("reviewer")).expect("reviewer");
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
        Self {
            _temp: temp,
            home,
            client,
            root,
            outside,
            group_id: group.group_id,
            destination_id: destination.group_id,
            server,
        }
    }

    async fn send(&self, extra: Value) -> Result<Value, crate::ToolCallError> {
        let mut arguments = json!({
            "to":["lead"],"mode":"send","text":"Screens",
            "insight":"The same screen at each size."
        })
        .as_object()
        .expect("args")
        .clone();
        arguments.extend(extra.as_object().expect("extra").clone());
        self.call(Value::Object(arguments)).await
    }

    /// The tool called by `worker` with exactly these arguments.
    async fn call(&self, arguments: Value) -> Result<Value, crate::ToolCallError> {
        crate::router::call_with_context(
            &self.home,
            &self.client,
            "cccc_file_send",
            arguments.as_object().expect("args").clone(),
            Some(crate::RequestContext {
                native_binding: None,
                binding: None,
                group_id: &self.group_id,
                actor_id: "worker",
            }),
            false,
        )
        .await
    }

    fn blobs(&self, group_id: &str) -> usize {
        let state = GroupStore::new(self.home.clone())
            .expect("store")
            .state_dir(group_id)
            .expect("state");
        std::fs::read_dir(state.join("blobs")).map_or(0, Iterator::count)
    }

    fn messages(&self, group_id: &str) -> usize {
        let ledger = GroupStore::new(self.home.clone())
            .expect("store")
            .ledger_path(group_id)
            .expect("ledger");
        std::fs::read_to_string(ledger)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("\"chat.message\""))
            .count()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn shot(root: &Path, name: &str) -> PathBuf {
    root.join(format!("shots/{name}.png"))
}

#[tokio::test]
async fn file_send_carries_several_files_in_one_message_after_checking_all() {
    let fixture = Fixture::new().await;
    let group = fixture.group_id.clone();
    let sent = fixture
        .send(json!({"path":["shots/desktop.png", shot(&fixture.root, "phone")]}))
        .await
        .expect("two files");
    let receipt = &sent["structuredContent"];
    assert_eq!(receipt["attachments"].as_array().map(Vec::len), Some(2));
    assert!(receipt.get("attachment").is_none());
    let attachments = &receipt["result"]["event"]["data"]["attachments"];
    assert_eq!(attachments.as_array().map(Vec::len), Some(2));
    assert_eq!(
        fixture.messages(&group),
        1,
        "one message carries both files"
    );

    // A file outside the workspace, a blank entry or too many files send nothing.
    for path in [
        json!(["shots/desktop.png", fixture.outside]),
        json!(["shots/desktop.png", "../outside.txt"]),
        json!(["shots/desktop.png", " "]),
        json!(["shots/desktop.png", "shots/missing.png"]),
        json!(vec!["shots/desktop.png"; 11]),
        json!([]),
    ] {
        assert!(fixture.send(json!({"path":path})).await.is_err(), "{path}");
        assert_eq!(fixture.messages(&group), 1, "{path}");
    }

    // The receipt follows the input's shape, not the number of files.
    let listed = fixture
        .send(json!({"path":["shots/phone.png"]}))
        .await
        .expect("one listed file");
    assert_eq!(
        listed["structuredContent"]["attachments"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let single = fixture
        .send(json!({"path":"shots/phone.png"}))
        .await
        .expect("one file");
    assert_eq!(single["structuredContent"]["attachment"]["kind"], "image");
    assert_eq!(fixture.messages(&group), 3);
}

#[tokio::test]
async fn a_file_without_a_mode_is_sent_to_the_user_and_mailed_to_agents() {
    let fixture = Fixture::new().await;
    let group = fixture.group_id.clone();
    let file = |path: &str, to: Value| json!({"path":path,"to":to,"text":"Screens","insight":"The default mode follows the recipient."});
    let mode = |sent: &Value| {
        let result = &sent["structuredContent"]["result"];
        result
            .get("dst_event")
            .filter(|event| !event.is_null())
            .unwrap_or(&result["event"])["data"]["message_mode"]
            .clone()
    };
    // The user has no Inbox: without a mode, a file for the user is sent, in this Group or another.
    for to in [json!(["user"]), json!("@user")] {
        let sent = fixture.call(file("shots/phone.png", to.clone())).await;
        assert_eq!(mode(&sent.expect("file for the user")), "send", "{to}");
    }
    let mut elsewhere = file("shots/phone.png", json!(["user"]));
    elsewhere["dst_group_id"] = json!(fixture.destination_id);
    let sent = fixture
        .call(elsewhere)
        .await
        .expect("file for the user elsewhere");
    assert_eq!(mode(&sent), "send");
    // A managed member's file for agents keeps Mail as the default.
    let sent = fixture.call(file("shots/phone.png", json!(["lead"]))).await;
    assert_eq!(mode(&sent.expect("file for an agent")), "mail");

    // Refused calls send and store nothing; the file is one not stored yet.
    let (messages, blobs) = (fixture.messages(&group), fixture.blobs(&group));
    let mut mail = file("shots/tablet.png", json!(["user"]));
    mail["mode"] = json!("mail");
    let mut null_mode = file("shots/tablet.png", json!(["lead"]));
    null_mode["mode"] = Value::Null;
    let mut paths = file("shots/tablet.png", json!(["user"]));
    paths["paths"] = json!(["shots/desktop.png"]);
    // A malformed list is refused even beside an explicit mode.
    let mut named_send = file("shots/tablet.png", json!(["lead", 42]));
    named_send["mode"] = json!("send");
    for (arguments, code) in [
        (mail, "mail_requires_actor_recipient"),
        (
            file("shots/tablet.png", json!(["user", 42])),
            "invalid_recipient",
        ),
        (named_send, "invalid_recipient"),
        (null_mode, "invalid_args"),
        (paths, "invalid_args"),
    ] {
        let error = fixture.call(arguments.clone()).await.expect_err("refused");
        assert_eq!(error.error_value()["code"], code, "{arguments}");
    }
    assert_eq!(fixture.messages(&group), messages);
    assert_eq!(fixture.blobs(&group), blobs);
}

#[tokio::test]
async fn files_sent_as_mail_say_which_recipients_cannot_act_on_them_yet() {
    let fixture = Fixture::new().await;
    let sent = fixture
        .send(json!({"path":"shots/phone.png","mode":"mail"}))
        .await
        .expect("mail with a file");
    let receipt = &sent["structuredContent"];
    assert_eq!(receipt["attachment"]["kind"], "image");
    assert_eq!(receipt["result"]["message_mode"], "mail");
    assert_eq!(
        receipt["result"]["mail_recipient_advisories"],
        json!([{"actor_id":"lead","reasons":["group_paused"]}])
    );
    assert!(
        receipt["result"]["mail_advisory"]
            .as_str()
            .is_some_and(|note| note.contains("Writing Mail does not start an Actor"))
    );
    // Some clients read the text form of the result.
    assert!(
        sent["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("mail_recipient_advisories"))
    );
}

#[tokio::test]
async fn a_file_send_retry_key_returns_the_first_message_even_after_a_source_is_removed() {
    let fixture = Fixture::new().await;
    for (destination, key) in [
        (None, "local-screens"),
        (Some(fixture.destination_id.clone()), "cross-group-screens"),
    ] {
        let name = if destination.is_some() {
            "tablet"
        } else {
            "desktop"
        };
        let mut extra = json!({
            "path":[format!("shots/{name}.png"), "shots/phone.png"],
            "idempotency_key":key
        });
        let counted = match &destination {
            Some(group_id) => {
                extra["dst_group_id"] = json!(group_id);
                extra["to"] = json!(["reviewer"]);
                group_id.clone()
            }
            None => fixture.group_id.clone(),
        };
        let before = fixture.messages(&counted);
        let first = fixture.send(extra.clone()).await.expect("first send");
        assert_eq!(fixture.messages(&counted), before + 1, "{key}");
        std::fs::remove_file(shot(&fixture.root, name)).expect("remove a source");
        let retry = fixture.send(extra).await.expect("retry after removal");
        assert_eq!(fixture.messages(&counted), before + 1, "{key}: one message");
        let event = |value: &Value| {
            let result = &value["structuredContent"]["result"];
            result
                .get("dst_event")
                .filter(|event| !event.is_null())
                .unwrap_or(&result["event"])["id"]
                .clone()
        };
        assert_eq!(event(&retry), event(&first), "{key}: the same message");
    }
}
