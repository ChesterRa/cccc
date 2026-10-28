use super::*;
use cccc_core::local_sessions::{self as native, NativeHost, NativeSession};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

mod resume_tests;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    source: NativeSession,
}
impl Fixture {
    fn new() -> Self {
        Self::with_runtime(LocalSessionRuntime::Claude)
    }
    fn with_runtime(runtime: LocalSessionRuntime) -> Self {
        let temp = tempfile::tempdir().expect("fixture should succeed");
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("fixture should succeed");
        let host = NativeHost::capture(std::process::id()).expect("fixture should succeed");
        let source_key = if runtime == LocalSessionRuntime::Opencode {
            "ses_fixture".to_owned()
        } else {
            host.key()
        };
        let source = NativeSession {
            source_id: runtime.source_id(&source_key),
            runtime,
            native_session_id: "first-conversation".into(),
            host: host.clone(),
            cwd: temp.path().to_string_lossy().into(),
            title: "Claude native fixture".into(),
            origin: None,
            revision: "first-revision".into(),
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: None,
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: (runtime == LocalSessionRuntime::Opencode).then_some(host),
            retired: None,
        };
        native::write(&home, source.clone()).expect("fixture should succeed");
        Self {
            _temp: temp,
            home,
            source,
        }
    }
    fn attach(&self) -> store::Receiver {
        store::attach(&self.home, &self.source.source_id, "fixture-receiver")
            .expect("fixture should succeed")
    }
    fn conversation(&self) -> GroupDoc {
        self.attach();
        let result = conversations::create(
            &self.home,
            &req(
                "local_session_conversation",
                json!({"source_ids":[self.source.source_id]}),
            ),
        )
        .expect("fixture should succeed");
        GroupStore::new(self.home.clone())
            .expect("fixture should succeed")
            .load(result["group_id"].as_str().expect("fixture should succeed"))
            .expect("fixture should succeed")
    }
    fn handoff(&self, group: &GroupDoc) -> transport::Handoff {
        transport::Handoff {
            handoff_id: String::new(),
            source_id: self.source.source_id.clone(),
            native_session_id: String::new(),
            group_id: group.group_id.clone(),
            actor_id: group.actors[0].id.clone(),
            actor_generation: cccc_core::actors::generation_identity(&group.actors[0]),
            event_ids: vec!["event-fixture".into()],
            text: "Synthetic notification".into(),
        }
    }
    fn call(&self, op: &str, args: Value) -> Value {
        let response = crate::dispatch::dispatch(&self.home, &req(op, args));
        assert!(response.ok, "{op}: {:?}", response.error);
        Value::Object(response.result)
    }
    fn wait_state(&self, group: &GroupDoc, event_id: &str, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let state = super::super::runtime_delivery::latest_state(
                &self.home,
                &group.group_id,
                &group.actors[0].id,
                event_id,
            )
            .expect("delivery state");
            if state.as_ref().is_some_and(|(state, _)| state == expected) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected}, got {state:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(groups) = GroupStore::new(self.home.clone()).and_then(|s| s.list()) {
            for group in groups {
                super::super::actor_delivery::shutdown_group(&group.group_id);
            }
        }
        let _ = store::detach(&self.home, &self.source.source_id, "fixture-receiver");
    }
}
#[test]
fn claude_compact_notification_recovers_full_message_and_instructions_without_redelivery() {
    let fixture = Fixture::new();
    let group = fixture.conversation();
    let actor = &group.actors[0];
    let receiver = fixture.attach();
    fixture.call(
        "group_preamble_set",
        json!({"group_id":group.group_id,"by":"user","content":"CUSTOM_NATIVE_INSTRUCTIONS"}),
    );
    let body = "Long native request 界😀 \"quoted\"\n".repeat(1_000);
    let sent = fixture.call("message_send", json!({"group_id":group.group_id,"by":"user","to":[actor.id],"message_mode":"request_reply","text":body}));
    let id = sent["event"]["id"].as_str().expect("event ID");
    let handoff = offered(&fixture.home, &receiver);
    let line = serde_json::to_vec(&json!({"message":handoff.text})).expect("Monitor JSON");
    assert!(line.len() < 500);
    assert!(handoff.text.contains(id));
    assert!(handoff.text.contains("cccc_message_history("));
    assert!(!handoff.text.contains("CUSTOM_NATIVE_INSTRUCTIONS"));
    let read = fixture.call(
        "message_history",
        json!({"group_id":group.group_id,"actor_id":actor.id,"by":actor.id,"event_id":id}),
    );
    assert_eq!(read["messages"][0]["data"]["text"], body);
    let delivery = read["delivery_text"].as_str().expect("full delivery");
    assert!(delivery.contains(&body));
    assert!(delivery.contains("CUSTOM_NATIVE_INSTRUCTIONS"));
    assert!(delivery.contains(&format!("You are {} (foreman)", actor.id)));
    assert!(delivery.contains("cccc_message_reply"));
    fixture.wait_state(&group, id, "claimed");
    transport::acknowledge(
        &fixture.home,
        &receiver,
        &handoff.handoff_id,
        transport::Outcome::Accepted,
    )
    .expect("accept");
    fixture.wait_state(&group, id, "accepted");
    assert!(
        transport::next(&fixture.home, &receiver)
            .expect("no replay")
            .is_none()
    );
}

fn req(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("fixture should succeed").clone(),
    }
}
fn offered(home: &HomeLayout, receiver: &store::Receiver) -> transport::Handoff {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(handoff) = transport::next(home, receiver).expect("fixture should succeed") {
            return handoff;
        }
        assert!(Instant::now() < deadline, "handoff must not stall");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn extension_receive_lease_context_replacement_and_disable_preserve_authority() {
    for runtime in [LocalSessionRuntime::Pi, LocalSessionRuntime::Opencode] {
        let fixture = Fixture::with_runtime(runtime);
        let group = fixture.conversation();
        assert_eq!(
            group.actors[0].runtime,
            if runtime == LocalSessionRuntime::Pi {
                ActorRuntime::Pi
            } else {
                ActorRuntime::Opencode
            }
        );
        let receiver = fixture.attach();
        let sent = fixture.call("message_send", json!({"group_id":group.group_id,"by":"user","to":[group.actors[0].id],"message_mode":"send","text":"Pi native receipt"}));
        let handoff = offered(&fixture.home, &receiver);
        assert!(handoff.text.contains("Pi native receipt"));
        assert!(handoff.text.contains(&group.group_id));
        let event_id = sent["event"]["id"].as_str().expect("event ID");
        transport::acknowledge(
            &fixture.home,
            &receiver,
            &handoff.handoff_id,
            transport::Outcome::Accepted,
        )
        .expect("Pi admission");
        fixture.wait_state(&group, event_id, "accepted");
        let mut replacement = fixture.source.clone();
        let binding = cccc_contracts::local_session::LocalSessionBinding {
            group_id: group.group_id.clone(),
            actor_id: group.actors[0].id.clone(),
            actor_generation: cccc_core::actors::generation_identity(&group.actors[0]),
            source_id: fixture.source.source_id.clone(),
            session_revision: fixture.source.revision.clone(),
        };
        native::validate_binding(&fixture.home, &binding).expect("current Pi tool authority");
        replacement.native_session_id = "new-conversation".into();
        replacement.revision = "new-revision".into();
        native::write(&fixture.home, replacement).expect("new Pi context");
        assert!(
            native::validate_binding(&fixture.home, &binding).is_err(),
            "old MCP request cannot act in a new Pi context"
        );
        store::detach(
            &fixture.home,
            &fixture.source.source_id,
            &receiver.connection_id,
        )
        .expect("unload old extension");
        let new_receiver = store::attach(&fixture.home, &fixture.source.source_id, "new-receiver")
            .expect("new Pi extension");
        assert!(
            transport::next(&fixture.home, &new_receiver)
                .expect("accepted input never replays")
                .is_none()
        );
        native::set_integration_disabled(&fixture.home, runtime, true).expect("disable Pi");
        assert!(
            store::attach(
                &fixture.home,
                &fixture.source.source_id,
                "disabled-receiver"
            )
            .is_err()
        );
        assert!(store::sources(&fixture.home).expect("directory").is_empty());
    }
}

#[test]
fn overview_reads_do_not_qualify_native_sources_or_dispatch_pending_work() {
    for runtime in [
        LocalSessionRuntime::Codex,
        LocalSessionRuntime::Claude,
        LocalSessionRuntime::Pi,
        LocalSessionRuntime::Opencode,
    ] {
        let fixture = Fixture::with_runtime(runtime);
        let groups = GroupStore::new(fixture.home.clone()).expect("store");
        let mut group = groups.create("Overview fixture", "").expect("Group");
        let mut actor = Actor::new("native");
        actor.runtime = actor_runtime(runtime);
        actor.external_session_id = fixture.source.source_id.clone();
        group.actors.push(actor);
        group.running = true;
        groups.save(&group).expect("save membership");
        let path = groups.ledger_path(&group.group_id).expect("ledger");
        let mut pending = cccc_contracts::Event::new("chat.message", &group.group_id);
        pending.by = "user".into();
        pending.data = json!({"to":["native"],"message_mode":"send","text":"Unclaimed fixture"})
            .as_object()
            .expect("event data")
            .clone();
        cccc_core::ledger::append(&path, &pending).expect("pending work");

        for ready in [false, true] {
            if ready {
                if runtime == LocalSessionRuntime::Codex {
                    let mut session = fixture.source.clone();
                    session.codex_can_receive = true;
                    native::write(&fixture.home, session).expect("qualification evidence");
                } else {
                    fixture.attach();
                }
            }
            let before = std::fs::read(&path).expect("ledger before reads");
            let lifecycle = native::read_record(&fixture.home, &fixture.source.source_id)
                .expect("lifecycle before reads");
            let configuration = groups.load(&group.group_id).expect("Group before reads");
            for _ in 0..3 {
                let directory = fixture.call("local_session_list", json!({"include_offline":true}));
                assert_eq!(directory["sources"][0]["connected"], ready);
                let overview = fixture.call("group_list", json!({}));
                assert_eq!(overview["groups"][0]["running"], ready);
                assert_eq!(
                    overview["groups"][0]["runtime_status"]["running_actor_count"],
                    usize::from(ready)
                );
                let actors = fixture.call("actor_list", json!({"group_id":group.group_id}));
                assert_eq!(actors["actors"][0]["running"], ready);
            }
            assert_eq!(std::fs::read(&path).expect("ledger after reads"), before);
            assert_eq!(
                native::read_record(&fixture.home, &fixture.source.source_id)
                    .expect("lifecycle after reads"),
                lifecycle
            );
            assert_eq!(
                serde_json::to_value(groups.load(&group.group_id).expect("Group after reads"))
                    .expect("Group value"),
                serde_json::to_value(configuration).expect("Group value")
            );
            assert!(
                super::super::runtime_delivery::latest_state(
                    &fixture.home,
                    &group.group_id,
                    "native",
                    &pending.id,
                )
                .expect("delivery state")
                .is_none(),
                "an overview read must not claim pending native input"
            );
        }
    }
}

#[test]
fn group_controls_preserve_native_hosts_and_independent_membership_access() {
    for runtime in [
        LocalSessionRuntime::Codex,
        LocalSessionRuntime::Claude,
        LocalSessionRuntime::Pi,
        LocalSessionRuntime::Opencode,
    ] {
        let fixture = Fixture::with_runtime(runtime);
        if runtime == LocalSessionRuntime::Codex {
            let mut session = fixture.source.clone();
            session.codex_can_receive = true;
            native::write(&fixture.home, session).expect("qualification evidence");
        } else {
            fixture.attach();
        }
        let groups = GroupStore::new(fixture.home.clone()).expect("store");
        let mut first = groups.create("First membership", "").expect("first Group");
        let mut actor = Actor::new("native");
        actor.runtime = actor_runtime(runtime);
        actor.external_session_id = fixture.source.source_id.clone();
        first.actors.push(actor.clone());
        first.state = GroupState::Stopped;
        groups.save(&first).expect("first membership");
        let mut other = groups.create("Paused membership", "").expect("other Group");
        actor.enabled = false;
        other.actors.push(actor);
        other.state = GroupState::Paused;
        other.running = true;
        groups.save(&other).expect("paused membership");

        let started = fixture.call(
            "group_start",
            json!({"group_id":first.group_id,"by":"user"}),
        );
        assert_eq!(started["group"]["state"], "active");
        assert_eq!(started["group"]["running"], true);
        fixture.call("group_stop", json!({"group_id":first.group_id,"by":"user"}));
        let stopped = groups.load(&first.group_id).expect("stopped Group");
        assert_eq!(stopped.state, GroupState::Stopped);
        assert!(!stopped.running);
        assert!(
            stopped.actors[0].enabled,
            "Stop must retain membership access"
        );
        let retained = groups.load(&other.group_id).expect("unaffected Group");
        assert_eq!(retained.state, GroupState::Paused);
        assert!(retained.running);
        assert!(!retained.actors[0].enabled);
        let resumed = fixture.call(
            "group_start",
            json!({"group_id":other.group_id,"by":"user"}),
        );
        assert_eq!(resumed["group"]["desired_running"], true);
        assert_eq!(resumed["group"]["running"], false);
        assert_eq!(resumed["group"]["actors"][0]["enabled"], false);
        fixture.call("group_stop", json!({"group_id":other.group_id,"by":"user"}));
        assert!(connected(&fixture.home, &fixture.source.source_id));
        assert!(
            fixture.source.host.alive(),
            "Group controls do not own the native host"
        );
        for group in [&first, &other] {
            let events =
                cccc_core::ledger::read_all(&groups.ledger_path(&group.group_id).expect("ledger"))
                    .expect("lifecycle events");
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].kind, "group.start");
            assert_eq!(events[1].kind, "group.stop");
            assert!(events.iter().all(|event| event.by == "user"));
        }
    }
}

#[test]
fn discovery_and_conversation_do_not_steal_active_group_or_project_defaults() {
    let fixture = Fixture::new();
    assert!(
        store::sources(&fixture.home)
            .expect("fixture should succeed")
            .iter()
            .all(|s| !s.connected)
    );
    assert!(
        GroupStore::new(fixture.home.clone())
            .expect("fixture should succeed")
            .list()
            .expect("fixture should succeed")
            .is_empty()
    );
    let receiver = fixture.attach();
    assert!(connected(&fixture.home, &receiver.source_id));
    let existing = GroupStore::new(fixture.home.clone())
        .expect("store")
        .create("Existing selection", "")
        .expect("existing Group");
    cccc_core::active::set(&fixture.home, &existing.group_id).expect("existing selection");
    let registry = cccc_core::Registry::load(&fixture.home).expect("fixture should succeed");
    let result = conversations::create(
        &fixture.home,
        &req(
            "local_session_conversation",
            json!({"source_ids":[receiver.source_id],"direct":true}),
        ),
    )
    .expect("fixture should succeed");
    let again = conversations::create(
        &fixture.home,
        &req(
            "local_session_conversation",
            json!({"source_ids":[receiver.source_id],"direct":true}),
        ),
    )
    .expect("fixture should succeed");
    assert_eq!(result["group_id"], again["group_id"]);
    assert_eq!(again["reused"], true);
    assert_eq!(
        cccc_core::active::get(&fixture.home).expect("active Group"),
        Some(existing.group_id)
    );
    assert_eq!(
        cccc_core::Registry::load(&fixture.home)
            .expect("fixture should succeed")
            .defaults,
        registry.defaults
    );
    let group = GroupStore::new(fixture.home.clone())
        .expect("fixture should succeed")
        .load(result["group_id"].as_str().expect("fixture should succeed"))
        .expect("fixture should succeed");
    let actor = &group.actors[0];
    let listed =
        super::super::actor_listing::list(&fixture.home, &group, &req("actor_list", json!({})))
            .expect("Actor projection");
    assert_eq!(listed[0]["runner_effective"], "headless");
    assert_eq!(listed[0]["runtime_state_source"], "external_session");
    let activity = super::super::actor_activity::activity_payload(&fixture.home, &group, actor)
        .expect("activity projection");
    assert_eq!(activity["runner_effective"], "headless");
    assert_eq!(
        actor.runtime_state_source,
        cccc_contracts::RuntimeStateSource::ExternalSession
    );
    assert_eq!(actor.default_scope_key, group.scopes[0].scope_key);
    assert!(
        super::super::actor_runtime::apply(&fixture.home, &group, &actor.id, "actor.restart")
            .is_err()
    );
    super::super::actor_runtime::apply(&fixture.home, &group, &actor.id, "actor.stop")
        .expect("fixture should succeed");
    assert!(fixture.source.host.alive());
    assert!(
        connected(&fixture.home, &receiver.source_id),
        "stopping Group access cannot stop native host"
    );
    let before = GroupStore::new(fixture.home.clone())
        .expect("fixture should succeed")
        .list()
        .expect("fixture should succeed")
        .len();
    assert!(
        conversations::send(
            &fixture.home,
            &req(
                "local_session_send",
                json!({"source_ids":[receiver.source_id],"text":" "})
            )
        )
        .is_err()
    );
    assert_eq!(
        GroupStore::new(fixture.home.clone())
            .expect("fixture should succeed")
            .list()
            .expect("fixture should succeed")
            .len(),
        before
    );
}

#[test]
fn recovery_survives_end_without_restoring_authority_and_quotes_the_original_environment() {
    for runtime in [LocalSessionRuntime::Codex, LocalSessionRuntime::Claude] {
        let mut fixture = Fixture::new();
        let cwd = fixture._temp.path().join("project '$(printf WRONG)' 目录");
        let provider_home = fixture._temp.path().join("provider '$(printf WRONG)'");
        std::fs::create_dir(&cwd).expect("original directory");
        std::fs::create_dir(&provider_home).expect("provider configuration");
        fixture.source.runtime = runtime;
        fixture.source.cwd = cwd.to_string_lossy().into_owned();
        if runtime == LocalSessionRuntime::Codex {
            fixture.source.source_id = "codex:thread-fixture".into();
            fixture.source.native_session_id = "thread-fixture".into();
            fixture.source.codex_home = Some(provider_home.clone());
            fixture.source.codex_can_receive = true;
        } else {
            fixture.source.native_session_id = "conversation '$(printf WRONG)'".into();
            fixture.source.claude_config_dir = Some(provider_home.clone());
        }
        native::write(&fixture.home, fixture.source.clone()).expect("native callback");
        if runtime == LocalSessionRuntime::Claude {
            fixture.attach();
        }
        let created = fixture.call(
            "local_session_conversation",
            json!({"source_ids":[fixture.source.source_id]}),
        );
        let group_id = created["group_id"].as_str().expect("Group");
        let actor_id = created["group"]["actors"][0]["id"].as_str().expect("Actor");
        let args = json!({"group_id":group_id,"actor_id":actor_id});
        let online = fixture.call("local_session_recovery", args.clone());
        assert_eq!(online["recovery"]["state"], "connected");
        fixture.call("actor_stop", args.clone());
        assert_eq!(
            fixture.call("local_session_recovery", args.clone()),
            online,
            "access pause must not pretend the native receiver exited"
        );
        let store = GroupStore::new(fixture.home.clone()).expect("store");
        let mut group = store.load(group_id).expect("Group");
        group.state = GroupState::Paused;
        store.save(&group).expect("pause Group");
        assert_eq!(fixture.call("local_session_recovery", args.clone()), online);
        let ledger_path = store.ledger_path(group_id).expect("ledger path");
        let before = std::fs::read(&ledger_path).expect("existing ledger");
        native::retire(
            &fixture.home,
            &fixture.source.source_id,
            &fixture.source.native_session_id,
            &fixture.source.host,
            native::RetireReason::SessionEnd,
        )
        .expect("End");
        let ended = fixture.call("local_session_recovery", args.clone());
        assert_eq!(ended["recovery"]["state"], "session_ended");
        assert_eq!(ended["recovery"]["cwd"], fixture.source.cwd);
        assert_eq!(ended["recovery"]["command"], online["recovery"]["command"]);
        assert!(!connected(&fixture.home, &fixture.source.source_id));
        assert!(session_marker(&fixture.home, &fixture.source.source_id).is_none());
        assert!(native::read(&fixture.home, &fixture.source.source_id).is_err());
        assert!(
            store::sources(&fixture.home)
                .expect("directory")
                .iter()
                .all(|source| source.source_id != fixture.source.source_id)
        );
        if runtime == LocalSessionRuntime::Claude {
            assert!(
                store::receiver(&fixture.home, &fixture.source.source_id, "fixture-receiver")
                    .is_err(),
                "old lease cannot authorize after End"
            );
        }
        let listing = fixture
            .call("actor_list", json!({"group_id":group_id}))
            .to_string();
        assert!(!listing.contains("recovery"));
        assert!(
            !listing.contains("provider '"),
            "private provider paths stay out of Group reads"
        );
        assert_eq!(
            std::fs::read(&ledger_path).expect("ledger after reads"),
            before
        );

        // Execute only a shell function, not any provider executable. This checks
        // the copied snippet's actual argv/cwd/env, including hostile quoting.
        let variable = if runtime == LocalSessionRuntime::Codex {
            "CODEX_HOME"
        } else {
            "CLAUDE_CONFIG_DIR"
        };
        let script = format!(
            "{}() {{ printf '%s\\000' \"$PWD\" \"$CCCC_HOME\" \"${variable}\" \"$@\"; }}\n{}",
            runtime.as_str(),
            ended["recovery"]["command"].as_str().expect("command")
        );
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .env_clear()
            .env("PATH", "/nonexistent")
            .env("HOME", fixture._temp.path())
            .env("CODEX_HOME", &provider_home)
            .env("CLAUDE_CONFIG_DIR", &provider_home)
            .output()
            .expect("shell fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let fields = String::from_utf8(output.stdout).expect("fixture fields");
        assert_eq!(
            fields.split_terminator('\0').collect::<Vec<_>>(),
            vec![
                fixture.source.cwd.as_str(),
                fixture.home.root().to_str().expect("home"),
                provider_home.to_str().expect("provider home"),
                if runtime == LocalSessionRuntime::Codex {
                    "resume"
                } else {
                    "--resume"
                },
                fixture.source.native_session_id.as_str(),
            ]
        );
    }
}

#[test]
fn recovery_distinguishes_disconnect_disable_host_exit_and_unknown_without_guessing_claude_ids() {
    let fixture = Fixture::new();
    let group = fixture.conversation();
    let args = json!({"group_id":group.group_id,"actor_id":group.actors[0].id});
    store::detach(&fixture.home, &fixture.source.source_id, "fixture-receiver")
        .expect("disconnect receiver");
    assert_eq!(
        fixture.call("local_session_recovery", args.clone())["recovery"]["state"],
        "disconnected"
    );
    native::retire(
        &fixture.home,
        &fixture.source.source_id,
        &fixture.source.native_session_id,
        &fixture.source.host,
        native::RetireReason::IntegrationDisabled,
    )
    .expect("disable integration");
    assert_eq!(
        fixture.call("local_session_recovery", args.clone())["recovery"]["state"],
        "integration_disabled"
    );
    let mut source = fixture.source.clone();
    source.host.started.push('0');
    native::write(&fixture.home, source).expect("host fingerprint no longer present");
    let exited = fixture.call("local_session_recovery", args.clone());
    assert_eq!(exited["recovery"]["state"], "host_exited");
    fixture.call("local_session_list", json!({"include_offline":true}));
    assert_eq!(
        fixture.call("local_session_recovery", args.clone()),
        exited,
        "directory reads must retain recovery information without reviving the source"
    );
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    let mut group = group;
    for (runtime, source_id, tail) in [
        (
            ActorRuntime::Claude,
            "claude:old-host-without-metadata",
            "claude --resume",
        ),
        (
            ActorRuntime::Codex,
            "codex:known-thread",
            "codex resume known-thread",
        ),
    ] {
        group.actors[0].runtime = runtime;
        group.actors[0].external_session_id = source_id.into();
        store
            .save(&group)
            .expect("older membership without metadata");
        let unknown = fixture.call("local_session_recovery", args.clone());
        assert_eq!(unknown["recovery"]["state"], "unknown");
        assert_eq!(unknown["recovery"]["cwd"], Value::Null);
        assert!(
            unknown["recovery"]["command"]
                .as_str()
                .expect("command")
                .ends_with(tail)
        );
        assert!(
            !unknown["recovery"]["command"]
                .as_str()
                .expect("command")
                .contains("old-host-without-metadata")
        );
    }
    for extra in [
        json!({"by":"claude-1"}),
        json!({"source_id":fixture.source.source_id}),
        json!({"_cccc_web_binding":{}}),
    ] {
        let mut denied = args.clone();
        denied
            .as_object_mut()
            .expect("args")
            .extend(extra.as_object().expect("extra").clone());
        let response =
            crate::dispatch::dispatch(&fixture.home, &req("local_session_recovery", denied));
        assert_eq!(response.error.expect("denied").code, "permission_denied");
    }
}

#[test]
fn activity_snapshots_include_native_members_offline_before_the_first_tick() {
    let fixture = Fixture::new();
    fixture.attach();
    let mut codex = fixture.source.clone();
    codex.source_id = "codex:availability-fixture".into();
    codex.native_session_id = "availability-fixture".into();
    codex.runtime = LocalSessionRuntime::Codex;
    codex.codex_can_receive = true;
    native::write(&fixture.home, codex.clone()).expect("Codex source");
    let created = fixture.call(
        "local_session_conversation",
        json!({"source_ids":[codex.source_id, fixture.source.source_id]}),
    );
    let group_id = created["group_id"].as_str().expect("Group ID");
    assert!(
        fixture.call("actor_list", json!({"group_id":group_id}))["actors"]
            .as_array()
            .expect("members")
            .iter()
            .all(|actor| actor["running"] == true)
    );
    let mut publisher = super::super::actor_activity::Publisher::default();
    // The Web can hydrate a connected snapshot just before the ordinary
    // publisher first observes this Group, including after a pause/resume.
    codex.codex_can_receive = false;
    native::write(&fixture.home, codex.clone()).expect("disconnect Codex");
    publisher.tick(&fixture.home).expect("first status tick");
    store::detach(&fixture.home, &fixture.source.source_id, "fixture-receiver")
        .expect("disconnect Claude");
    publisher.tick(&fixture.home).expect("both disconnected");
    codex.codex_can_receive = true;
    native::write(&fixture.home, codex).expect("reconnect Codex");
    publisher.tick(&fixture.home).expect("Codex reconnected");
    publisher.tick(&fixture.home).expect("stable tick");

    let group_store = GroupStore::new(fixture.home.clone()).expect("store");
    let events = cccc_core::ledger::read_all(&group_store.ledger_path(group_id).expect("ledger"))
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "actor.activity")
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 3, "stable availability must not add events");
    for (event, expected) in events.iter().zip([
        [("codex-1", false), ("claude-1", true)],
        [("codex-1", false), ("claude-1", false)],
        [("codex-1", true), ("claude-1", false)],
    ]) {
        let actual = event.data["actors"]
            .as_array()
            .expect("availability snapshot")
            .iter()
            .map(|actor| {
                (
                    actor["id"].as_str().expect("Actor ID"),
                    actor["running"].as_bool().expect("receive readiness"),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(actual, expected.into_iter().collect());
    }
    assert!(fixture.source.host.alive());
}

#[test]
fn different_native_working_directories_keep_their_own_actor_scope() {
    let fixture = Fixture::new();
    fixture.attach();
    let directory = fixture._temp.path().join("second-project");
    std::fs::create_dir(&directory).expect("second project");
    let mut second = fixture.source.clone();
    second.source_id = "codex:second-fixture".into();
    second.native_session_id = "second-fixture".into();
    second.runtime = LocalSessionRuntime::Codex;
    second.cwd = directory.to_string_lossy().into_owned();
    second.codex_can_receive = true;
    native::write(&fixture.home, second.clone()).expect("second source");
    let response = conversations::create(
        &fixture.home,
        &req(
            "local_session_conversation",
            json!({"source_ids":[fixture.source.source_id,second.source_id],"source_id":fixture.source.source_id}),
        ),
    )
    .expect("conversation");
    let group = GroupStore::new(fixture.home.clone())
        .expect("store")
        .load(response["group_id"].as_str().expect("ID"))
        .expect("Group");
    let events = cccc_core::ledger::read_all(
        &GroupStore::new(fixture.home.clone())
            .expect("store")
            .ledger_path(&group.group_id)
            .expect("ledger"),
    )
    .expect("events");
    assert_eq!(events[0].by, "svc:cccc.local-session");
    assert_eq!(events[0].data["source_id"], fixture.source.source_id);
    assert_eq!(group.scopes.len(), 2);
    assert_ne!(
        group.actors[0].default_scope_key,
        group.actors[1].default_scope_key
    );
}

#[test]
fn conversation_roles_follow_the_initiator_or_selection_and_survive_direct_reuse() {
    for native_initiated in [true, false] {
        let fixture = Fixture::new();
        fixture.attach();
        let mut codex = fixture.source.clone();
        codex.source_id = "codex:initiator".into();
        codex.native_session_id = "initiator".into();
        codex.runtime = LocalSessionRuntime::Codex;
        codex.codex_can_receive = true;
        native::write(&fixture.home, codex.clone()).expect("Codex source");
        let mut args = if native_initiated {
            json!({"source_ids":[fixture.source.source_id, codex.source_id],"source_id":codex.source_id})
        } else {
            json!({"source_ids":[codex.source_id,fixture.source.source_id,codex.source_id]})
        };
        args["direct"] = json!(true);
        let first = conversations::create(&fixture.home, &req("local_session_conversation", args))
            .expect("new conversation");
        let store = GroupStore::new(fixture.home.clone()).expect("store");
        let original = store
            .load(first["group_id"].as_str().expect("ID"))
            .expect("Group");
        assert_eq!(
            original.actors.len(),
            2,
            "duplicate selections are one member"
        );
        let initiator = original
            .actors
            .iter()
            .find(|a| a.external_session_id == codex.source_id)
            .expect("Codex member");
        assert_eq!(
            cccc_core::actors::effective_role(&original, &initiator.id),
            Some(cccc_contracts::ActorRole::Foreman),
            "the initiating source or first selected source coordinates the new Group",
        );
        let again = conversations::create(
            &fixture.home,
            &req(
                "local_session_conversation",
                json!({
                    "source_ids":[codex.source_id],
                    "source_id":fixture.source.source_id,
                    "direct":true,
                }),
            ),
        )
        .expect("reverse-direction reuse");
        assert_eq!(again["group_id"], first["group_id"]);
        assert_eq!(again["reused"], true);
        let reused = store
            .load(first["group_id"].as_str().expect("ID"))
            .expect("Group");
        assert_eq!(
            reused.actors, original.actors,
            "reuse cannot change roles or member identities"
        );
    }
}

#[test]
fn joining_preserves_existing_members_defaults_and_their_message_boundaries() {
    let fixture = Fixture::new();
    let mut group = fixture.conversation();
    let groups = GroupStore::new(fixture.home.clone()).expect("store");
    group.actors[0].enabled = false;
    for id in ["codex-1", "codex-3"] {
        let mut actor = Actor::new(id);
        actor.enabled = false;
        cccc_core::actors::add(&mut group, actor).expect("existing member");
    }
    group.state = GroupState::Paused;
    group.running = false;
    groups.save(&group).expect("existing access and defaults");
    let before = groups.load(&group.group_id).expect("original Group");
    let elsewhere = groups.create("User selection", "").expect("another Group");
    cccc_core::active::set(&fixture.home, &elsewhere.group_id).expect("user selection");
    let defaults = cccc_core::Registry::load(&fixture.home)
        .expect("registry")
        .defaults;
    let path = groups.ledger_path(&group.group_id).expect("ledger");
    let mut old_messages = Vec::new();
    for mode in ["mail", "send"] {
        let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["@all"],"text":"Before joining","message_mode":mode})
            .as_object()
            .expect("source")
            .clone();
        cccc_core::ledger::append(&path, &event).expect("earlier message");
        old_messages.push(event);
    }
    let directory = fixture._temp.path().join("new-project");
    std::fs::create_dir(&directory).expect("new native directory");
    let mut source = fixture.source.clone();
    source.source_id = "codex:joining-source".into();
    source.native_session_id = "joining-source".into();
    source.runtime = LocalSessionRuntime::Codex;
    source.cwd = directory.to_string_lossy().into_owned();
    source.codex_can_receive = true;
    native::write(&fixture.home, source.clone()).expect("native source");
    let args = json!({
        "group_id":group.group_id,
        "source_ids":[source.source_id, fixture.source.source_id, source.source_id]
    });
    let joined = fixture.call("local_session_join", args.clone());
    assert_eq!(joined["group_id"], group.group_id);
    assert_eq!(joined["reused"], false);
    let after = groups.load(&group.group_id).expect("joined Group");
    assert_eq!(&after.actors[..before.actors.len()], &before.actors);
    assert_eq!(after.actors.len(), before.actors.len() + 1);
    assert_eq!(after.active_scope_key, before.active_scope_key);
    assert_eq!(after.state, before.state);
    assert_eq!(after.running, before.running);
    assert_eq!(after.scopes[0], before.scopes[0]);
    assert_eq!(after.scopes.len(), before.scopes.len() + 1);
    assert_eq!(
        cccc_core::active::get(&fixture.home).expect("active Group"),
        Some(elsewhere.group_id)
    );
    assert_eq!(
        cccc_core::Registry::load(&fixture.home)
            .expect("registry")
            .defaults,
        defaults
    );
    let added = after.actors.last().expect("new member");
    assert_eq!(added.id, "codex-2", "existing sparse Actor IDs stay intact");
    assert_eq!(added.external_session_id, source.source_id);
    assert_ne!(added.generation, before.actors[0].generation);
    assert_eq!(added.default_scope_key, after.scopes[1].scope_key);
    assert_eq!(
        cccc_core::actors::effective_role(&after, &added.id),
        Some(cccc_contracts::ActorRole::Peer)
    );
    assert!(
        cccc_core::inbox::list_unread(&fixture.home, &after, &added.id, 20)
            .expect("new member Inbox")
            .is_empty(),
        "a new source must not inherit old broadcast Mail"
    );
    assert!(
        super::super::runtime_delivery::pending_sources(&fixture.home, &after, added, 20)
            .expect("new member delivery")
            .is_empty(),
        "a new source must not inherit old delivery work"
    );
    let pending = super::super::runtime_delivery::pending_sources(
        &fixture.home,
        &after,
        &after.actors[0],
        20,
    )
    .expect("old member work");
    assert_eq!(pending, [old_messages[1].clone()]);
    let events = cccc_core::ledger::read_all(&path).expect("ledger");
    assert_eq!(fixture.call("local_session_join", args)["reused"], true);
    assert_eq!(groups.load(&group.group_id).expect("same Group"), after);
    assert_eq!(
        cccc_core::ledger::read_all(&path).expect("same ledger"),
        events
    );
    let mut later = old_messages[0].clone();
    later.id = uuid::Uuid::new_v4().to_string();
    cccc_core::ledger::append(&path, &later).expect("new broadcast Mail");
    assert_eq!(
        cccc_core::inbox::list_unread(&fixture.home, &after, &added.id, 20)
            .expect("new member receives subsequent Mail"),
        [later]
    );
    for denied in [
        json!({"source_id":source.source_id}),
        json!({"by":before.actors[0].id}),
        json!({"_cccc_web_binding":{}}),
    ] {
        let mut args = json!({"group_id":group.group_id,"source_ids":[source.source_id]});
        args.as_object_mut()
            .expect("args")
            .extend(denied.as_object().expect("denied origin").clone());
        let response = crate::dispatch::dispatch(&fixture.home, &req("local_session_join", args));
        assert!(!response.ok, "native and cloud callers cannot self-admit");
        assert_eq!(response.error.expect("denied").code, "permission_denied");
    }
}

#[test]
fn clear_settles_missing_old_ack_and_cannot_acknowledge_the_next_message() {
    let fixture = Fixture::new();
    let group = fixture.conversation();
    let old = fixture.attach();
    let h = fixture.handoff(&group);
    let home = fixture.home.clone();
    let task = std::thread::spawn(move || transport::submit(&home, h, &AtomicBool::new(false)));
    let previous = offered(&fixture.home, &old);
    let mut metadata = fixture.source.clone();
    metadata.native_session_id = "after-clear".into();
    metadata.revision = "after-clear-revision".into();
    native::write(&fixture.home, metadata).expect("fixture should succeed");
    let current = store::receiver(&fixture.home, &old.source_id, &old.connection_id)
        .expect("fixture should succeed");
    assert!(
        transport::next(&fixture.home, &current)
            .expect("fixture should succeed")
            .is_none()
    );
    assert!(matches!(
        task.join().expect("fixture should succeed"),
        transport::Outcome::Ambiguous(_)
    ));
    let h = fixture.handoff(&group);
    let home = fixture.home.clone();
    let task = std::thread::spawn(move || transport::submit(&home, h, &AtomicBool::new(false)));
    let next = offered(&fixture.home, &current);
    assert_eq!(next.native_session_id, "after-clear");
    assert!(
        transport::acknowledge(
            &fixture.home,
            &current,
            &previous.handoff_id,
            transport::Outcome::Accepted
        )
        .is_err()
    );
    transport::acknowledge(
        &fixture.home,
        &current,
        &next.handoff_id,
        transport::Outcome::Accepted,
    )
    .expect("fixture should succeed");
    assert_eq!(
        task.join().expect("fixture should succeed"),
        transport::Outcome::Accepted
    );
}

#[test]
fn rejection_and_receiver_detach_do_not_block_later_work() {
    let fixture = Fixture::new();
    let group = fixture.conversation();
    let receiver = fixture.attach();
    for outcome in [
        transport::Outcome::Rejected("fixture rejection".into()),
        transport::Outcome::Accepted,
    ] {
        let h = fixture.handoff(&group);
        let home = fixture.home.clone();
        let task = std::thread::spawn(move || transport::submit(&home, h, &AtomicBool::new(false)));
        let handoff = offered(&fixture.home, &receiver);
        transport::acknowledge(
            &fixture.home,
            &receiver,
            &handoff.handoff_id,
            outcome.clone(),
        )
        .expect("fixture should succeed");
        assert_eq!(task.join().expect("fixture should succeed"), outcome);
    }
    let h = fixture.handoff(&group);
    let home = fixture.home.clone();
    let task = std::thread::spawn(move || transport::submit(&home, h, &AtomicBool::new(false)));
    offered(&fixture.home, &receiver);
    store::detach(&fixture.home, &receiver.source_id, &receiver.connection_id)
        .expect("fixture should succeed");
    assert!(matches!(
        task.join().expect("fixture should succeed"),
        transport::Outcome::Ambiguous(_)
    ));
    assert!(!connected(&fixture.home, &receiver.source_id));
    let new = store::attach(&fixture.home, &receiver.source_id, "new-receiver")
        .expect("fixture should succeed");
    store::detach(&fixture.home, &receiver.source_id, &receiver.connection_id)
        .expect("fixture should succeed");
    assert!(
        store::receiver(&fixture.home, &new.source_id, &new.connection_id).is_ok(),
        "old cleanup must not detach new owner"
    );
    store::detach(&fixture.home, &new.source_id, &new.connection_id)
        .expect("fixture should succeed");
}

#[test]
fn stopped_or_recreated_actor_never_receives_a_pending_handoff() {
    let fixture = Fixture::new();
    let group = fixture.conversation();
    let receiver = fixture.attach();
    let h = fixture.handoff(&group);
    let home = fixture.home.clone();
    let task = std::thread::spawn(move || transport::submit(&home, h, &AtomicBool::new(false)));
    std::thread::sleep(Duration::from_millis(20));
    GroupStore::new(fixture.home.clone())
        .expect("fixture should succeed")
        .mutate(&group.group_id, |doc| {
            doc.actors[0].enabled = false;
            Ok(())
        })
        .expect("fixture should succeed");
    assert!(
        transport::next(&fixture.home, &receiver)
            .expect("fixture should succeed")
            .is_none()
    );
    assert_eq!(
        task.join().expect("fixture should succeed"),
        transport::Outcome::Unavailable
    );
}

#[test]
fn one_corrupt_source_does_not_hide_other_sources_or_accept_cloud_control() {
    let fixture = Fixture::new();
    fixture.attach();
    std::fs::write(
        fixture
            .home
            .root()
            .join("state/local_sessions/codex%3Abroken.json"),
        "{",
    )
    .expect("fixture should succeed");
    assert_eq!(
        store::sources(&fixture.home)
            .expect("fixture should succeed")
            .len(),
        1
    );
    assert!(
        list(
            &fixture.home,
            &req("local_session_list", json!({"_cccc_web_binding":{}}))
        )
        .is_err()
    );
    let groups = GroupStore::new(fixture.home.clone()).expect("store");
    let broken = groups
        .create("Legacy unavailable Group", "")
        .expect("Group");
    std::fs::write(
        groups
            .group_dir(&broken.group_id)
            .expect("Group directory")
            .join("group.yaml"),
        "actors: [",
    )
    .expect("unreadable Group document");
    let args = json!({"source_ids":[fixture.source.source_id],"direct":true});
    let created = fixture.call("local_session_conversation", args.clone());
    let reused = fixture.call("local_session_conversation", args);
    assert_eq!(created["reused"], false);
    assert_eq!(reused["group_id"], created["group_id"]);
    assert_eq!(reused["reused"], true);
}

// Re-exec instead of mutating process-wide provider homes in parallel tests.
fn isolated_codex_test(name: &str) -> bool {
    const ISOLATED: &str = "CCCC_TEST_CODEX_DELIVERY_ISOLATED";
    if std::env::var_os(ISOLATED).is_some() {
        return false;
    }
    let temp = tempfile::tempdir().expect("isolated native test");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args([
            "--exact",
            &format!("ops::local_sessions::tests::{name}"),
            "--nocapture",
        ])
        .env(ISOLATED, "1");
    for (variable, directory) in [
        ("CCCC_HOME", "cccc"),
        ("HOME", "user"),
        ("CODEX_HOME", "codex"),
        ("CLAUDE_CONFIG_DIR", "claude"),
    ] {
        let path = temp.path().join(directory);
        std::fs::create_dir(&path).expect("isolated provider directory");
        command.env(variable, path);
    }
    let output = command.output().expect("isolated native test process");
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

struct CodexDeliveryFixture {
    fixture: Fixture,
    group: GroupDoc,
    program: std::path::PathBuf,
    provider: std::path::PathBuf,
}

impl CodexDeliveryFixture {
    fn new() -> Self {
        let mut fixture = Fixture::new();
        let program = fixture._temp.path().join("codex-fixture");
        let provider = fixture._temp.path().join("isolated-codex");
        std::fs::create_dir(&provider).expect("isolated Codex Home");
        fixture.source.runtime = LocalSessionRuntime::Codex;
        fixture.source.source_id = "codex:queue-fixture".into();
        fixture.source.native_session_id = "queue-fixture".into();
        fixture.source.codex_program = Some(program.clone());
        fixture.source.codex_home = Some(provider.clone());
        fixture.source.codex_can_receive = true;
        native::write(&fixture.home, fixture.source.clone()).expect("Codex lifecycle fixture");
        let result = fixture.call(
            "local_session_conversation",
            json!({"source_ids":[fixture.source.source_id]}),
        );
        let group = GroupStore::new(fixture.home.clone())
            .expect("Group store")
            .load(result["group_id"].as_str().expect("Group ID"))
            .expect("native Group");
        let fixture = Self {
            fixture,
            group,
            program,
            provider,
        };
        fixture.install_program();
        fixture
    }

    fn install_program(&self) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&self.program, concat!(
            "#!/bin/sh\n",
            "[ \"$1\" = queue ] || exit 1\n",
            "[ -d \"$CCCC_HOME\" ] && [ -d \"$HOME\" ] && [ -d \"$CODEX_HOME\" ] && [ -d \"$CLAUDE_CONFIG_DIR\" ] || exit 2\n",
            "[ \"$2 $3 $4 $5 $6\" = '--remote unix:// --thread queue-fixture --message' ] || exit 3\n",
            "printf '%s\\n' \"$$\" >> \"$CODEX_HOME/calls\"\n",
            "printf '%s' \"$7\" > \"$CODEX_HOME/payload-$$\"\n",
            "case \"$7\" in\n",
            " *REJECT_FIXTURE*) exit 1;;\n",
            " *STALL_FIXTURE*) touch \"$CODEX_HOME/started\"; exec sleep 60;;\n",
            "esac\n",
        )).expect("fake provider executable");
        std::fs::set_permissions(&self.program, std::fs::Permissions::from_mode(0o700))
            .expect("fake provider permissions");
    }

    fn send(&self, text: &str) -> String {
        self.fixture.call(
            "local_session_send",
            json!({
                "group_id":self.group.group_id,"text":text
            }),
        )["event"]["id"]
            .as_str()
            .expect("source event")
            .to_owned()
    }

    fn receipts(&self, id: &str) -> Vec<cccc_contracts::Event> {
        let store = GroupStore::new(self.fixture.home.clone()).expect("Group store");
        cccc_core::ledger::read_all(&store.ledger_path(&self.group.group_id).expect("ledger"))
            .expect("delivery ledger")
            .into_iter()
            .filter(|event| event.kind == "runtime.delivery" && event.data["source_event_id"] == id)
            .collect()
    }

    fn wait_state(&self, id: &str, expected: &str) -> Vec<cccc_contracts::Event> {
        let deadline = Instant::now() + Duration::from_secs(13);
        loop {
            let receipts = self.receipts(id);
            if receipts
                .last()
                .is_some_and(|event| event.data["state"] == expected)
            {
                println!(
                    "{id}: {}",
                    json!(receipts.iter().map(|event| &event.data).collect::<Vec<_>>())
                );
                return receipts;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected}, got {}",
                json!(receipts)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.provider.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn restart_worker(&self) {
        super::super::actor_delivery::shutdown_group(&self.group.group_id);
        assert_eq!(super::super::runtime_delivery::settle_stranded_claims(
            &self.fixture.home, &self.group,
        ).expect("startup claim recovery"), 0);
        self.fixture.call(
            "actor_start",
            json!({
                "group_id":self.group.group_id,"actor_id":self.group.actors[0].id
            }),
        );
    }

    fn assert_settled_not_replayed(&self, calls: usize) {
        self.restart_worker();
        assert!(
            super::super::runtime_delivery::pending_sources(
                &self.fixture.home,
                &self.group,
                &self.group.actors[0],
                512,
            )
            .expect("native pending work")
            .is_empty()
        );
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(self.calls().len(), calls, "settled work must not replay");
    }
}

#[test]
fn native_codex_missing_program_recovers_once() {
    if isolated_codex_test("native_codex_missing_program_recovers_once") {
        return;
    }
    for restart in [false, true] {
        let fixture = CodexDeliveryFixture::new();
        std::fs::remove_file(&fixture.program).expect("temporarily unavailable program");
        let id = fixture.send("Missing program, retained delivery");
        let receipts = fixture.wait_state(&id, "failed");
        assert_eq!(
            receipts.last().expect("failed receipt").data["retryable"],
            true
        );
        assert!(fixture.calls().is_empty(), "queue did not start");
        fixture.install_program();
        if restart {
            fixture.restart_worker();
        }
        fixture.wait_state(&id, "accepted");
        assert_eq!(fixture.calls().len(), 1, "restored program accepts once");
        fixture.assert_settled_not_replayed(1);
        println!("missing program -> recovery; restart={restart}; queue calls=1");
    }
}

#[test]
fn native_refresh_returns_revalidated_details_on_unknown_qualification_and_fences_replacement() {
    if isolated_codex_test(
        "native_refresh_returns_revalidated_details_on_unknown_qualification_and_fences_replacement",
    ) {
        return;
    }
    let fixture = CodexDeliveryFixture::new();
    let home = &fixture.fixture.home;
    let args = json!({"group_id":fixture.group.group_id,"actor_id":fixture.group.actors[0].id,"by":"user"});
    let before = recovery(home, &req("local_session_recovery", args.clone())).expect("recovery");
    // The fixture rejects app-server qualification without running a model.
    let refreshed =
        refresh(home, &req("local_session_refresh", args.clone())).expect("validated snapshot");
    assert_eq!(refreshed["refresh_status"], "unavailable");
    assert_eq!(refreshed["recovery"], before["recovery"]);
    assert_eq!(refreshed["source_id"], before["source_id"]);
    assert!(
        native::read(home, &fixture.fixture.source.source_id)
            .expect("source")
            .codex_can_receive
    );

    let started = fixture.provider.join("qualification-started");
    let release = fixture.provider.join("release-qualification");
    std::fs::write(&fixture.program, format!(
        "#!/bin/sh\ntouch {}\ni=0\nwhile [ ! -f {} ] && [ $i -lt 500 ]; do sleep 0.01; i=$((i+1)); done\nexit 1\n",
        shell_words::quote(&started.to_string_lossy()), shell_words::quote(&release.to_string_lossy())
    )).expect("controlled qualification fixture");
    let thread_home = home.clone();
    let refreshing =
        std::thread::spawn(move || refresh(&thread_home, &req("local_session_refresh", args)));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !started.exists() {
        assert!(Instant::now() < deadline, "qualification must start");
        std::thread::sleep(Duration::from_millis(5));
    }
    GroupStore::new(home.clone())
        .expect("store")
        .mutate(&fixture.group.group_id, |group| {
            group.actors[0].generation = "replacement-generation".into();
            Ok(())
        })
        .expect("replace member during qualification");
    std::fs::write(release, "release").expect("release fixture");
    assert_eq!(
        refreshing
            .join()
            .expect("refresh worker")
            .expect_err("changed member")
            .code,
        "local_session_changed"
    );
    assert!(
        !fixture.provider.join("calls").exists(),
        "no queue input was submitted"
    );
}

#[test]
fn native_codex_rejects_oversized_rendered_payload_before_claim() {
    if isolated_codex_test("native_codex_rejects_oversized_rendered_payload_before_claim") {
        return;
    }
    let page = std::process::Command::new("getconf")
        .arg("PAGESIZE")
        .output()
        .expect("page size");
    assert!(page.status.success());
    let argument_limit = String::from_utf8(page.stdout)
        .expect("page size UTF-8")
        .trim()
        .parse::<usize>()
        .expect("page size")
        * 32;
    for (label, body) in [
        ("reported ASCII", "B".repeat(200_000)),
        ("multibyte Unicode", "界".repeat(argument_limit / 3)),
        ("rendering overhead", "R".repeat(argument_limit - 64)),
        ("embedded NUL", "before\0after".to_owned()),
    ] {
        let fixture = CodexDeliveryFixture::new();
        let id = fixture.send(&body);
        let receipts = fixture.wait_state(&id, "failed");
        assert_eq!(
            receipts.len(),
            1,
            "{label}: invalid argv must be rejected before durable claim"
        );
        let failed = receipts.last().expect("explicit failure");
        assert_ne!(
            failed.data.get("retryable"),
            Some(&json!(true)),
            "permanent argv failure cannot retry"
        );
        assert!(
            failed.data["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("command-line")),
            "{label}: {failed:?}"
        );
        assert!(fixture.calls().is_empty(), "{label}: no queue invocation");
        let store = GroupStore::new(fixture.fixture.home.clone()).expect("store");
        let ledger = cccc_core::ledger::read_all(
            &store.ledger_path(&fixture.group.group_id).expect("ledger"),
        )
        .expect("ledger");
        assert_eq!(
            ledger
                .iter()
                .find(|event| event.id == id)
                .expect("source body retained")
                .data["text"],
            body
        );
        let next = fixture.send("Small message after permanent rejection");
        fixture.wait_state(&next, "accepted");
        fixture.assert_settled_not_replayed(1);
        println!(
            "{label}: raw bytes={}, chars={}, failed before claim; next accepted once",
            body.len(),
            body.chars().count()
        );
    }
}

#[test]
fn native_codex_started_timeout_is_ambiguous_and_not_replayed() {
    if isolated_codex_test("native_codex_started_timeout_is_ambiguous_and_not_replayed") {
        return;
    }
    let fixture = CodexDeliveryFixture::new();
    let id = fixture.send("STALL_FIXTURE: no native confirmation");
    let receipts = fixture.wait_state(&id, "ambiguous");
    assert_eq!(receipts.len(), 2);
    assert_ne!(receipts[1].data.get("retryable"), Some(&json!(true)));
    assert_eq!(fixture.calls().len(), 1, "queue started before timeout");
    let next = fixture.send("Small message after ambiguous timeout");
    fixture.wait_state(&next, "accepted");
    fixture.assert_settled_not_replayed(2);
}

#[test]
fn native_codex_started_cancel_is_ambiguous_and_not_replayed() {
    if isolated_codex_test("native_codex_started_cancel_is_ambiguous_and_not_replayed") {
        return;
    }
    let fixture = CodexDeliveryFixture::new();
    let id = fixture.send("STALL_FIXTURE: cancel after queue starts");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !fixture.provider.join("started").exists() {
        assert!(
            Instant::now() < deadline,
            "queue must start before cancellation"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    fixture.fixture.call(
        "actor_stop",
        json!({
            "group_id":fixture.group.group_id,"actor_id":fixture.group.actors[0].id
        }),
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cancellation must not await queue timeout"
    );
    fixture.wait_state(&id, "ambiguous");
    assert_eq!(fixture.calls().len(), 1);
    fixture.restart_worker();
    let next = fixture.send("Small message after cancellation");
    fixture.wait_state(&next, "accepted");
    fixture.assert_settled_not_replayed(2);
}

#[test]
fn native_codex_rejection_is_failed_and_not_replayed() {
    if isolated_codex_test("native_codex_rejection_is_failed_and_not_replayed") {
        return;
    }
    let fixture = CodexDeliveryFixture::new();
    let id = fixture.send("REJECT_FIXTURE: known rejection");
    let receipts = fixture.wait_state(&id, "failed");
    assert_eq!(receipts.len(), 2);
    assert_ne!(receipts[1].data.get("retryable"), Some(&json!(true)));
    let next = fixture.send("Small message after known rejection");
    fixture.wait_state(&next, "accepted");
    fixture.assert_settled_not_replayed(2);
}

#[test]
fn native_codex_preserves_large_quoted_unicode_payload_and_receiving_context() {
    if isolated_codex_test(
        "native_codex_preserves_large_quoted_unicode_payload_and_receiving_context",
    ) {
        return;
    }
    let fixture = CodexDeliveryFixture::new();
    let body = format!("{}END_PAYLOAD 界😀", "界😀 \"quoted\" \\\n".repeat(4_000));
    let id = fixture.send(&body);
    fixture.wait_state(&id, "accepted");
    let calls = fixture.calls();
    assert_eq!(calls.len(), 1);
    let payload = std::fs::read_to_string(fixture.provider.join(format!("payload-{}", calls[0])))
        .expect("exact provider payload");
    let store = GroupStore::new(fixture.fixture.home.clone()).expect("store");
    let event =
        cccc_core::ledger::read_all(&store.ledger_path(&fixture.group.group_id).expect("ledger"))
            .expect("ledger")
            .into_iter()
            .find(|event| event.id == id)
            .expect("source body retained");
    assert_eq!(event.data["text"], body);
    let actor = &fixture.group.actors[0];
    let rendered = super::super::actor_delivery_render::render_batch_with_mail_context(
        &fixture.fixture.home,
        &fixture.group,
        &actor.id,
        &[event],
    )
    .expect("rendered delivery");
    let preamble =
        super::super::actor_delivery_preamble::render(&fixture.fixture.home, &fixture.group, actor);
    let expected = format!("{preamble}\n\n{rendered}");
    assert_eq!(
        payload.as_bytes(),
        expected.as_bytes(),
        "argv must preserve the entire rendered input"
    );
    assert!(
        payload.contains(&body),
        "no truncation or implicit splitting"
    );
    println!(
        "accepted exact rendered payload: raw={} bytes / {} chars, queue={} bytes",
        body.len(),
        body.chars().count(),
        payload.len()
    );
    fixture.assert_settled_not_replayed(1);
}

#[test]
fn native_codex_capacity_refills_offline_400_and_recovered_270_without_replay() {
    if isolated_codex_test(
        "native_codex_capacity_refills_offline_400_and_recovered_270_without_replay",
    ) {
        return;
    }
    for (count, restart) in [(400, false), (270, true)] {
        let fixture = CodexDeliveryFixture::new();
        let mut offline = fixture.fixture.source.clone();
        offline.codex_can_receive = false;
        native::write(&fixture.fixture.home, offline).expect("offline qualification snapshot");
        let ids = (0..count)
            .map(|index| {
                fixture.send(&format!(
                    "CAPACITY_FIXTURE[{count}:{index}] 界😀 \"quoted\" \\"
                ))
            })
            .collect::<Vec<_>>();
        assert!(
            fixture.calls().is_empty(),
            "offline delivery cannot invoke queue"
        );
        let pending = super::super::runtime_delivery::pending_sources(
            &fixture.fixture.home,
            &fixture.group,
            &fixture.group.actors[0],
            count + 1,
        )
        .expect("durable offline work");
        assert_eq!(
            pending.iter().map(|event| &event.id).collect::<Vec<_>>(),
            ids.iter().collect::<Vec<_>>()
        );
        let store = GroupStore::new(fixture.fixture.home.clone()).expect("store");
        let path = store.ledger_path(&fixture.group.group_id).expect("ledger");
        assert!(
            !cccc_core::ledger::read_all(&path)
                .expect("offline ledger")
                .iter()
                .any(|event| event.kind == "runtime.delivery"),
            "offline messages remain unclaimed"
        );
        if !restart {
            assert!(
                super::super::actor_delivery::pending_group_ids().contains(&fixture.group.group_id),
                "overflow requests a bounded refill"
            );
        }
        if restart {
            super::super::actor_delivery::shutdown_group(&fixture.group.group_id);
        }
        native::write(&fixture.fixture.home, fixture.fixture.source.clone())
            .expect("qualified receive channel restored");
        if restart {
            fixture.restart_worker();
            assert!(
                super::super::actor_delivery::pending_group_ids().contains(&fixture.group.group_id),
                "a full recovery page requests its tail"
            );
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        let events = loop {
            // Exercise the daemon owner's refill tick, never a polling GET.
            super::super::actor_delivery::drain_group(
                &fixture.fixture.home,
                &fixture.group.group_id,
            );
            let events = cccc_core::ledger::read_all(&path).expect("delivery ledger");
            let accepted = events
                .iter()
                .filter(|event| {
                    event.kind == "runtime.delivery" && event.data["state"] == "accepted"
                })
                .count();
            if accepted == count {
                break events;
            }
            assert!(
                Instant::now() < deadline,
                "only {accepted}/{count} accepted after recovery"
            );
            std::thread::sleep(Duration::from_millis(25));
        };
        for id in &ids {
            let states = events
                .iter()
                .filter(|event| {
                    event.kind == "runtime.delivery" && event.data["source_event_id"] == *id
                })
                .map(|event| event.data["state"].as_str().expect("delivery state"))
                .collect::<Vec<_>>();
            assert_eq!(
                states,
                ["claimed", "accepted"],
                "exactly one submission per event"
            );
        }
        let payloads = fixture
            .calls()
            .iter()
            .map(|pid| {
                std::fs::read_to_string(fixture.provider.join(format!("payload-{pid}")))
                    .expect("provider input")
            })
            .collect::<Vec<_>>();
        assert_eq!(payloads.len(), count);
        for index in 0..count {
            let body = format!("CAPACITY_FIXTURE[{count}:{index}] 界😀 \"quoted\" \\");
            assert_eq!(
                payloads
                    .iter()
                    .filter(|payload| payload.contains(&body))
                    .count(),
                1
            );
        }
        fixture.assert_settled_not_replayed(count);
        println!(
            "capacity control: {count} offline messages, worker restart={restart}, {count} exact queue payloads and accepted receipts, no replay"
        );
    }
}

#[test]
fn native_codex_preflight_preserves_existing_claim_and_terminal_states() {
    if isolated_codex_test("native_codex_preflight_preserves_existing_claim_and_terminal_states") {
        return;
    }
    use super::super::runtime_delivery::DeliveryOutcome as LedgerOutcome;
    let fixture = CodexDeliveryFixture::new();
    let store = GroupStore::new(fixture.fixture.home.clone()).expect("store");
    let path = store.ledger_path(&fixture.group.group_id).expect("ledger");
    let actor = &fixture.group.actors[0];
    for (state, outcome) in [
        ("claimed", LedgerOutcome::Claimed),
        ("accepted", LedgerOutcome::Accepted),
        ("ambiguous", LedgerOutcome::Ambiguous("prior attempt")),
        ("withdrawn", LedgerOutcome::Withdrawn("prior attempt")),
    ] {
        let mut event = cccc_contracts::Event::new("chat.message", &fixture.group.group_id);
        event.by = "user".into();
        event.data = json!({"text":"invalid\0argv","to":[actor.id],"message_mode":"send"})
            .as_object()
            .expect("source data")
            .clone();
        cccc_core::ledger::append(&path, &event).expect("source event");
        super::super::runtime_delivery::append_state(
            &fixture.fixture.home,
            &fixture.group.group_id,
            &actor.id,
            &actor.created_at,
            &event.id,
            "local_session",
            outcome,
        )
        .expect("prior durable evidence");
        let id = event.id.clone();
        let mut jobs = vec![super::super::actor_delivery::DeliveryJob {
            home: fixture.fixture.home.clone(),
            group: fixture.group.clone(),
            actor: actor.clone(),
            event,
            claimed: false,
        }];
        assert_eq!(
            super::super::actor_delivery_worker::process_batch(
                &mut jobs,
                &mut String::new(),
                &AtomicBool::new(false),
            ),
            super::super::actor_delivery_worker::BatchOutcome::Delivered
        );
        assert!(jobs.is_empty());
        let receipts = fixture.receipts(&id);
        assert_eq!(receipts.len(), 1, "preflight must not overwrite {state}");
        assert_eq!(receipts[0].data["state"], state);
    }
    assert!(fixture.calls().is_empty());
}

#[test]
fn native_queue_failure_is_recorded_once_and_the_same_batch_continues() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let codex_home = fixture._temp.path().join("isolated-codex");
    std::fs::create_dir(&codex_home).expect("fixture should succeed");
    let program = fixture._temp.path().join("codex-fixture");
    std::fs::write(&program,"#!/bin/sh\n[ \"$1\" = queue ] || exit 1\nprintf '%s\\n' \"$1 $2 $3 $4 $5 $6\" >> \"$CODEX_HOME/calls\"\ncase \"$7\" in *REJECT_FIXTURE*) exit 1;; esac\n").expect("fixture should succeed");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
        .expect("fixture should succeed");
    let mut metadata = fixture.source.clone();
    metadata.runtime = LocalSessionRuntime::Codex;
    metadata.source_id = "codex:queue-fixture".into();
    metadata.native_session_id = "queue-fixture".into();
    metadata.codex_home = Some(codex_home.clone());
    metadata.codex_program = Some(program);
    metadata.codex_can_receive = true;
    native::write(&fixture.home, metadata.clone()).expect("fixture should succeed");
    let result = conversations::create(
        &fixture.home,
        &req(
            "local_session_conversation",
            json!({"source_ids":[metadata.source_id]}),
        ),
    )
    .expect("fixture should succeed");
    let store = GroupStore::new(fixture.home.clone()).expect("fixture should succeed");
    let group = store
        .load(result["group_id"].as_str().expect("fixture should succeed"))
        .expect("fixture should succeed");
    let actor = &group.actors[0];
    let mut jobs = vec![];
    for text in ["REJECT_FIXTURE", "ACCEPT_FIXTURE"] {
        let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"text":text,"to":[actor.id],"message_mode":"send"})
            .as_object()
            .expect("fixture should succeed")
            .clone();
        cccc_core::ledger::append(
            &store
                .ledger_path(&group.group_id)
                .expect("fixture should succeed"),
            &event,
        )
        .expect("fixture should succeed");
        jobs.push(super::super::actor_delivery::DeliveryJob {
            home: fixture.home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event,
            claimed: false,
        });
    }
    let original = jobs.clone();
    assert_eq!(
        super::super::actor_delivery_worker::process_batch(
            &mut jobs,
            &mut String::new(),
            &AtomicBool::new(false)
        ),
        super::super::actor_delivery_worker::BatchOutcome::Delivered
    );
    assert!(jobs.is_empty());
    for (job, expected) in original.iter().zip(["failed", "accepted"]) {
        let state = super::super::runtime_delivery::latest_state(
            &fixture.home,
            &group.group_id,
            &actor.id,
            &job.event.id,
        )
        .expect("fixture should succeed")
        .expect("fixture should succeed");
        assert_eq!(state.0, expected);
        assert_eq!(state.1, "local_session");
    }
    assert_eq!(
        std::fs::read_to_string(codex_home.join("calls"))
            .expect("fixture should succeed")
            .lines()
            .count(),
        2
    );
    let mut managed_group = group.clone();
    managed_group.actors[0].external_session_id.clear();
    assert_eq!(
        super::super::runtime_delivery::pending_sources(
            &fixture.home,
            &managed_group,
            &managed_group.actors[0],
            20,
        )
        .expect("fixture should succeed")
        .iter()
        .map(|event| event.id.as_str())
        .collect::<Vec<_>>(),
        [original[0].event.id.as_str()],
        "managed failures remain pending without replaying accepted messages"
    );
    assert!(
        super::super::runtime_delivery::pending_sources(&fixture.home, &group, actor, 20)
            .expect("fixture should succeed")
            .is_empty(),
        "known failures must not be automatically repeated"
    );
}

#[test]
fn source_directed_send_does_not_broadcast_to_added_group_members() {
    let fixture = Fixture::new();
    fixture.attach();
    let first = fixture.call("local_session_send", json!({
        "source_ids":[fixture.source.source_id],"text":"Initial direct Mail","message_mode":"mail"
    }));
    let group_id = first["group_id"].as_str().expect("Group ID");
    let groups = GroupStore::new(fixture.home.clone()).expect("store");
    let mut group = groups.load(group_id).expect("Group");
    let native_actor = group.actors[0].id.clone();
    cccc_core::actors::add(&mut group, Actor::new("managed-peer")).expect("extra member");
    groups.save(&group).expect("expanded Group");

    let directed = fixture.call("local_session_send", json!({
        "source_ids":[fixture.source.source_id],"text":"Only the selected source","message_mode":"mail"
    }));
    assert_eq!(directed["group_id"], group_id);
    assert_eq!(directed["event"]["data"]["to"], json!([native_actor]));
    for addressing in [
        json!({"source_ids":[fixture.source.source_id],"to":["@all"]}),
        json!({"source_ids":[fixture.source.source_id],"group_id":group_id}),
    ] {
        let mut args = addressing.as_object().expect("addressing").clone();
        args.insert("text".into(), json!("Reject mixed addressing"));
        args.insert("message_mode".into(), json!("mail"));
        let before = cccc_core::ledger::read_all(&groups.ledger_path(group_id).expect("ledger"))
            .expect("events")
            .len();
        let result = crate::dispatch::dispatch(
            &fixture.home,
            &req("local_session_send", Value::Object(args)),
        );
        assert!(!result.ok);
        assert_eq!(
            result.error.expect("ambiguous addressing").code,
            "invalid_args"
        );
        assert_eq!(
            cccc_core::ledger::read_all(&groups.ledger_path(group_id).expect("ledger"))
                .expect("events")
                .len(),
            before
        );
    }
    let broadcast = fixture.call(
        "local_session_send",
        json!({
            "group_id":group_id,"text":"Explicit Group broadcast","message_mode":"mail"
        }),
    );
    assert_eq!(broadcast["event"]["data"]["to"], json!(["@all"]));
}

#[test]
fn captured_native_authority_rejects_replaced_members_and_conversations() {
    use cccc_contracts::local_session::LocalSessionBinding;
    let fixture = Fixture::new();
    fixture.attach();
    let group = fixture.conversation();
    let actor = &group.actors[0];
    let binding = LocalSessionBinding {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        actor_generation: cccc_core::actors::generation_identity(actor),
        source_id: fixture.source.source_id.clone(),
        session_revision: fixture.source.revision.clone(),
    };
    let request = req(
        "send",
        json!({
            "group_id":group.group_id,"by":actor.id,"to":["user"],"text":"Captured request",
            "message_mode":"send","_cccc_native_binding":binding
        }),
    );
    let initial = crate::dispatch::dispatch(&fixture.home, &request);
    assert!(initial.ok, "initial native request: {:?}", initial.error);
    let groups = GroupStore::new(fixture.home.clone()).expect("store");
    for changed in [
        "source",
        "generation",
        "disabled",
        "revision",
        "caller",
        "group",
    ] {
        groups.save(&group).expect("restore membership");
        let mut current = group.clone();
        let mut request = request.clone();
        let mut session = fixture.source.clone();
        match changed {
            "source" => current.actors[0].external_session_id = "claude:replacement".into(),
            "generation" => current.actors[0].generation = "replacement-generation".into(),
            "disabled" => current.actors[0].enabled = false,
            "revision" => {
                session.native_session_id = "new-conversation".into();
                session.revision = "new-revision".into();
            }
            "caller" => {
                request.args.insert("by".into(), json!("user"));
            }
            "group" => {
                request
                    .args
                    .insert("group_id".into(), json!("another-group"));
            }
            _ => unreachable!(),
        }
        groups.save(&current).expect("membership transition");
        native::write(&fixture.home, session).expect("lifecycle transition");
        let response = crate::dispatch::dispatch(&fixture.home, &request);
        assert!(!response.ok, "{changed}");
        assert_eq!(
            response.error.expect("stale identity").code,
            "permission_denied",
            "{changed}"
        );
    }
    let events = cccc_core::ledger::read_all(&groups.ledger_path(&group.group_id).expect("ledger"))
        .expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "chat.message")
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .all(|event| !event.data.contains_key("_cccc_native_binding"))
    );
}

#[tokio::test]
async fn native_source_authority_is_rechecked_after_waiting_for_the_daemon_permit() {
    use crate::dispatch_concurrency::DispatchLocks;
    use cccc_contracts::local_session::LocalSessionSourceBinding;
    for transition in ["end", "clear"] {
        let fixture = Fixture::new();
        fixture.attach();
        let locks = DispatchLocks::default();
        let blocker = locks.global_read().await;
        let request = req(
            "local_session_conversation",
            json!({
                "source_id":fixture.source.source_id,"source_ids":[],
                "_cccc_native_source_binding":LocalSessionSourceBinding {
                    source_id:fixture.source.source_id.clone(),session_revision:fixture.source.revision.clone()
                }
            }),
        );
        let task_home = fixture.home.clone();
        let mut pending = tokio::spawn(async move {
            let _permit = locks.acquire_for(&task_home, &request).await;
            crate::dispatch::dispatch(&task_home, &request)
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut pending)
                .await
                .is_err(),
            "request must wait for the global permit"
        );
        if transition == "end" {
            native::retire(
                &fixture.home,
                &fixture.source.source_id,
                &fixture.source.native_session_id,
                &fixture.source.host,
                native::RetireReason::SessionEnd,
            )
            .expect("end while queued");
        } else {
            let mut changed = fixture.source.clone();
            changed.native_session_id = "cleared-conversation".into();
            changed.revision = "cleared-revision".into();
            native::write(&fixture.home, changed).expect("clear while queued");
        }
        drop(blocker);
        let response = pending.await.expect("queued request");
        assert_eq!(
            response.error.expect("stale authority").code,
            "permission_denied"
        );
        assert!(
            GroupStore::new(fixture.home.clone())
                .expect("store")
                .list()
                .expect("groups")
                .is_empty(),
            "stale creation must not leave a Group"
        );
    }
}

#[test]
fn native_deliveries_always_identify_the_receiving_group_and_member() {
    let fixture = Fixture::new();
    let first = fixture.conversation();
    let second = fixture.conversation();
    let receiver = fixture.attach();
    for (group, label) in [(&first, "A1"), (&second, "B1"), (&first, "A2")] {
        let sent = fixture.call(
            "local_session_send",
            json!({
                "group_id":group.group_id,"text":label
            }),
        );
        let handoff = offered(&fixture.home, &receiver);
        assert_eq!(handoff.group_id, group.group_id);
        assert!(handoff.text.contains(label));
        assert!(
            handoff
                .text
                .starts_with("[CCCC] Read full message and instructions:")
        );
        assert!(!handoff.text.contains("[CCCC] You are "));
        for identity in [
            group.group_id.as_str(),
            group.actors[0].id.as_str(),
            "foreman",
            group.scopes[0].scope_key.as_str(),
        ] {
            assert!(
                handoff.text.contains(identity),
                "{label} lacks {identity}: {}",
                handoff.text
            );
        }
        let full = fixture.call(
            "message_history",
            json!({
                "group_id":group.group_id,"actor_id":group.actors[0].id,"by":group.actors[0].id,
                "event_id":sent["event"]["id"]
            }),
        );
        assert!(
            full["delivery_text"]
                .as_str()
                .expect("full context")
                .contains(&group.scopes[0].url)
        );
        transport::acknowledge(
            &fixture.home,
            &receiver,
            &handoff.handoff_id,
            transport::Outcome::Accepted,
        )
        .expect("accept handoff");
        fixture.wait_state(
            group,
            sent["event"]["id"].as_str().expect("message ID"),
            "accepted",
        );
    }
}

#[test]
fn explicit_resume_requires_the_selected_native_source_and_preserves_group_pause() {
    let fixture = Fixture::new();
    let group = fixture.conversation();
    let groups = GroupStore::new(fixture.home.clone()).expect("store");
    groups
        .mutate(&group.group_id, |doc| {
            doc.state = GroupState::Paused;
            doc.running = false;
            doc.actors[0].enabled = false;
            Ok(())
        })
        .expect("pause membership and Group");
    let stale = req(
        "actor_update",
        json!({"group_id":group.group_id,"actor_id":group.actors[0].id,"by":"user","enabled":true,"external_session_id":"codex:replacement"}),
    );
    assert!(!crate::dispatch::dispatch(&fixture.home, &stale).ok);
    assert!(
        !groups
            .load(&group.group_id)
            .expect("unchanged member")
            .actors[0]
            .enabled
    );
    fixture.call("actor_update", json!({"group_id":group.group_id,"actor_id":group.actors[0].id,"by":"user","enabled":true,"external_session_id":fixture.source.source_id}));
    let resumed = groups.load(&group.group_id).expect("resumed");
    assert!(resumed.actors[0].enabled);
    assert_eq!(
        resumed.actors[0].external_session_id,
        fixture.source.source_id
    );
    assert_eq!(resumed.state, GroupState::Paused);
    assert!(!resumed.running);
}

#[test]
fn native_actor_start_and_stop_preserve_group_stop_and_pause() {
    use cccc_contracts::Event;
    for runtime in [
        LocalSessionRuntime::Claude,
        LocalSessionRuntime::Pi,
        LocalSessionRuntime::Opencode,
    ] {
        for (state, desired_running) in [
            (GroupState::Stopped, false),
            (GroupState::Paused, false),
            (GroupState::Paused, true),
            (GroupState::Idle, false),
            (GroupState::Active, false),
        ] {
            let fixture = Fixture::with_runtime(runtime);
            let mut group = fixture.conversation();
            let actor_id = group.actors[0].id.clone();
            group.state = state;
            group.running = desired_running;
            group.actors[0].enabled = false;
            let store = GroupStore::new(fixture.home.clone()).expect("store");
            store.save(&group).expect("saved access state");
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data =
                json!({"to":[actor_id],"message_mode":"send","text":"Pending native work"})
                    .as_object()
                    .expect("event data")
                    .clone();
            cccc_core::ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
                .expect("pending message");
            fixture.call(
                "actor_start",
                json!({"group_id":group.group_id,"actor_id":actor_id,"by":"user"}),
            );
            let after = store.load(&group.group_id).expect("enabled member");
            assert!(after.actors[0].enabled);
            assert_eq!(after.state, state, "native Start cannot change Group state");
            if matches!(state, GroupState::Stopped | GroupState::Paused) {
                assert_eq!(
                    after.running, desired_running,
                    "native Start cannot release a Group lifecycle fence"
                );
                assert!(
                    super::super::runtime_delivery::latest_state(
                        &fixture.home,
                        &group.group_id,
                        &actor_id,
                        &event.id
                    )
                    .expect("delivery state")
                    .is_none()
                );
            } else {
                assert!(
                    after.running,
                    "enabled native access retains desired running in available Groups"
                );
                let receiver = fixture.attach();
                let handoff = offered(&fixture.home, &receiver);
                assert_eq!(handoff.event_ids, [event.id.clone()]);
                transport::acknowledge(
                    &fixture.home,
                    &receiver,
                    &handoff.handoff_id,
                    transport::Outcome::Accepted,
                )
                .expect("accept");
                fixture.wait_state(&after, &event.id, "accepted");
            }
            fixture.call(
                "actor_stop",
                json!({"group_id":group.group_id,"actor_id":actor_id,"by":"user"}),
            );
            let stopped = store.load(&group.group_id).expect("disabled member");
            assert_eq!(stopped.state, state);
            assert!(!stopped.running);
        }
    }
}

#[test]
fn enabling_an_imported_native_actor_wakes_its_pending_work_without_resuming_the_group() {
    use cccc_contracts::{Actor, ActorRuntime, GroupState};

    for state in [GroupState::Idle, GroupState::Paused, GroupState::Stopped] {
        let fixture = Fixture::new();
        let group = fixture.conversation();
        let groups = GroupStore::new(fixture.home.clone()).expect("store");
        let marker = fixture._temp.path().join("managed-started");
        groups
            .mutate(&group.group_id, |group| {
                let mut managed = Actor::new("managed-peer");
                managed.runtime = ActorRuntime::Custom;
                managed.command = vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    format!("printf started > '{}'", marker.display()),
                ];
                group.actors.push(managed);
                Ok(())
            })
            .expect("additional managed Actor configuration");
        store::detach(&fixture.home, &fixture.source.source_id, "fixture-receiver")
            .expect("native receiver offline");
        let sent = fixture.call(
            "message_send",
            json!({"group_id":group.group_id,"by":"user","to":[group.actors[0].id],"message_mode":"send","text":"Pending before Group copy"}),
        );
        let event_id = sent["event"]["id"].as_str().expect("message ID");
        let exported = fixture.call(
            "group_copy_export",
            json!({"group_id":group.group_id,"by":"user"}),
        );
        fixture.call(
            "actor_stop",
            json!({"group_id":group.group_id,"actor_id":group.actors[0].id,"by":"user"}),
        );
        let original = serde_json::to_value(groups.load(&group.group_id).expect("source Group"))
            .expect("source snapshot");
        let imported = fixture.call(
            "group_copy_import",
            json!({"package_b64":exported["package_b64"],"by":"user"}),
        );
        let imported_id = imported["group_id"].as_str().expect("imported Group ID");
        let mut copy = groups.load(imported_id).expect("imported Group");
        assert!(!copy.running);
        assert!(!copy.actors[0].enabled);
        assert!(copy.actors[1].enabled);
        copy.state = state;
        groups.save(&copy).expect("initial Group access state");
        let receiver = fixture.attach();

        fixture.call(
            "actor_update",
            json!({"group_id":imported_id,"actor_id":copy.actors[0].id,"by":"user","patch":{"enabled":true}}),
        );

        let updated = groups.load(imported_id).expect("updated Group");
        assert!(updated.actors[0].enabled);
        assert_eq!(updated.state, state, "enabling cannot resume Group access");
        assert_eq!(
            updated.running,
            state == GroupState::Idle,
            "an available native membership must persist its desired-running state"
        );
        if state == GroupState::Idle {
            let handoff = offered(&fixture.home, &receiver);
            assert_eq!(handoff.group_id, imported_id);
            assert_eq!(handoff.event_ids, [event_id]);
            transport::acknowledge(
                &fixture.home,
                &receiver,
                &handoff.handoff_id,
                transport::Outcome::Accepted,
            )
            .expect("accept imported pending work");
            fixture.wait_state(&updated, event_id, "accepted");
        }
        assert!(
            transport::next(&fixture.home, &receiver)
                .expect("no duplicate or paused handoff")
                .is_none()
        );
        assert!(!marker.exists(), "enabling cannot launch another Actor");
        assert_eq!(
            serde_json::to_value(groups.load(&group.group_id).expect("source Group"))
                .expect("source snapshot"),
            original,
            "the source Group must remain unchanged"
        );
    }
}

#[test]
fn offline_native_work_stays_pending_without_creating_an_uncertain_claim() {
    let fixture = Fixture::new();
    let group = fixture.conversation();
    store::detach(&fixture.home, &fixture.source.source_id, "fixture-receiver")
        .expect("receiver offline");
    let sent = fixture.call(
        "local_session_send",
        json!({"group_id":group.group_id,"text":"Wait for the original source"}),
    );
    let event_id = sent["event"]["id"].as_str().expect("source event");
    // Reconciliation may encounter the same pending event while it is already
    // waiting in memory. This must not manufacture a failed or claimed attempt.
    super::super::actor_delivery::dispatch_unread(&fixture.home, &group, &group.actors[0].id);
    assert_eq!(
        super::super::runtime_delivery::latest_state(
            &fixture.home,
            &group.group_id,
            &group.actors[0].id,
            event_id,
        )
        .expect("offline delivery state"),
        None
    );
    assert_eq!(
        super::super::runtime_delivery::settle_stranded_claims(&fixture.home, &group)
            .expect("startup recovery leaves unsubmitted work pending"),
        0
    );
    let receiver = fixture.attach();
    let handoff = offered(&fixture.home, &receiver);
    assert_eq!(handoff.event_ids, [event_id]);
    fixture.wait_state(&group, event_id, "claimed");
    transport::acknowledge(
        &fixture.home,
        &receiver,
        &handoff.handoff_id,
        transport::Outcome::Accepted,
    )
    .expect("accept the first and only native handoff");
    fixture.wait_state(&group, event_id, "accepted");
    assert_eq!(
        super::super::actor_delivery::dispatch_unread(&fixture.home, &group, &group.actors[0].id),
        0,
        "reconciliation cannot repeat an accepted handoff"
    );
}

#[test]
fn pausing_native_access_recovers_only_work_that_was_never_offered() {
    for already_offered in [false, true] {
        let fixture = Fixture::new();
        let group = fixture.conversation();
        let receiver = fixture.attach();
        let sent = fixture.call(
            "local_session_send",
            json!({
                "group_id":group.group_id,"text":"Paused before acceptance"
            }),
        );
        let event_id = sent["event"]["id"].as_str().expect("message ID");
        fixture.wait_state(&group, event_id, "claimed");
        if already_offered {
            offered(&fixture.home, &receiver);
        }
        fixture.call(
            "group_set_state",
            json!({"group_id":group.group_id,"state":"paused"}),
        );
        fixture.wait_state(
            &group,
            event_id,
            if already_offered {
                "ambiguous"
            } else {
                "failed"
            },
        );
        fixture.call(
            "group_set_state",
            json!({"group_id":group.group_id,"state":"active"}),
        );
        let pending = super::super::runtime_delivery::pending_sources(
            &fixture.home,
            &group,
            &group.actors[0],
            20,
        )
        .expect("pending work");
        if already_offered {
            assert!(pending.is_empty(), "uncertain handoffs must never replay");
            let later = fixture.call(
                "local_session_send",
                json!({
                    "group_id":group.group_id,"text":"Later control message"
                }),
            );
            let handoff = offered(&fixture.home, &receiver);
            assert_eq!(
                handoff.event_ids,
                [later["event"]["id"].as_str().expect("later ID")]
            );
            transport::acknowledge(
                &fixture.home,
                &receiver,
                &handoff.handoff_id,
                transport::Outcome::Accepted,
            )
            .expect("accept later work");
            fixture.wait_state(
                &group,
                later["event"]["id"].as_str().expect("later ID"),
                "accepted",
            );
        } else {
            assert_eq!(
                pending
                    .iter()
                    .map(|event| event.id.as_str())
                    .collect::<Vec<_>>(),
                [event_id]
            );
            let handoff = offered(&fixture.home, &receiver);
            assert_eq!(handoff.event_ids, [event_id]);
            transport::acknowledge(
                &fixture.home,
                &receiver,
                &handoff.handoff_id,
                transport::Outcome::Accepted,
            )
            .expect("accept resumed work");
            fixture.wait_state(&group, event_id, "accepted");
            assert!(
                super::super::runtime_delivery::pending_sources(
                    &fixture.home,
                    &group,
                    &group.actors[0],
                    20,
                )
                .expect("settled work")
                .is_empty()
            );
        }
        assert!(fixture.source.host.alive());
    }
}

#[test]
fn different_worktrees_of_one_project_are_rejected_without_creating_a_group() {
    let fixture = Fixture::new();
    fixture.attach();
    let first_root = fixture._temp.path().join("checkout-a");
    let second_root = fixture._temp.path().join("checkout-b");
    for root in [&first_root, &second_root] {
        std::fs::create_dir(root).expect("checkout");
        for args in [
            vec!["init", "--quiet"],
            vec![
                "remote",
                "add",
                "origin",
                "https://example.invalid/native-fixture.git",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .expect("local Git configuration")
                    .success()
            );
        }
    }
    let mut first = fixture.source.clone();
    first.cwd = first_root.to_string_lossy().into_owned();
    native::write(&fixture.home, first.clone()).expect("first checkout");
    let mut second = first.clone();
    second.runtime = LocalSessionRuntime::Codex;
    second.source_id = "codex:other-checkout".into();
    second.native_session_id = "other-checkout".into();
    second.cwd = second_root.to_string_lossy().into_owned();
    second.codex_can_receive = true;
    native::write(&fixture.home, second.clone()).expect("second checkout");
    let rejected = crate::dispatch::dispatch(
        &fixture.home,
        &req(
            "local_session_conversation",
            json!({"source_ids":[first.source_id,second.source_id]}),
        ),
    );
    assert!(
        !rejected.ok,
        "distinct physical roots must not silently collapse"
    );
    assert_eq!(
        rejected.error.expect("scope conflict").code,
        "local_session_scope_conflict"
    );
    assert!(
        GroupStore::new(fixture.home.clone())
            .expect("store")
            .list()
            .expect("Groups")
            .is_empty()
    );
    fixture.call(
        "local_session_conversation",
        json!({"source_ids":[second.source_id]}),
    );
    let created = fixture.call(
        "local_session_conversation",
        json!({"source_ids":[first.source_id]}),
    );
    let group_id = created["group_id"].as_str().expect("existing Group ID");
    let groups = GroupStore::new(fixture.home.clone()).expect("store");
    let before = groups.load(group_id).expect("original Group");
    let path = groups.ledger_path(group_id).expect("ledger");
    let events = cccc_core::ledger::read_all(&path).expect("earlier events");
    let mut valid = second.clone();
    valid.source_id = "codex:unrelated-project".into();
    valid.native_session_id = "unrelated-project".into();
    valid.cwd.clone_from(&fixture.source.cwd);
    native::write(&fixture.home, valid.clone()).expect("otherwise valid source");
    let rejected = crate::dispatch::dispatch(
        &fixture.home,
        &req(
            "local_session_join",
            json!({"group_id":group_id,"source_ids":[valid.source_id,second.source_id]}),
        ),
    );
    assert!(!rejected.ok);
    assert_eq!(
        rejected.error.expect("scope conflict").code,
        "local_session_scope_conflict"
    );
    assert_eq!(groups.load(group_id).expect("unchanged Group"), before);
    assert_eq!(
        cccc_core::ledger::read_all(&path).expect("unchanged ledger"),
        events
    );
}
