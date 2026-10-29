use super::*;
use cccc_contracts::Event;
use cccc_core::{actors, ledger};

fn resumed_fixture(runtime: LocalSessionRuntime) -> (Fixture, NativeSession, GroupDoc) {
    let mut fixture = Fixture::with_runtime(runtime);
    let provider = fixture._temp.path().join("provider");
    std::fs::create_dir(&provider).expect("isolated provider directory");
    if runtime == LocalSessionRuntime::Claude {
        fixture.source.claude_config_dir = Some(provider);
    } else {
        fixture.source.pi_agent_dir = Some(provider);
    }
    native::write(&fixture.home, fixture.source.clone()).expect("new host registration");
    // A reused PID with a different creation fingerprint proves the old host
    // is gone without launching a provider or modifying process-global state.
    let mut old = fixture.source.clone();
    old.host.started = "0".into();
    old.source_id = runtime.source_id(&old.host.key());
    old.revision = "previous-host".into();
    old.title = "Previous native host".into();
    native::write(&fixture.home, old.clone()).expect("retained old lifecycle");
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    let mut group = store.create("Original collaboration", "").expect("Group");
    let mut actor = Actor::new("original-member");
    actor.runtime = actor_runtime(runtime);
    actor.external_session_id.clone_from(&old.source_id);
    actors::add(&mut group, actor).expect("original member");
    group.extra.insert(
        "local_session_conversation".into(),
        json!({"direct":true,"source_ids":[old.source_id]}),
    );
    store.save(&group).expect("persist original Group");
    (fixture, old, group)
}

fn attach_resumed(fixture: &Fixture) {
    fixture.call(
        "local_session_receiver_attach",
        json!({"source_id":fixture.source.source_id,"connection_id":"resumed-receiver"}),
    );
}

#[test]
fn receive_attach_restores_the_same_conversation_memberships_and_direct_reuse() {
    for runtime in [LocalSessionRuntime::Claude, LocalSessionRuntime::Pi] {
        let (fixture, old, group) = resumed_fixture(runtime);
        let store = GroupStore::new(fixture.home.clone()).expect("groups");
        let mut stopped = group.clone();
        stopped.group_id = store
            .create("Stopped membership", "")
            .expect("Group")
            .group_id;
        stopped.state = GroupState::Stopped;
        stopped.running = false;
        stopped.actors[0].enabled = false;
        store.save(&stopped).expect("persist stopped Group");

        attach_resumed(&fixture);
        for before in [&group, &stopped] {
            let after = store.load(&before.group_id).expect("resumed Group");
            assert_eq!(after.actors.len(), 1);
            let mut expected = before.actors[0].clone();
            expected
                .external_session_id
                .clone_from(&fixture.source.source_id);
            expected.updated_at.clone_from(&after.actors[0].updated_at);
            assert_eq!(
                after.actors[0], expected,
                "identity and access are retained"
            );
            assert_eq!(after.state, before.state);
            assert_eq!(after.running, before.running);
            assert_eq!(after.scopes, before.scopes);
            let events = ledger::read_all(&store.ledger_path(&before.group_id).expect("ledger"))
                .expect("audit");
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, "actor.update");
            assert_eq!(events[0].by, "svc:cccc.local-session");
            assert_eq!(
                events[0].data["previous_external_session_id"],
                old.source_id
            );
            assert_eq!(events[0].data["reason"], "native_session_resumed");
            assert!(
                !serde_json::to_string(&events)
                    .expect("audit")
                    .contains("provider")
            );
        }
        let reused = fixture.call(
            "local_session_conversation",
            json!({"source_ids":[fixture.source.source_id],"direct":true}),
        );
        assert_eq!(reused["group_id"], group.group_id);
        assert_eq!(reused["reused"], true);
        attach_resumed(&fixture);
        assert_eq!(
            ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
                .expect("audit")
                .len(),
            1,
            "reattachment must not append another restoration event"
        );
    }
}

#[test]
fn receive_attach_does_not_guess_or_override_explicit_native_boundaries() {
    for runtime in [LocalSessionRuntime::Claude, LocalSessionRuntime::Pi] {
        for boundary in [
            "different_conversation",
            "different_provider",
            "missing_provider",
            "disabled",
            "live_host",
            "already_joined",
            "ambiguous_members",
        ] {
            let (fixture, mut old, mut group) = resumed_fixture(runtime);
            match boundary {
                "different_conversation" => old.native_session_id = "another-conversation".into(),
                "different_provider" => {
                    let other = fixture._temp.path().join("other-provider");
                    std::fs::create_dir(&other).expect("different provider");
                    if runtime == LocalSessionRuntime::Claude {
                        old.claude_config_dir = Some(other);
                    } else {
                        old.pi_agent_dir = Some(other);
                    }
                }
                "missing_provider" => {
                    old.claude_config_dir = None;
                    old.pi_agent_dir = None;
                }
                "disabled" => old.retired = Some(native::RetireReason::IntegrationDisabled),
                "live_host" => old.host = fixture.source.host.clone(),
                "already_joined" | "ambiguous_members" => {
                    let mut second = group.actors[0].clone();
                    second.id = "other-member".into();
                    if boundary == "already_joined" {
                        second
                            .external_session_id
                            .clone_from(&fixture.source.source_id);
                    }
                    actors::add(&mut group, second).expect("second member");
                }
                _ => unreachable!(),
            }
            native::write(&fixture.home, old).expect("changed old record");
            let store = GroupStore::new(fixture.home.clone()).expect("groups");
            store.save(&group).expect("Group boundary");
            let before = store.load(&group.group_id).expect("before");
            attach_resumed(&fixture);
            assert_eq!(
                store.load(&group.group_id).expect("after"),
                before,
                "{runtime:?}: {boundary}"
            );
        }
    }
}

#[test]
fn source_directory_reads_do_not_restore_memberships() {
    let (fixture, _, group) = resumed_fixture(LocalSessionRuntime::Claude);
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    let before = store.load(&group.group_id).expect("before");
    fixture.call(
        "local_session_list",
        json!({"source_id":fixture.source.source_id,"include_offline":true}),
    );
    assert_eq!(store.load(&group.group_id).expect("after"), before);
}

#[test]
fn restored_pending_work_keeps_terminal_delivery_receipts() {
    let (fixture, _, mut group) = resumed_fixture(LocalSessionRuntime::Pi);
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    group.state = GroupState::Paused;
    store
        .save(&group)
        .expect("pause delivery while inspecting receipts");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let mut events = Vec::new();
    for state in ["pending", "accepted", "ambiguous", "withdrawn", "claimed"] {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":[group.actors[0].id],"text":state,"message_mode":"send"})
            .as_object()
            .expect("message")
            .clone();
        ledger::append(&path, &event).expect("pending message");
        if state != "pending" {
            use crate::ops::runtime_delivery::{self, DeliveryOutcome};
            runtime_delivery::claim(
                &fixture.home,
                &group,
                &group.actors[0],
                &event.id,
                "local_session",
                false,
            )
            .expect("claim");
            let outcome = match state {
                "accepted" => Some(DeliveryOutcome::Accepted),
                "ambiguous" => Some(DeliveryOutcome::Ambiguous("unknown")),
                "withdrawn" => Some(DeliveryOutcome::Withdrawn("cancelled")),
                _ => None,
            };
            if let Some(outcome) = outcome {
                runtime_delivery::append_state(
                    &fixture.home,
                    &group.group_id,
                    &group.actors[0].id,
                    &group.actors[0].created_at,
                    &event.id,
                    "local_session",
                    outcome,
                )
                .expect("receipt");
            }
        }
        events.push((state, event));
    }
    attach_resumed(&fixture);
    let resumed = store.load(&group.group_id).expect("restored");
    assert_eq!(
        resumed.actors[0].external_session_id,
        fixture.source.source_id
    );
    let pending = crate::ops::runtime_delivery::pending_sources(
        &fixture.home,
        &resumed,
        &resumed.actors[0],
        16,
    )
    .expect("pending projection");
    assert!(pending.iter().any(|event| event.id == events[0].1.id));
    for (_, event) in &events[1..4] {
        assert!(!pending.iter().any(|candidate| candidate.id == event.id));
    }
    for (state, event) in events.into_iter().skip(1) {
        assert_eq!(
            crate::ops::runtime_delivery::latest_state(
                &fixture.home,
                &group.group_id,
                &group.actors[0].id,
                &event.id
            )
            .expect("receipt")
            .expect("retained")
            .0,
            state
        );
        assert_eq!(
            crate::ops::runtime_delivery::claim(
                &fixture.home,
                &resumed,
                &resumed.actors[0],
                &event.id,
                "local_session",
                false
            )
            .expect("no second claim"),
            crate::ops::runtime_delivery::ClaimResult::Terminal(state.into())
        );
    }
}

#[test]
fn resumed_members_only_refresh_an_unchanged_native_title() {
    for custom in [false, true] {
        let (fixture, old, mut group) = resumed_fixture(LocalSessionRuntime::Claude);
        group.actors[0].title = if custom {
            "My reviewer".into()
        } else {
            old.title
        };
        let store = GroupStore::new(fixture.home.clone()).expect("groups");
        store.save(&group).expect("title");
        attach_resumed(&fixture);
        let restored = store.load(&group.group_id).expect("restored");
        assert_eq!(
            restored.actors[0].title,
            if custom {
                "My reviewer"
            } else {
                fixture.source.title.as_str()
            }
        );
        assert_eq!(restored.title, group.title);
    }
}

#[test]
fn failed_recovery_audit_restores_the_original_member_binding() {
    let (fixture, _, group) = resumed_fixture(LocalSessionRuntime::Claude);
    let store = GroupStore::new(fixture.home.clone()).expect("groups");
    let before = store.load(&group.group_id).expect("before");
    let mut healthy = store
        .create("Healthy collaboration", "")
        .expect("other Group");
    actors::add(&mut healthy, group.actors[0].clone()).expect("other original member");
    store.save(&healthy).expect("healthy Group");
    let path = store.ledger_path(&group.group_id).expect("ledger path");
    assert!(
        std::fs::read(&path)
            .expect("empty fixture ledger")
            .is_empty()
    );
    std::fs::remove_file(&path).expect("replace only the empty fixture ledger");
    std::fs::create_dir(&path).expect("unwritable ledger fixture");
    let log_path = fixture.home.daemon_dir().join("recovery-diagnostics.log");
    let diagnostics = cccc_runtime::log_diagnostics_to(&log_path).expect("diagnostic sink");
    let response = crate::dispatch::dispatch(
        &fixture.home,
        &req(
            "local_session_receiver_attach",
            json!({"source_id":fixture.source.source_id,"connection_id":"audit-failure"}),
        ),
    );
    assert!(
        response.ok,
        "a failed restoration must not disconnect the receive lease"
    );
    drop(diagnostics);
    let log = std::fs::read_to_string(log_path).expect("recorded recovery diagnostic");
    assert!(log.contains(&format!("group_id={}", group.group_id)));
    assert!(log.contains(&format!("source_id={}", fixture.source.source_id)));
    assert!(log.contains("error_kind="));
    assert!(!log.contains(fixture.home.root().to_str().expect("fixture path")));
    assert_eq!(
        store.load(&group.group_id).expect("after failed audit"),
        before
    );
    let healthy = store.load(&healthy.group_id).expect("other restored Group");
    assert_eq!(
        healthy.actors[0].external_session_id,
        fixture.source.source_id
    );
    let sent=fixture.call("send",json!({"group_id":healthy.group_id,"by":"user","to":[healthy.actors[0].id],"text":"Healthy group still delivers","message_mode":"send"}));
    let binding = json!({"source_id":fixture.source.source_id,"connection_id":"audit-failure"});
    let deadline = Instant::now() + Duration::from_secs(3);
    let handoff = loop {
        let next = fixture.call("local_session_receiver_next", binding.clone());
        if !next["handoff"].is_null() {
            break next["handoff"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "healthy group delivery was blocked by recovery failure"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(handoff["event_ids"], json!([sent["event"]["id"]]));
    fixture.call("local_session_receiver_ack",json!({"source_id":fixture.source.source_id,"connection_id":"audit-failure","handoff_id":handoff["handoff_id"],"state":"accepted"}));
    fixture.call("local_session_receiver_detach", binding);

    // An unreadable registry fails before any individual Group can be tried.
    // It must still leave a usable lease and a diagnostic without private IO
    // details, just as a single Group's failed audit does above.
    let (unreadable, _, _) = resumed_fixture(LocalSessionRuntime::Claude);
    std::fs::write(
        unreadable.home.registry_path(),
        "private fixture: invalid JSON",
    )
    .expect("unreadable registry fixture");
    let log_path = unreadable
        .home
        .daemon_dir()
        .join("recovery-diagnostics.log");
    let diagnostics = cccc_runtime::log_diagnostics_to(&log_path).expect("diagnostic sink");
    let binding =
        json!({"source_id":unreadable.source.source_id,"connection_id":"registry-failure"});
    let response = crate::dispatch::dispatch(
        &unreadable.home,
        &req("local_session_receiver_attach", binding.clone()),
    );
    assert!(
        response.ok,
        "registry recovery must not disconnect the lease"
    );
    drop(diagnostics);
    let log = std::fs::read_to_string(log_path).expect("recorded registry diagnostic");
    assert!(log.contains(&format!("source_id={}", unreadable.source.source_id)));
    assert!(log.contains("error_code=io_error"));
    assert!(!log.contains("private fixture"));
    assert!(!log.contains(unreadable.home.root().to_str().expect("fixture path")));
    assert!(unreadable.call("local_session_receiver_next", binding.clone())["handoff"].is_null());
    unreadable.call("local_session_receiver_detach", binding);
}
