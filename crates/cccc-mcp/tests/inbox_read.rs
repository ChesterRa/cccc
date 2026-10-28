use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

// A daemon's shutdown retires its process-global runtime owners. Keep all
// in-process daemon fixtures in this binary disjoint even under parallel tests.
async fn daemon_fixture_guard() -> tokio::sync::OwnedMutexGuard<()> {
    static GUARD: std::sync::OnceLock<std::sync::Arc<tokio::sync::Mutex<()>>> =
        std::sync::OnceLock::new();
    std::sync::Arc::clone(GUARD.get_or_init(|| std::sync::Arc::new(tokio::sync::Mutex::new(()))))
        .lock_owned()
        .await
}

#[tokio::test]
async fn core_inbox_read_consumes_native_daemon_batches() {
    let _guard = daemon_fixture_guard().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("inbox read", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("actor");
    store.save(&group).expect("save group");

    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;

    let first_event_id = send_mail(&client, &group.group_id, "first").await;
    let second_event_id = send_mail(&client, &group.group_id, "second").await;
    let direct_event_id = send_message(&client, &group.group_id, "direct", "send").await;

    let first = mcp_call(
        &home,
        &group.group_id,
        1,
        "cccc_inbox_read",
        json!({"limit":1}),
    )
    .await;
    assert!(first.get("error").is_none(), "first read failed: {first}");
    assert_eq!(
        first["result"]["structuredContent"]["messages"][0]["id"],
        first_event_id
    );
    assert_eq!(
        first["result"]["structuredContent"]["cursor"]["event_id"],
        first_event_id
    );
    assert_eq!(
        first["result"]["structuredContent"]["event"]["kind"],
        "mail.read"
    );

    let second = mcp_call(
        &home,
        &group.group_id,
        2,
        "cccc_inbox_read",
        json!({"limit":50}),
    )
    .await;
    assert_eq!(
        second["result"]["structuredContent"]["messages"][0]["id"],
        second_event_id
    );
    assert_eq!(
        second["result"]["structuredContent"]["cursor"]["event_id"],
        second_event_id
    );

    let empty = mcp_call(&home, &group.group_id, 3, "cccc_inbox_read", json!({})).await;
    assert_eq!(empty["result"]["structuredContent"]["messages"], json!([]));
    assert_eq!(empty["result"]["structuredContent"]["event"], Value::Null);

    let history = mcp_call(
        &home,
        &group.group_id,
        4,
        "cccc_message_history",
        json!({"mode":"send"}),
    )
    .await;
    assert_eq!(
        history["result"]["structuredContent"]["messages"][0]["id"],
        direct_event_id
    );
    assert_eq!(
        history["result"]["structuredContent"]["messages"][0]["data"]["text"],
        "direct"
    );

    let exact = mcp_call(
        &home,
        &group.group_id,
        5,
        "cccc_message_history",
        json!({"event_id":direct_event_id}),
    )
    .await;
    let exact = &exact["result"]["structuredContent"];
    assert_eq!(exact["message"]["id"], direct_event_id);
    assert!(exact.get("messages").is_none());
    assert!(exact.get("has_more").is_none());
    assert!(exact["message"]["data"].get("text").is_none());
    assert!(
        exact["delivery_text"]
            .as_str()
            .expect("full delivery")
            .contains("direct")
    );

    daemon_task.abort();
    let _ = daemon_task.await;
}

#[tokio::test]
async fn exact_history_read_returns_the_delivered_text_once_without_dropping_metadata() {
    let _guard = daemon_fixture_guard().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("exact history", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("actor");
    store.save(&group).expect("save group");
    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;

    let original = "An original request that is longer than the quote shown in the delivery header, \
                    so the stored reply quote keeps more of it than the rendered one.";
    let body = "Line one of the reply.\nLine two of the reply.";
    let insight = "The stored quote is longer than the rendered one.";
    let original_id = send_message(&client, &group.group_id, original, "send").await;
    let reply = daemon_call(
        &client,
        "reply",
        json!({
            "group_id":group.group_id,"by":"user","reply_to":original_id,"to":["peer1"],
            "text":body,"insight":insight,
        }),
    )
    .await;
    let reply_id = reply["event"]["id"].as_str().expect("reply id");
    let quote = reply["event"]["data"]["quote_text"]
        .as_str()
        .expect("stored quote");

    let exact = mcp_call(
        &home,
        &group.group_id,
        1,
        "cccc_message_history",
        json!({"event_id":reply_id}),
    )
    .await;
    let exact = &exact["result"]["structuredContent"];
    let delivery_text = exact["delivery_text"].as_str().expect("delivery text");
    assert!(delivery_text.contains(body) && delivery_text.contains(insight));
    assert!(
        !delivery_text.contains(quote),
        "the header abbreviates the quote"
    );
    let data = &exact["message"]["data"];
    assert!(data.get("text").is_none() && data.get("insight").is_none());
    assert_eq!(data["quote_text"], quote);
    assert_eq!(data["reply_to"], original_id);
    assert_eq!(data["to"], json!(["peer1"]));
    assert_eq!(exact["message"]["kind"], "chat.message");
    assert_eq!(exact["message"]["ts"], reply["event"]["ts"]);
    let payload = serde_json::to_string(exact).expect("payload");
    for value in [body, insight] {
        let encoded = serde_json::to_string(value).expect("encoded");
        assert_eq!(payload.matches(encoded.trim_matches('"')).count(), 1);
    }

    // The /install delivery shows capability instructions, not the typed text.
    let install_id = send_message(&client, &group.group_id, "/install owner/repo", "mail").await;
    let install = mcp_call(
        &home,
        &group.group_id,
        2,
        "cccc_message_history",
        json!({"event_id":install_id}),
    )
    .await;
    let install = &install["result"]["structuredContent"];
    assert!(
        install["delivery_text"]
            .as_str()
            .expect("install delivery")
            .contains("[cccc] Slash command: /install")
    );
    assert_eq!(install["message"]["data"]["text"], "/install owner/repo");

    // Reading the exact Mail item leaves it unread.
    let inbox = mcp_call(&home, &group.group_id, 3, "cccc_inbox_read", json!({})).await;
    assert_eq!(
        inbox["result"]["structuredContent"]["messages"][0]["id"],
        install_id
    );

    let unseen = mcp_call(
        &home,
        &group.group_id,
        4,
        "cccc_message_history",
        json!({"event_id":"missing"}),
    )
    .await;
    assert!(
        unseen.get("error").is_some() || unseen["result"]["isError"] == json!(true),
        "unknown events stay unavailable: {unseen}"
    );

    daemon_task.abort();
    let _ = daemon_task.await;
}

async fn daemon_call(client: &DaemonClient, op: &str, args: Value) -> Value {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().expect("daemon args"),
        })
        .await
        .expect("daemon call");
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

#[tokio::test]
async fn file_send_uses_daemon_owned_preflight_before_blob_storage() {
    let _guard = daemon_fixture_guard().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let scope_root = temp.path().join("scope");
    std::fs::create_dir_all(&scope_root).expect("scope");
    std::fs::write(scope_root.join("note.txt"), "deliverable").expect("file");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("file send", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("actor");
    store.save(&group).expect("save group");
    cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::Scope {
            scope_key: "scope_project".into(),
            url: scope_root.to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach scope");

    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;
    let blobs_dir = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("blobs");

    let rejected = mcp_call(
        &home,
        &group.group_id,
        10,
        "cccc_file_send",
        json!({"path":"note.txt","to":["user"],"mode":"mail"}),
    )
    .await;
    assert_eq!(rejected["result"]["isError"], true, "{rejected}");
    assert!(
        !blobs_dir.exists()
            || std::fs::read_dir(&blobs_dir)
                .expect("blobs")
                .next()
                .is_none()
    );

    let sent = mcp_call(
        &home,
        &group.group_id,
        11,
        "cccc_file_send",
        json!({
            "path":"note.txt","text":"attached",
            "to":["user"],"mode":"send"
        }),
    )
    .await;
    assert!(sent.get("error").is_none(), "send failed: {sent}");
    assert_eq!(sent["result"]["structuredContent"]["sent"], true);
    assert_eq!(
        sent["result"]["structuredContent"]["result"]["event"]["data"]["message_mode"],
        "send"
    );
    assert_eq!(
        std::fs::read_dir(&blobs_dir)
            .expect("blobs")
            .filter_map(Result::ok)
            .count(),
        1
    );

    daemon_task.abort();
    let _ = daemon_task.await;
}

async fn wait_for_daemon(client: &DaemonClient) {
    for _ in 0..100 {
        if client
            .call(&DaemonRequest {
                v: 1,
                op: "group_list".into(),
                args: Map::new(),
            })
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("daemon did not start");
}

async fn send_mail(client: &DaemonClient, group_id: &str, text: &str) -> String {
    send_message(client, group_id, text, "mail").await
}

async fn send_message(client: &DaemonClient, group_id: &str, text: &str, mode: &str) -> String {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: "send".into(),
            args: json!({
                "group_id":group_id,
                "by":"user",
                "to":["peer1"],
                "text":text,
                "message_mode":mode,
            })
            .as_object()
            .cloned()
            .expect("send args"),
        })
        .await
        .expect("send message");
    assert!(response.ok, "send message: {:?}", response.error);
    response.result["event"]["id"]
        .as_str()
        .expect("event id")
        .to_owned()
}

async fn mcp_call(
    home: &HomeLayout,
    group_id: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    cccc_mcp::handle_request_for_actor(
        home,
        &json!({
            "jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
        group_id,
        "peer1",
    )
    .await
}

struct ActorScopeFixture {
    _temp: tempfile::TempDir,
    _guard: tokio::sync::OwnedMutexGuard<()>,
    home: HomeLayout,
    store: GroupStore,
    group_id: String,
    roots: [std::path::PathBuf; 2],
    daemon: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl ActorScopeFixture {
    async fn new() -> Self {
        let guard = daemon_fixture_guard().await;
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let roots = [temp.path().join("A"), temp.path().join("B")];
        for (root, content) in roots.iter().zip(["A-only bytes", "B-only distinct bytes"]) {
            std::fs::create_dir(root).expect("scope directory");
            std::fs::write(root.join("same.txt"), content).expect("same-named file");
        }
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Actor tool scopes", "").expect("group");
        for (key, root) in ["scope_a", "scope_b"].into_iter().zip(&roots) {
            group.scopes.push(cccc_core::Scope {
                scope_key: key.into(),
                url: root.to_string_lossy().into_owned(),
                label: key.into(),
                git_remote: String::new(),
            });
        }
        group.active_scope_key = "scope_a".into();
        for (id, scope) in [("fixed-b", "scope_b"), ("follow", "")] {
            let mut actor = cccc_contracts::Actor::new(id);
            actor.default_scope_key = scope.into();
            cccc_core::actors::add(&mut group, actor).expect("actor");
        }
        store.save(&group).expect("save group");
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&DaemonClient::new(home.clone())).await;
        Self {
            _temp: temp,
            _guard: guard,
            home,
            store,
            group_id: group.group_id,
            roots,
            daemon,
        }
    }

    async fn call(&self, actor: Option<&str>, name: &str, mut arguments: Value) -> Value {
        // Actor identity comes from the public request context, never from cwd
        // or a model-controlled actor_id argument.
        let mut request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}});
        if let Some(actor) = actor {
            cccc_mcp::handle_request_for_actor(&self.home, &request, &self.group_id, actor).await
        } else {
            arguments["group_id"] = json!(self.group_id);
            request["params"]["arguments"] = arguments;
            cccc_mcp::handle_request(&self.home, &request).await
        }
    }

    async fn stop(mut self) {
        for actor in ["fixed-b", "follow"] {
            let _ = cccc_runtime::stop(&self.group_id, actor);
        }
        self.daemon.abort();
        let _ = (&mut self.daemon).await;
    }
}

impl Drop for ActorScopeFixture {
    fn drop(&mut self) {
        for actor in ["fixed-b", "follow"] {
            let _ = cccc_runtime::stop(&self.group_id, actor);
        }
        self.daemon.abort();
    }
}

#[tokio::test]
async fn actor_scope_roots_and_file_reads_follow_the_authenticated_actor() {
    let fixture = ActorScopeFixture::new().await;
    for active in ["scope_a", "scope_b", "scope_a"] {
        cccc_core::group_scope::activate(&fixture.store, &fixture.group_id, active)
            .expect("change Group active scope");
        for actor in [Some("fixed-b"), Some("follow"), Some("user"), None] {
            let index = usize::from(actor == Some("fixed-b") || active == "scope_b");
            let expected_root = &fixture.roots[index];
            let root = fixture.call(actor, "cccc_repo", json!({})).await;
            let read = fixture
                .call(
                    actor,
                    "cccc_file",
                    json!({"path":"same.txt","actor_id":"fixed-b","by":"user"}),
                )
                .await;
            assert_ne!(root["result"]["isError"], true, "{root}");
            assert_ne!(read["result"]["isError"], true, "{read}");
            assert_eq!(
                read["result"]["structuredContent"]["content"],
                std::fs::read_to_string(expected_root.join("same.txt")).expect("expected file"),
                "actor={actor:?}, Group active={active}: {read}"
            );
            assert_eq!(
                root["result"]["structuredContent"]["root"],
                expected_root.to_string_lossy().as_ref(),
                "actor={actor:?}, Group active={active}"
            );
        }
    }
    let edited = fixture
        .call(
            Some("fixed-b"),
            "cccc_repo_edit",
            json!({"action":"write","path":"created.txt","content":"B-only edit"}),
        )
        .await;
    assert_ne!(edited["result"]["isError"], true, "{edited}");
    assert_eq!(
        std::fs::read_to_string(fixture.roots[1].join("created.txt")).expect("B edit"),
        "B-only edit"
    );
    assert!(!fixture.roots[0].join("created.txt").exists());
    for path in [
        "../A/same.txt".to_owned(),
        fixture.roots[0]
            .join("same.txt")
            .to_string_lossy()
            .into_owned(),
    ] {
        let denied = fixture
            .call(Some("fixed-b"), "cccc_file", json!({"path":path}))
            .await;
        assert_eq!(denied["result"]["isError"], true, "{denied}");
    }
    fixture
        .store
        .mutate(&fixture.group_id, |group| {
            group
                .actors
                .iter_mut()
                .find(|actor| actor.id == "fixed-b")
                .expect("existing Actor")
                .default_scope_key
                .clear();
            Ok(())
        })
        .expect("switch the same Actor to follow Group active");
    let following = fixture
        .call(Some("fixed-b"), "cccc_file", json!({"path":"same.txt"}))
        .await;
    assert_eq!(
        following["result"]["structuredContent"]["content"], "A-only bytes",
        "an unchanged caller identity must pick up saved follow mode: {following}"
    );
    fixture.stop().await;
}

#[tokio::test]
async fn actor_scope_file_send_stores_bytes_from_the_effective_workspace() {
    let fixture = ActorScopeFixture::new().await;
    for active in ["scope_a", "scope_b", "scope_a"] {
        cccc_core::group_scope::activate(&fixture.store, &fixture.group_id, active)
            .expect("change Group active scope");
        for actor in ["fixed-b", "follow"] {
            let index = usize::from(actor == "fixed-b" || active == "scope_b");
            let sent = fixture
                .call(
                    Some(actor),
                    "cccc_file_send",
                    json!({"path":"same.txt","to":["user"],"mode":"send","by":"user"}),
                )
                .await;
            assert_ne!(sent["result"]["isError"], true, "{sent}");
            let result = &sent["result"]["structuredContent"];
            assert_eq!(result["sent"], true, "{sent}");
            let blob = cccc_core::blobs::resolve(
                &fixture.home,
                &fixture.group_id,
                result["attachment"]["path"]
                    .as_str()
                    .expect("sent attachment"),
            )
            .expect("stored attachment");
            assert_eq!(
                std::fs::read_to_string(blob).expect("actual attachment bytes"),
                std::fs::read_to_string(fixture.roots[index].join("same.txt"))
                    .expect("expected source bytes"),
                "actor={actor}, Group active={active}: {sent}"
            );
            assert_eq!(result["result"]["event"]["by"], actor);
            assert_eq!(
                fixture
                    .store
                    .load(&fixture.group_id)
                    .expect("Group")
                    .active_scope_key,
                active,
                "tool scope cannot change the Group active scope"
            );
        }
    }
    fixture.stop().await;
}

#[tokio::test]
async fn actor_scope_bootstrap_keeps_group_active_separate_from_effective_workspace() {
    let fixture = ActorScopeFixture::new().await;
    for active in ["scope_a", "scope_b", "scope_a"] {
        cccc_core::group_scope::activate(&fixture.store, &fixture.group_id, active)
            .expect("change Group active scope");
        for actor in [Some("fixed-b"), Some("follow")] {
            let response = fixture.call(actor, "cccc_bootstrap", json!({})).await;
            assert_ne!(response["result"]["isError"], true, "{response}");
            let session = &response["result"]["structuredContent"]["session"];
            let fixed = actor == Some("fixed-b");
            let index = usize::from(fixed || active == "scope_b");
            assert_eq!(session["active_scope"]["scope_key"], active);
            assert_eq!(
                session["effective_scope"],
                json!({
                    "scope_key": if index == 1 { "scope_b" } else { "scope_a" },
                    "path":fixture.roots[index],
                    "source":if fixed { "actor_default" } else { "group_active" },
                }),
                "actor={actor:?}, Group active={active}: {response}"
            );
        }
    }
    // Bootstrap includes an agent-only Inbox preview. Human/no-Actor fallback
    // belongs to the local workspace tools, not a new human Inbox authority.
    for actor in [Some("user"), None] {
        let denied = fixture.call(actor, "cccc_bootstrap", json!({})).await;
        assert_eq!(
            denied["result"]["structuredContent"]["error"]["code"], "invalid_inbox_recipient",
            "{denied}"
        );
    }
    fixture.stop().await;
}

#[cfg(unix)]
impl ActorScopeFixture {
    fn same_key_git_scopes(&self) -> [cccc_core::Scope; 2] {
        for root in &self.roots {
            for args in [
                vec!["init", "-q"],
                vec![
                    "remote",
                    "add",
                    "origin",
                    "https://fixture.invalid/scope-lifecycle.git",
                ],
            ] {
                let output = std::process::Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .output()
                    .expect("fixture Git");
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        let scopes = self
            .roots
            .each_ref()
            .map(|root| cccc_core::scope::detect(root).expect("Git scope identity"));
        assert_eq!(scopes[0].scope_key, scopes[1].scope_key);
        scopes
    }

    async fn ipc(&self, op: &str, mut args: Value) -> Value {
        args["group_id"] = json!(self.group_id);
        args["by"] = json!("user");
        let response = DaemonClient::new(self.home.clone())
            .call(&DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("IPC args").clone(),
            })
            .await
            .expect("real daemon IPC");
        assert!(response.ok, "{op}: {:?}", response.error);
        Value::Object(response.result)
    }

    async fn fixture_process(&self, actor: &str) {
        self.ipc(
            "actor_update",
            json!({"actor_id":actor,"patch":{
                "runtime":"custom","command":["sh","-c","exec sleep 120"],
                "env":{
                    "HOME":self._temp.path().join("provider-home"),
                    "CODEX_HOME":self._temp.path().join("provider-home/codex"),
                    "CLAUDE_CONFIG_DIR":self._temp.path().join("provider-home/claude"),
                    "CCCC_SCOPE_KEY":"scope_b",
                    "CCCC_ACTOR_GENERATION":"model-controlled-not-authority"
                }
            }}),
        )
        .await;
    }

    /// Separate process: it can learn a live workspace only through daemon IPC,
    /// never by reading the test process's in-memory runtime registry.
    async fn stdio_tool(&self, actor: &str, name: &str, arguments: Value) -> Value {
        self.stdio_tool_in(&self.group_id, actor, name, arguments)
            .await
    }

    async fn stdio_tool_in(
        &self,
        group_id: &str,
        actor: &str,
        name: &str,
        arguments: Value,
    ) -> Value {
        use std::process::Stdio;
        use tokio::io::AsyncWriteExt;
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc-mcp"))
            .current_dir(self._temp.path())
            .env("CCCC_HOME", self.home.root())
            .env("CCCC_GROUP_ID", group_id)
            .env("CCCC_ACTOR_ID", actor)
            .env("HOME", self._temp.path().join("mcp-home"))
            .env("CODEX_HOME", self._temp.path().join("mcp-home/codex"))
            .env(
                "CLAUDE_CONFIG_DIR",
                self._temp.path().join("mcp-home/claude"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("stdio MCP");
        let mut input = child.stdin.take().expect("MCP stdin");
        let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}});
        input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .expect("MCP request");
        input.shutdown().await.expect("MCP EOF");
        drop(input);
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(15), child.wait_with_output())
                .await
                .expect("bounded MCP call")
                .expect("MCP output");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("MCP JSON response")
    }

    async fn pid(&self, actor: &str) -> u32 {
        let listed = self.ipc("actor_list", json!({})).await;
        listed["actors"]
            .as_array()
            .expect("actors")
            .iter()
            .find(|item| item["id"] == actor)
            .expect("listed Actor")["pid"]
            .as_u64()
            .expect("running PID") as u32
    }

    async fn assert_workspace(&self, actor: &str, index: usize) {
        let root = self.stdio_tool(actor, "cccc_repo", json!({})).await;
        let read = self
            .stdio_tool(actor, "cccc_file", json!({"path":"same.txt"}))
            .await;
        let shell = self
            .stdio_tool(actor, "cccc_shell", json!({"command":["pwd"]}))
            .await;
        let bootstrap = self.stdio_tool(actor, "cccc_bootstrap", json!({})).await;
        let prompt = self.ipc("actor_prompt", json!({"actor_id":actor})).await;
        let sent = self
            .stdio_tool(
                actor,
                "cccc_file_send",
                json!({
                    "path":"same.txt","to":["user"],"mode":"send"
                }),
            )
            .await;
        println!(
            "workspace observations: {}",
            json!({"actor":actor,"expected":self.roots[index],
            "root":root["result"]["structuredContent"]["root"],
            "read":read["result"]["structuredContent"]["content"],
            "shell":shell["result"]["structuredContent"],
            "bootstrap_session":bootstrap["result"]["structuredContent"]["session"],
            "prompt_workspace":prompt["prompt"].as_str().and_then(|s|s.lines().find(|s|s.starts_with("workspace:"))),
            "attachment":sent["result"]["structuredContent"]["attachment"],
            "send_error":sent["result"]["structuredContent"]["error"]})
        );
        for response in [&root, &read, &shell, &bootstrap, &sent] {
            assert_ne!(response["result"]["isError"], true, "{response}");
        }
        assert_eq!(
            root["result"]["structuredContent"]["root"],
            json!(self.roots[index])
        );
        let expected =
            std::fs::read_to_string(self.roots[index].join("same.txt")).expect("source bytes");
        assert_eq!(read["result"]["structuredContent"]["content"], expected);
        assert_eq!(
            shell["result"]["structuredContent"]["stdout"],
            format!("{}\n", self.roots[index].display())
        );
        assert_eq!(
            bootstrap["result"]["structuredContent"]["session"]["effective_scope"]["path"],
            json!(self.roots[index])
        );
        assert!(
            prompt["prompt"]
                .as_str()
                .expect("prompt")
                .contains(&format!("workspace: {}\n", self.roots[index].display())),
            "{prompt}"
        );
        let attachment = &sent["result"]["structuredContent"]["attachment"];
        let blob = cccc_core::blobs::resolve(
            &self.home,
            &self.group_id,
            attachment["path"].as_str().expect("attachment path"),
        )
        .expect("actual blob");
        assert_eq!(
            std::fs::read_to_string(blob).expect("actual stored bytes"),
            expected
        );
        assert_eq!(
            sent["result"]["structuredContent"]["result"]["event"]["by"],
            actor
        );
        let edited = self.stdio_tool(actor, "cccc_repo_edit", json!({
            "action":"write","path":"scope-edit.txt","content":format!("edited in {}",self.roots[index].display())
        })).await;
        assert_ne!(edited["result"]["isError"], true, "{edited}");
        assert_eq!(
            std::fs::read_to_string(self.roots[index].join("scope-edit.txt"))
                .expect("edit at actual root"),
            format!("edited in {}", self.roots[index].display())
        );
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn running_scope_save_uses_launch_workspace_until_explicit_restart() {
    let fixture = ActorScopeFixture::new().await;
    fixture.fixture_process("follow").await;
    fixture
        .ipc("actor_start", json!({"actor_id":"follow"}))
        .await;
    let pid = fixture.pid("follow").await;
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).expect("physical cwd"),
        fixture.roots[0]
    );
    fixture.assert_workspace("follow", 0).await;
    fixture
        .ipc(
            "actor_update",
            json!({"actor_id":"follow","patch":{"default_scope_key":"scope_b"}}),
        )
        .await;
    assert_eq!(fixture.pid("follow").await, pid, "saving must not restart");
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).expect("physical cwd after save");
    println!(
        "save without restart: {}",
        json!({"pid":pid,"physical_cwd":cwd,"saved_scope":"scope_b"})
    );
    assert_eq!(cwd, fixture.roots[0]);
    fixture.assert_workspace("follow", 0).await;
    assert!(!fixture.roots[1].join("scope-edit.txt").exists());
    std::os::unix::fs::symlink(
        fixture.roots[1].join("same.txt"),
        fixture.roots[0].join("escape.txt"),
    )
    .expect("escape symlink");
    for path in ["../B/same.txt", "escape.txt"] {
        for name in ["cccc_file", "cccc_repo_edit", "cccc_file_send"] {
            let args = match name {
                "cccc_repo_edit" => {
                    json!({"path":path,"action":"write","content":"must not cross roots"})
                }
                "cccc_file_send" => json!({"path":path,"to":["user"],"mode":"send"}),
                _ => json!({"path":path}),
            };
            let denied = fixture.stdio_tool("follow", name, args).await;
            assert_eq!(denied["result"]["isError"], true, "{denied}");
        }
    }
    let denied = fixture
        .stdio_tool(
            "follow",
            "cccc_shell",
            json!({"command":["pwd"],"cwd":"../B"}),
        )
        .await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    assert_eq!(
        std::fs::read_to_string(fixture.roots[1].join("same.txt")).expect("B unchanged"),
        "B-only distinct bytes"
    );
    fixture
        .ipc("actor_restart", json!({"actor_id":"follow"}))
        .await;
    let next_pid = fixture.pid("follow").await;
    assert_ne!(pid, next_pid);
    assert_eq!(
        std::fs::read_link(format!("/proc/{next_pid}/cwd")).expect("restarted cwd"),
        fixture.roots[1]
    );
    fixture.assert_workspace("follow", 1).await;
    fixture
        .ipc("actor_stop", json!({"actor_id":"follow"}))
        .await;
    fixture
        .ipc(
            "actor_update",
            json!({"actor_id":"follow","patch":{"default_scope_key":"scope_a"}}),
        )
        .await;
    fixture.assert_workspace("follow", 0).await;
    fixture.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn scope_bootstrap_stdio_cannot_select_another_actor() {
    let fixture = ActorScopeFixture::new().await;
    let response = fixture
        .stdio_tool(
            "follow",
            "cccc_bootstrap",
            json!({"actor_id":"fixed-b","by":"user"}),
        )
        .await;
    println!(
        "forged bootstrap actor_id: {}",
        response["result"]["structuredContent"]["session"]
    );
    assert_ne!(response["result"]["isError"], true, "{response}");
    let session = &response["result"]["structuredContent"]["session"];
    assert_eq!(session["actor_id"], "follow");
    assert_eq!(session["effective_scope"]["path"], json!(fixture.roots[0]));
    fixture.stop().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn scope_same_key_rebind_preserves_live_root_and_attachment_admission() {
    let fixture = ActorScopeFixture::new().await;
    let [scope_a, scope_b] = fixture.same_key_git_scopes();
    fixture
        .store
        .mutate(&fixture.group_id, |group| {
            group.scopes = vec![scope_a.clone()];
            group.active_scope_key.clone_from(&scope_a.scope_key);
            Ok(())
        })
        .expect("same-key fixture");
    fixture.fixture_process("follow").await;
    fixture
        .ipc("actor_start", json!({"actor_id":"follow"}))
        .await;
    let pid = fixture.pid("follow").await;
    let refused = DaemonClient::new(fixture.home.clone()).call(&DaemonRequest {
        v:1,op:"attach".into(),args:json!({"group_id":fixture.group_id,"by":"user","path":fixture.roots[1],"activate":false}).as_object().expect("args").clone()
    }).await.expect("nonactivating attach");
    assert!(!refused.ok);
    assert_eq!(
        refused.error.expect("scope conflict").code,
        "scope_conflict"
    );
    assert_eq!(
        fixture
            .store
            .load(&fixture.group_id)
            .expect("unchanged Group")
            .scopes[0]
            .url,
        scope_a.url
    );
    // Ordinary attach omission and true retain their explicit rebind semantics.
    for activate in [None, Some(true)] {
        let mut args = json!({"path":fixture.roots[1]});
        if let Some(activate) = activate {
            args["activate"] = json!(activate);
        }
        fixture.ipc("attach", args).await;
        assert_eq!(fixture.pid("follow").await, pid);
        assert_eq!(
            std::fs::read_link(format!("/proc/{pid}/cwd")).expect("same physical root"),
            fixture.roots[0]
        );
        assert_eq!(
            fixture
                .store
                .load(&fixture.group_id)
                .expect("rebound Group")
                .scopes[0]
                .url,
            scope_b.url
        );
        fixture.assert_workspace("follow", 0).await;
    }
    fixture
        .ipc("actor_restart", json!({"actor_id":"follow"}))
        .await;
    let pid = fixture.pid("follow").await;
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).expect("new physical root"),
        fixture.roots[1]
    );
    fixture.assert_workspace("follow", 1).await;
    fixture.stop().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn scope_detach_preserves_running_workspace_and_unavailable_pin() {
    let fixture = ActorScopeFixture::new().await;
    fixture.fixture_process("fixed-b").await;
    fixture
        .ipc("actor_start", json!({"actor_id":"fixed-b"}))
        .await;
    let pid = fixture.pid("fixed-b").await;
    fixture
        .ipc("group_detach_scope", json!({"scope_key":"scope_b"}))
        .await;
    let mut group = fixture
        .store
        .load(&fixture.group_id)
        .expect("detached Group");
    assert_eq!(
        cccc_core::group_scope::normalize_actor_scope_keys(&mut group),
        0
    );
    assert_eq!(
        cccc_core::actors::find(&group, "fixed-b")
            .expect("Actor")
            .default_scope_key,
        "scope_b"
    );
    assert_eq!(fixture.pid("fixed-b").await, pid);
    fixture.assert_workspace("fixed-b", 1).await;
    fixture
        .ipc("actor_stop", json!({"actor_id":"fixed-b"}))
        .await;
    for name in ["cccc_repo", "cccc_file", "cccc_shell", "cccc_file_send"] {
        let response = fixture
            .stdio_tool(
                "fixed-b",
                name,
                json!({"path":"same.txt","command":["pwd"]}),
            )
            .await;
        assert_eq!(response["result"]["isError"], true, "{response}");
    }
    let bootstrap = fixture
        .stdio_tool("fixed-b", "cccc_bootstrap", json!({}))
        .await;
    assert_eq!(
        bootstrap["result"]["structuredContent"]["session"]["effective_scope"],
        json!({})
    );
    let prompt = fixture
        .ipc("actor_prompt", json!({"actor_id":"fixed-b"}))
        .await;
    assert!(
        prompt["prompt"]
            .as_str()
            .expect("prompt")
            .contains("effective_scope: unavailable")
    );
    assert!(
        !prompt["prompt"]
            .as_str()
            .expect("prompt")
            .contains("workspace: ")
    );
    fixture.assert_workspace("follow", 0).await;
    fixture.stop().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn scope_follow_group_changes_only_after_the_owner_exits() {
    let fixture = ActorScopeFixture::new().await;
    fixture.fixture_process("follow").await;
    fixture
        .ipc(
            "actor_update",
            json!({"actor_id":"follow","patch":{
                "command":["sh","-c","while [ ! -e exit.flag ]; do sleep 0.02; done"]
            }}),
        )
        .await;
    fixture
        .ipc("actor_start", json!({"actor_id":"follow"}))
        .await;
    let pid = fixture.pid("follow").await;
    cccc_core::group_scope::activate(&fixture.store, &fixture.group_id, "scope_b")
        .expect("change active configuration");
    assert_eq!(fixture.pid("follow").await, pid);
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).expect("live follow cwd"),
        fixture.roots[0]
    );
    fixture.assert_workspace("follow", 0).await;
    let exited = fixture
        .stdio_tool(
            "follow",
            "cccc_shell",
            json!({"command":["touch","exit.flag"]}),
        )
        .await;
    assert_ne!(exited["result"]["isError"], true, "{exited}");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while cccc_runtime::status(&fixture.group_id, "follow").is_ok_and(|status| status.running) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture exits normally");
    fixture.assert_workspace("follow", 1).await;
    let bootstrap = fixture
        .stdio_tool("follow", "cccc_bootstrap", json!({}))
        .await;
    assert_eq!(
        bootstrap["result"]["structuredContent"]["session"]["effective_scope"]["source"],
        "group_active"
    );
    fixture
        .ipc("actor_start", json!({"actor_id":"follow"}))
        .await;
    let pid = fixture.pid("follow").await;
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).expect("next launch follows B"),
        fixture.roots[1]
    );
    fixture.stop().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn scope_copy_import_reset_and_replacement_do_not_inherit_launch_authority() {
    let fixture = ActorScopeFixture::new().await;
    fixture.fixture_process("follow").await;
    fixture
        .ipc("actor_start", json!({"actor_id":"follow"}))
        .await;
    fixture
        .ipc(
            "actor_update",
            json!({"actor_id":"follow","patch":{"default_scope_key":"scope_b"}}),
        )
        .await;
    fixture.assert_workspace("follow", 0).await;
    let persisted = serde_json::to_value(
        fixture
            .store
            .load(&fixture.group_id)
            .expect("stored config"),
    )
    .expect("serialize");
    for actor in persisted["actors"].as_array().expect("Actor config") {
        for field in ["effective_scope", "cwd", "workspace"] {
            assert!(actor.get(field).is_none());
        }
    }
    assert!(persisted.get("effective_scope").is_none());
    let exported = fixture.ipc("group_copy_export", json!({})).await;
    let imported = fixture
        .ipc(
            "group_copy_import",
            json!({"package_b64":exported["package_b64"]}),
        )
        .await;
    let imported_id = imported["group_id"].as_str().expect("imported ID");
    assert_ne!(imported_id, fixture.group_id);
    let copy = fixture
        .stdio_tool_in(imported_id, "follow", "cccc_bootstrap", json!({}))
        .await;
    assert_eq!(
        copy["result"]["structuredContent"]["session"]["effective_scope"],
        json!({
            "path":fixture.roots[1],"scope_key":"scope_b","source":"actor_default"
        })
    );
    // Deliberately leave the old registration behind to exercise the generation
    // fence, rather than relying only on normal remove/reset cleanup.
    fixture
        .store
        .mutate(&fixture.group_id, |group| {
            let actor = group
                .actors
                .iter_mut()
                .find(|actor| actor.id == "follow")
                .expect("Actor");
            actor.generation = "replacement-scope-generation".into();
            Ok(())
        })
        .expect("replacement generation");
    let denied = fixture.stdio_tool("follow", "cccc_repo", json!({})).await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    let bootstrap = fixture
        .stdio_tool("follow", "cccc_bootstrap", json!({}))
        .await;
    assert_eq!(
        bootstrap["result"]["structuredContent"]["session"]["effective_scope"],
        json!({})
    );
    let reset = fixture
        .ipc("group_reset", json!({"confirm":fixture.group_id}))
        .await;
    let new_id = reset["new_group_id"].as_str().expect("reset ID");
    assert_ne!(new_id, fixture.group_id);
    let reset_scope = fixture
        .stdio_tool_in(new_id, "follow", "cccc_repo", json!({}))
        .await;
    assert_eq!(
        reset_scope["result"]["structuredContent"]["root"],
        json!(fixture.roots[1])
    );
    assert!(!cccc_runtime::status(&fixture.group_id, "follow").is_ok_and(|s| s.running));
    fixture.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn scope_native_without_a_managed_owner_keeps_configuration_only() {
    let fixture = ActorScopeFixture::new().await;
    fixture
        .store
        .mutate(&fixture.group_id, |group| {
            let actor = group
                .actors
                .iter_mut()
                .find(|actor| actor.id == "fixed-b")
                .expect("Actor");
            actor.external_session_id = "codex:isolated-no-provider-source".into();
            Ok(())
        })
        .expect("Native configuration fixture");
    fixture.assert_workspace("fixed-b", 1).await;
    assert!(cccc_runtime::status(&fixture.group_id, "fixed-b").is_err());
    let prompt = fixture
        .ipc("actor_prompt", json!({"actor_id":"fixed-b"}))
        .await;
    assert!(
        prompt["prompt"]
            .as_str()
            .expect("Native prompt")
            .contains("native process cwd: managed by the original application")
    );
    fixture
        .store
        .mutate(&fixture.group_id, |group| {
            group
                .actors
                .iter_mut()
                .find(|actor| actor.id == "fixed-b")
                .expect("Actor")
                .default_scope_key = "missing-pin".into();
            assert_eq!(cccc_core::group_scope::normalize_actor_scope_keys(group), 0);
            Ok(())
        })
        .expect("invalid pin remains unavailable");
    let denied = fixture.stdio_tool("fixed-b", "cccc_repo", json!({})).await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    fixture.stop().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn scope_native_rebind_is_tool_configuration_and_detach_preserves_invalid_pin() {
    let fixture = ActorScopeFixture::new().await;
    let [scope_a, _scope_b] = fixture.same_key_git_scopes();
    let unrelated_root = fixture._temp.path().join("unrelated");
    std::fs::create_dir(&unrelated_root).expect("unrelated workspace");
    let unrelated_scope = cccc_core::scope::detect(&unrelated_root).expect("unrelated scope");
    fixture
        .store
        .mutate(&fixture.group_id, |group| {
            group.scopes = vec![scope_a.clone(), unrelated_scope.clone()];
            group.active_scope_key.clone_from(&scope_a.scope_key);
            for actor in &mut group.actors {
                actor.external_session_id = format!("codex:isolated-{}", actor.id);
                if actor.id == "fixed-b" {
                    actor.default_scope_key.clone_from(&scope_a.scope_key);
                }
            }
            Ok(())
        })
        .expect("Native configuration, no managed owner");
    // Stand in for an independently owned application without running a
    // provider or registering any CCCC process/lifetime authority.
    let mut original_app = tokio::process::Command::new("sh")
        .args(["-c", "exec sleep 120"])
        .current_dir(&fixture.roots[0])
        .env("CCCC_HOME", fixture.home.root())
        .env("HOME", fixture._temp.path().join("native-home"))
        .env("CODEX_HOME", fixture._temp.path().join("native-home/codex"))
        .env(
            "CLAUDE_CONFIG_DIR",
            fixture._temp.path().join("native-home/claude"),
        )
        .kill_on_drop(true)
        .spawn()
        .expect("original-application fixture");
    let pid = original_app.id().expect("fixture PID");
    fixture.assert_workspace("fixed-b", 0).await;
    fixture
        .ipc("attach", json!({"path":fixture.roots[1],"activate":true}))
        .await;
    for (actor, source) in [("fixed-b", "actor_default"), ("follow", "group_active")] {
        assert!(cccc_runtime::status(&fixture.group_id, actor).is_err());
        fixture.assert_workspace(actor, 1).await;
        let bootstrap = fixture.stdio_tool(actor, "cccc_bootstrap", json!({})).await;
        assert_eq!(
            bootstrap["result"]["structuredContent"]["session"]["effective_scope"]["source"],
            source
        );
        let prompt = fixture.ipc("actor_prompt", json!({"actor_id":actor})).await;
        assert!(
            prompt["prompt"]
                .as_str()
                .expect("Native prompt")
                .contains("native process cwd: managed by the original application")
        );
    }
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).expect("original app cwd after rebind"),
        fixture.roots[0]
    );
    println!(
        "native configuration boundary: {}",
        json!({"original_app_pid":pid,"original_app_cwd":fixture.roots[0],
            "cccc_tool_root_after_rebind":fixture.roots[1],"fixed_source":"actor_default",
            "follow_source":"group_active","managed_owner":false})
    );
    fixture
        .ipc("group_detach_scope", json!({"scope_key":scope_a.scope_key}))
        .await;
    let group = fixture
        .store
        .load(&fixture.group_id)
        .expect("detached Group");
    assert_eq!(group.active_scope_key, unrelated_scope.scope_key);
    assert_eq!(
        cccc_core::actors::find(&group, "fixed-b")
            .expect("Native Actor")
            .default_scope_key,
        scope_a.scope_key
    );
    for (name, args) in [
        ("cccc_repo", json!({})),
        ("cccc_file", json!({"path":"same.txt"})),
        (
            "cccc_repo_edit",
            json!({"path":"same.txt","action":"write","content":"denied"}),
        ),
        ("cccc_shell", json!({"command":["pwd"]})),
        (
            "cccc_file_send",
            json!({"path":"same.txt","to":["user"],"mode":"send"}),
        ),
    ] {
        let denied = fixture.stdio_tool("fixed-b", name, args).await;
        assert_eq!(denied["result"]["isError"], true, "{denied}");
    }
    let bootstrap = fixture
        .stdio_tool("fixed-b", "cccc_bootstrap", json!({}))
        .await;
    assert_eq!(
        bootstrap["result"]["structuredContent"]["session"]["effective_scope"],
        json!({})
    );
    let following = fixture.stdio_tool("follow", "cccc_repo", json!({})).await;
    assert_eq!(
        following["result"]["structuredContent"]["root"],
        json!(unrelated_root),
        "Native Follow Group is tool configuration, not original-app cwd"
    );
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).expect("original app cwd after detach"),
        fixture.roots[0]
    );
    original_app.kill().await.expect("retire only fixture app");
    original_app.wait().await.expect("reap fixture app");
    fixture.stop().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn scope_pure_headless_owner_survives_saved_workspace_and_backend_changes() {
    use std::os::unix::fs::PermissionsExt;
    // The production ACP launcher needs a public CCCC executable. Give only
    // this isolated test process a fixture launcher, without mutating the
    // parent test runner's environment or borrowing an installed user binary.
    if std::env::var_os("CCCC_SCOPE_ACP_FIXTURE_CHILD").is_none() {
        let isolated = tempfile::tempdir().expect("launcher fixture");
        let launcher = isolated.path().join("cccc");
        std::fs::write(
            &launcher,
            format!(
                "#!/bin/sh\n[ \"$1\" = mcp ] && shift\nexec '{}' \"$@\"\n",
                env!("CARGO_BIN_EXE_cccc-mcp").replace('\'', "'\\''")
            ),
        )
        .expect("fixture public MCP launcher");
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))
            .expect("launcher permissions");
        let mut child = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
        child
            .args([
                "--exact",
                "scope_pure_headless_owner_survives_saved_workspace_and_backend_changes",
                "--nocapture",
                "--test-threads=1",
            ])
            .current_dir(isolated.path())
            .env("CCCC_SCOPE_ACP_FIXTURE_CHILD", "1")
            .env("CCCC_LAUNCHER_PATH", launcher)
            .env("CCCC_HOME", isolated.path().join("unused-home"))
            .env("HOME", isolated.path().join("home"))
            .env("CODEX_HOME", isolated.path().join("home/codex"))
            .env("CLAUDE_CONFIG_DIR", isolated.path().join("home/claude"))
            .kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(30), child.output())
            .await
            .expect("bounded headless subprocess")
            .expect("headless subprocess");
        println!("{}", String::from_utf8_lossy(&output.stdout));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let fixture = std::sync::Arc::new(ActorScopeFixture::new().await);
    let checked = tokio::spawn({
        let fixture = std::sync::Arc::clone(&fixture);
        async move {
            let program = fixture._temp.path().join("kimi");
            std::fs::write(
                &program,
                include_str!(
                    "../../cccc-daemon/src/ops/codex_voice_analyst/native_acp/candidate_fixture.py"
                ),
            )
            .expect("fixture ACP executable");
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
                .expect("fixture permissions");
            fixture.fixture_process("follow").await;
            fixture
                .ipc(
                    "actor_update",
                    json!({"actor_id":"follow","patch":{
                        "runtime":"kimi","runtime_mode":"acp","command":[program],"env":{
                            "HOME":fixture._temp.path().join("headless-home"),
                            "CODEX_HOME":fixture._temp.path().join("headless-home/codex"),
                            "CLAUDE_CONFIG_DIR":fixture._temp.path().join("headless-home/claude"),
                            "KIMI_CODE_HOME":fixture._temp.path().join("headless-home/kimi"),
                            "ACP_FIXTURE_CLI":"/bin/true"
                        }
                    }}),
                )
                .await;
            fixture
                .ipc("actor_start", json!({"actor_id":"follow"}))
                .await;
            let pid = fixture.pid("follow").await;
            assert_eq!(
                std::fs::read_link(format!("/proc/{pid}/cwd")).expect("headless physical cwd"),
                fixture.roots[0]
            );
            assert!(
                cccc_runtime::status(&fixture.group_id, "follow").is_err(),
                "pure headless: no viewer registry to borrow cwd from"
            );
            fixture
                .ipc(
                    "actor_update",
                    json!({"actor_id":"follow","patch":{
                        "default_scope_key":"scope_b","runtime":"custom","runtime_mode":"default",
                        "command":["sh","-c","exec sleep 120"]
                    }}),
                )
                .await;
            assert_eq!(
                fixture.pid("follow").await,
                pid,
                "saving must retain the actual owner"
            );
            fixture.assert_workspace("follow", 0).await;
            fixture
                .ipc(
                    "send",
                    json!({"text":"scope-fixture-turn","to":["follow"],"message_mode":"send"}),
                )
                .await;
            let prompt = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let records =
                        std::fs::read_to_string(fixture.roots[0].join("candidate_requests.jsonl"))
                            .expect("fixture requests");
                    if let Some(text) = records
                        .lines()
                        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                        .filter(|request| request["method"] == "session/prompt")
                        .filter_map(|request| {
                            request["params"]["prompt"][0]["text"]
                                .as_str()
                                .map(str::to_owned)
                        })
                        .find(|text| text.contains("scope-fixture-turn"))
                    {
                        break text;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("actual startup preamble delivered to fixture");
            assert!(
                prompt.contains(&format!("workspace: {}\n", fixture.roots[0].display())),
                "{prompt}"
            );
            assert!(
                !prompt.contains(&format!("workspace: {}\n", fixture.roots[1].display())),
                "{prompt}"
            );
            fixture
                .ipc("actor_restart", json!({"actor_id":"follow"}))
                .await;
            let next = fixture.pid("follow").await;
            assert_ne!(pid, next);
            assert_eq!(
                std::fs::read_link(format!("/proc/{next}/cwd")).expect("explicit next launch cwd"),
                fixture.roots[1]
            );
            fixture.assert_workspace("follow", 1).await;
        }
    })
    .await;
    // Always retire this test's managed owner, including after an assertion
    // panic. Never stop unrelated process-global registries or user services.
    fixture
        .ipc("actor_stop", json!({"actor_id":"follow"}))
        .await;
    std::sync::Arc::try_unwrap(fixture)
        .ok()
        .expect("fixture refs released")
        .stop()
        .await;
    if let Err(error) = checked {
        std::panic::resume_unwind(error.into_panic());
    }
}
