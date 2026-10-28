use super::*;
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, local_session::LocalSessionRuntime};
use cccc_core::{
    GroupStore, actors,
    local_sessions::{self as native, NativeHost, NativeSession},
};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    client: DaemonClient,
    context: Context,
    source_id: String,
    daemon: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        std::fs::create_dir(temp.path().join("provider")).expect("isolated provider");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let host = NativeHost::capture(std::process::id()).expect("host");
        let source_id = format!("claude:{}", host.key());
        native::write(
            &home,
            NativeSession {
                source_id: source_id.clone(),
                runtime: LocalSessionRuntime::Claude,
                native_session_id: "current-conversation".into(),
                host,
                cwd: temp.path().to_string_lossy().into_owned(),
                title: "Isolated native fixture".into(),
                origin: None,
                revision: "current-revision".into(),
                codex_home: None,
                codex_can_receive: false,
                codex_program: None,
                claude_config_dir: Some(temp.path().join("provider")),
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            },
        )
        .expect("lifecycle registration");
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        let client = DaemonClient::new(home.clone());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if client
                    .call(&DaemonRequest {
                        v: 1,
                        op: "ping".into(),
                        args: Map::new(),
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
        .expect("daemon ready");
        let attached = client
            .call(&DaemonRequest {
                v: 1,
                op: "local_session_receiver_attach".into(),
                args: json!({"source_id":source_id,"connection_id":"fixture-receiver"})
                    .as_object()
                    .expect("args")
                    .clone(),
            })
            .await
            .expect("receiver registration");
        assert!(attached.ok);
        Self {
            _temp: temp,
            home,
            client,
            context: Context::new("claude", Some(std::process::id())).expect("MCP identity"),
            source_id,
            daemon,
        }
    }

    async fn call(&mut self, name: &str, args: Value) -> Value {
        self.context
            .handle(
                &self.home,
                &self.client,
                &json!({"id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
            )
            .await
    }

    async fn stop(self) {
        self.client
            .call(&DaemonRequest {
                v: 1,
                op: "shutdown".into(),
                args: Map::new(),
            })
            .await
            .expect("shutdown");
        tokio::time::timeout(std::time::Duration::from_secs(5), self.daemon)
            .await
            .expect("exit deadline")
            .expect("task")
            .expect("daemon exit");
    }
}

#[tokio::test]
async fn conversation_creation_send_and_leave_keep_their_distinct_effects() {
    let mut fixture = Fixture::new().await;
    let created = fixture
        .call(
            "cccc_session",
            json!({"action":"conversation","source_ids":[],"title":"Explicit collaboration"}),
        )
        .await;
    let group_id = created["result"]["structuredContent"]["group_id"]
        .as_str()
        .expect("Group created")
        .to_owned();
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    let path = groups.ledger_path(&group_id).expect("ledger");
    let created_messages = cccc_core::ledger::read_all(&path)
        .expect("creation events")
        .into_iter()
        .filter(|e| e.kind == "chat.message")
        .count();
    let rejected=fixture.call("cccc_session",json!({"action":"leave","group_id":group_id,"text":"This must not silently leave the Group"})).await;
    let enabled_after_rejection = groups.load(&group_id).expect("Group").actors[0].enabled;
    let sent=fixture.call("cccc_session",json!({"action":"send","group_id":group_id,"to":["user"],"mode":"send","text":"Explicitly sent message","client_id":"positive-message"})).await;
    let left = fixture
        .call(
            "cccc_session",
            json!({"action":"leave","group_id":group_id}),
        )
        .await;
    let enabled_after_leave = groups.load(&group_id).expect("Group").actors[0].enabled;
    let messages = cccc_core::ledger::read_all(&path)
        .expect("messages")
        .into_iter()
        .filter(|e| e.kind == "chat.message")
        .collect::<Vec<_>>();
    fixture.stop().await;
    assert_eq!(created_messages, 0);
    assert_eq!(rejected["result"]["isError"], true);
    assert!(enabled_after_rejection);
    assert_ne!(sent["result"]["isError"], true, "{sent}");
    assert_ne!(left["result"]["isError"], true, "{left}");
    assert!(!enabled_after_leave);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data["text"], "Explicitly sent message");
}

#[tokio::test]
async fn session_actions_reject_unused_parameters_before_any_group_changes() {
    let mut fixture = Fixture::new().await;
    let mut responses = Vec::new();
    for (action, field, value) in [
        (
            "conversation",
            "text",
            json!("This must not be silently lost"),
        ),
        ("conversation", "mode", json!("request_reply")),
        ("conversation", "message_mode", json!("send")),
        ("conversation", "insight", json!("Please read my request")),
        ("conversation", "to", json!(["peer"])),
        ("conversation", "client_id", json!("ignored-key")),
        ("list", "text", json!("Not a message")),
        ("list", "source_ids", json!(["claude:another-source"])),
    ] {
        let mut args = json!({"action":action,"source_ids":[]});
        if action == "list" {
            args = json!({"action":action});
        }
        args[field] = value;
        responses.push((action, field, fixture.call("cccc_session", args).await));
    }
    for action in [json!(null), json!(7), json!("unsupported")] {
        responses.push((
            "invalid-action",
            "action",
            fixture.call("cccc_session", json!({"action":action})).await,
        ));
    }
    let group_count = GroupStore::new(fixture.home.clone())
        .expect("groups")
        .list()
        .expect("Group list")
        .len();
    fixture.stop().await;
    for (action, field, response) in responses {
        assert_eq!(
            response["result"]["isError"], true,
            "{action} {field}: {response}"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"], "invalid_args",
            "{action} {field}: {response}"
        );
        if action == "conversation" {
            assert!(
                response["result"].to_string().contains("send"),
                "{response}"
            );
        }
    }
    assert_eq!(group_count, 0, "rejected input must not create a Group");
}

#[tokio::test]
async fn unset_optional_fields_and_default_mode_do_not_change_action_semantics() {
    let mut fixture = Fixture::new().await;
    let listed = fixture
        .call(
            "cccc_session",
            json!({"action":"list","mode":"send","text":null,"to":[],"title":""}),
        )
        .await;
    let created=fixture.call("cccc_session",json!({"action":"conversation","source_ids":[],"mode":"send","title":"Default-safe Group","text":"","insight":null,"to":[],"client_id":""})).await;
    let group_id = created["result"]["structuredContent"]["group_id"]
        .as_str()
        .expect("Group")
        .to_owned();
    let left = fixture
        .call(
            "cccc_session",
            json!({"action":"leave","group_id":group_id,"mode":"send","text":"","source_ids":[]}),
        )
        .await;
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    let events = cccc_core::ledger::read_all(&store.ledger_path(&group_id).expect("ledger"))
        .expect("events");
    fixture.stop().await;
    for response in [listed, created, left] {
        assert_ne!(response["result"]["isError"], true, "{response}");
    }
    assert!(!events.iter().any(|event| event.kind == "chat.message"));
}

#[tokio::test]
async fn native_membership_errors_identify_paused_access_and_the_original_group() {
    let mut fixture = Fixture::new().await;
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    let mut paused = groups.create("Paused access", "").expect("Group");
    let mut actor = Actor::new("original-member");
    actor.runtime = ActorRuntime::Claude;
    actor.external_session_id.clone_from(&fixture.source_id);
    actor.enabled = false;
    actors::add(&mut paused, actor).expect("member");
    groups.save(&paused).expect("Group");
    let missing = groups
        .create("Original group without this source", "")
        .expect("Group");
    let paused_response = fixture
        .call("cccc_bootstrap", json!({"group_id":paused.group_id}))
        .await;
    let missing_response = fixture
        .call("cccc_bootstrap", json!({"group_id":missing.group_id}))
        .await;
    let source_id = fixture.source_id.clone();
    fixture.stop().await;
    assert_eq!(
        paused_response["result"]["structuredContent"]["error"]["code"],
        "native_membership_paused"
    );
    assert_eq!(
        missing_response["result"]["structuredContent"]["error"]["code"],
        "native_membership_missing"
    );
    for (response, id) in [
        (&paused_response, &paused.group_id),
        (&missing_response, &missing.group_id),
    ] {
        let message = response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .expect("actionable error");
        assert!(
            message.contains(&source_id) && message.contains(id),
            "{message}"
        );
    }
    assert!(
        paused_response["result"]
            .to_string()
            .contains("Resume CCCC access")
    );
    assert!(
        missing_response["result"]
            .to_string()
            .contains("Connect existing sessions")
    );
}

#[tokio::test]
async fn missing_membership_explains_a_duplicate_live_conversation_owner() {
    let mut fixture = Fixture::new().await;
    let mut previous = native::read(&fixture.home, &fixture.source_id).expect("registration");
    previous.host = NativeHost::parent(std::process::id()).expect("live fixture parent");
    previous.source_id = format!("claude:{}", previous.host.key());
    native::write(&fixture.home, previous.clone()).expect("other live conversation host");
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    let mut group = store.create("Original collaboration", "").expect("Group");
    let mut actor = Actor::new("original-member");
    actor.runtime = ActorRuntime::Claude;
    actor.external_session_id.clone_from(&previous.source_id);
    actors::add(&mut group, actor).expect("member");
    store.save(&group).expect("Group");
    let response = fixture
        .call("cccc_bootstrap", json!({"group_id":group.group_id}))
        .await;
    let unchanged = store.load(&group.group_id).expect("unchanged Group").actors[0]
        .external_session_id
        == previous.source_id;
    fixture.stop().await;
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "native_membership_missing"
    );
    let message = response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .expect("error");
    assert!(
        message.contains("still running") && message.contains(&previous.host.pid.to_string()),
        "{message}"
    );
    assert!(unchanged);
}
