#![cfg(unix)]

// Included by the crate-level integration test harness.

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};

#[test]
fn chet_handoff_uses_service_sender_without_breaking_idempotency() {
    use cccc_core::{actors, voice_notifications as voice};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Chet sender", "").expect("group");
    group.state = cccc_contracts::GroupState::Stopped;
    group.running = false;
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    actor.enabled = false;
    group.actors.push(actor);
    store.save(&group).expect("save");
    let token = "c".repeat(32);
    voice::register_origin(
        &home,
        &token,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("origin");
    let args = json!({"group_id":group.group_id,"by":"user","to":["worker"],
        "text":"Investigate","message_mode":"send","client_id":"stable-request",
        "_voice_origin":token});
    let first = call(&home, "send", args.clone());
    assert!(first.ok, "first handoff should be admitted");
    assert_eq!(first.result["event"]["by"], actors::CHET_SERVICE_PRINCIPAL);
    let awake = store.load(&group.group_id).expect("group after handoff");
    assert_eq!(awake.state, cccc_contracts::GroupState::Active);
    assert!(
        awake.actors[0].enabled,
        "user-delegated work wakes its target"
    );
    let retry = call(&home, "send", args);
    assert!(retry.ok, "retry should reconcile");
    assert_eq!(retry.result["event"]["id"], first.result["event"]["id"]);
    assert_eq!(retry.result["duplicate"], true);
    let stored =
        ledger::read_all(&store.ledger_path(&group.group_id).expect("path")).expect("ledger");
    assert_eq!(
        stored
            .iter()
            .filter(|event| event.kind == "chat.message")
            .count(),
        1
    );
}

#[test]
fn chet_handoff_treats_an_empty_destination_group_as_local() {
    use cccc_core::{actors, voice_notifications as voice};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Chet local destination", "").expect("group");
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    group.actors.push(actor);
    store.save(&group).expect("save");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::write(workspace.join("note.txt"), "fixture").expect("attachment");
    call(
        &home,
        "attach",
        json!({"group_id":group.group_id,"by":"user","path":workspace}),
    );
    let token = "e".repeat(32);
    voice::register_origin(
        &home,
        &token,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("origin");

    for destination in [json!(null), json!(""), json!("   ")] {
        let sent = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"user","to":["worker"],
                "text":"Inspect","message_mode":"send","_voice_origin":token,
                "dst_group_id":destination}),
        );
        let event = &sent.result["event"];
        assert_eq!(event["by"], actors::CHET_SERVICE_PRINCIPAL);
        assert!(event["data"].get("dst_group_id").is_none(), "{event}");
        let event: cccc_contracts::Event =
            serde_json::from_value(event.clone()).expect("stored event");
        assert!(actors::is_chet_message(&home, &group, &event).expect("Chet origin"));
        let reply = call(
            &home,
            "reply",
            json!({"group_id":group.group_id,"by":"worker",
                "reply_to":event.id,"text":"Done","message_mode":"send"}),
        );
        assert_eq!(
            reply.result["event"]["data"]["to"],
            json!([actors::CHET_SERVICE_PRINCIPAL])
        );
    }

    let sent_file = call(
        &home,
        "send_files",
        json!({"group_id":group.group_id,"by":"user","to":["worker"],
            "paths":["note.txt"],"message_mode":"send","_voice_origin":token,
            "dst_group_id":""}),
    );
    let file_event: cccc_contracts::Event =
        serde_json::from_value(sent_file.result["event"].clone()).expect("file event");
    assert!(file_event.data.get("dst_group_id").is_none());
    assert!(actors::is_chet_message(&home, &group, &file_event).expect("Chet file origin"));
    let file_reply = call(
        &home,
        "reply",
        json!({"group_id":group.group_id,"by":"worker",
            "reply_to":file_event.id,"text":"Received","message_mode":"send"}),
    );
    assert_eq!(
        file_reply.result["event"]["data"]["to"],
        json!([actors::CHET_SERVICE_PRINCIPAL])
    );

    for op in ["send", "send_files"] {
        let mut args = json!({"group_id":group.group_id,"by":"user","to":["worker"],
            "text":"Invalid destination","message_mode":"send",
            "_voice_origin":token,"dst_group_id":7});
        if op == "send_files" {
            args["paths"] = json!(["missing.txt"]);
        }
        let invalid = call_unchecked(&home, op, args);
        assert_eq!(
            invalid.error.expect("invalid destination").code,
            "invalid_args"
        );
    }
}

#[test]
fn invalid_chet_origin_does_not_wake_a_stopped_member() {
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Invalid Chet sender", "").expect("group");
    group.state = cccc_contracts::GroupState::Stopped;
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    actor.enabled = false;
    group.actors.push(actor);
    store.save(&group).expect("save");
    let args = json!({"group_id":group.group_id,"by":"user",
        "to":["worker"],"text":"Investigate","message_mode":"send",
        "_voice_origin":"not-a-valid-origin"});
    let result = cccc_daemon::handle_request(
        &home,
        &DaemonRequest {
            v: 1,
            op: "send".into(),
            args: args.as_object().expect("args").clone(),
        },
    );
    assert!(!result.ok);
    let unchanged = store.load(&group.group_id).expect("group");
    assert_eq!(unchanged.state, cccc_contracts::GroupState::Stopped);
    assert!(!unchanged.actors[0].enabled);
    let events =
        ledger::read_all(&store.ledger_path(&group.group_id).expect("path")).expect("ledger");
    assert!(!events.iter().any(|event| event.kind == "chat.message"));
}

#[test]
fn chet_default_reply_uses_verified_service_origin_and_current_group() {
    use cccc_core::{actors, voice_notifications as voice};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Chet reply", "").expect("group");
    let mut worker = cccc_contracts::Actor::new("worker");
    worker.runtime = cccc_contracts::ActorRuntime::WebModel;
    group.actors.push(worker);
    let mut named = cccc_contracts::Actor::new("named");
    named.title = actors::CHET_SERVICE_PRINCIPAL.into();
    group.actors.push(named);
    store.save(&group).expect("save");
    let token = "d".repeat(32);
    voice::register_origin(
        &home,
        &token,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("origin");
    let source = call(
        &home,
        "send",
        json!({"group_id":group.group_id,"by":"user",
        "to":["worker"],"text":"Inspect","message_mode":"send","_voice_origin":token}),
    );
    let source_id = source.result["event"]["id"].as_str().expect("id");
    let make_request = |op: &str, args: Value| DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("args").clone(),
    };
    let preflight = cccc_daemon::handle_request(
        &home,
        &make_request(
            "message_upload_preflight",
            json!({"group_id":group.group_id,"by":"worker","operation":"reply",
               "reply_to":source_id,"has_attachments":true}),
        ),
    );
    assert!(preflight.ok, "{preflight:?}");
    for args in [
        json!({"group_id":group.group_id,"by":"worker","reply_to":source_id,"text":"answer","message_mode":"mail"}),
        json!({"group_id":group.group_id,"by":"user","reply_to":source_id,"text":"answer","transport":"im"}),
    ] {
        let rejected = cccc_daemon::handle_request(&home, &make_request("reply", args));
        assert!(!rejected.ok, "unsupported reply must fail");
    }
    let reply = call(
        &home,
        "reply",
        json!({"group_id":group.group_id,"by":"worker",
        "reply_to":source_id,"text":"Done"}),
    );
    assert_eq!(
        reply.result["event"]["data"]["to"],
        json!([actors::CHET_SERVICE_PRINCIPAL])
    );
    group.generation = "recreated-group".into();
    store.save(&group).expect("new generation");
    let stale = cccc_daemon::handle_request(
        &home,
        &make_request(
            "reply",
            json!({"group_id":group.group_id,"by":"worker","reply_to":source_id,"text":"stale"}),
        ),
    );
    assert!(
        !stale.ok,
        "old origin cannot identify a new Group generation"
    );
}

#[test]
fn chet_reply_does_not_readdress_a_recreated_member() {
    use cccc_core::{actors, voice_notifications as voice};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("Chet reply owner", "").expect("group");
    let mut worker = cccc_contracts::Actor::new("worker");
    worker.runtime = cccc_contracts::ActorRuntime::WebModel;
    let mut other = cccc_contracts::Actor::new("other");
    other.runtime = cccc_contracts::ActorRuntime::WebModel;
    group.actors.extend([worker.clone(), other]);
    groups.save(&group).expect("save");
    let path = groups.ledger_path(&group.group_id).expect("ledger");
    // An older Actor's creation time is its generation when no UUID exists.
    let mut added = cccc_contracts::Event::new("actor.add", &group.group_id);
    added.data.insert("actor".into(), json!(worker));
    ledger::append(&path, &added).expect("legacy registration");
    let origin = "e".repeat(32);
    voice::register_origin(
        &home,
        &origin,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("origin");
    let ask = |text: &str| {
        call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"worker",
            "to":["@chet"],"message_mode":"send","text":text}),
        )
        .result["event"]["id"]
            .as_str()
            .expect("source ID")
            .to_owned()
    };
    let legacy_source = ask("First request");
    let first_answer = json!({"group_id":group.group_id,"by":"user",
        "_voice_origin":origin,"reply_to":legacy_source,"text":"First answer",
        "client_id":"existing-chet-answer"});
    let answer = call(&home, "reply", first_answer.clone());
    assert_eq!(answer.result["event"]["by"], actors::CHET_SERVICE_PRINCIPAL);
    assert_eq!(answer.result["event"]["data"]["to"], json!(["worker"]));

    // The copy retains old messages and Actor registrations but has a new
    // Group generation; the copied request is not new work in that Group.
    let (copy, _, _) = cccc_core::group_copy::export(&groups, &group.group_id).expect("export");
    let copied_home = HomeLayout::from_path(temp.path().join("copy")).expect("copy home");
    let copied_groups = GroupStore::new(copied_home.clone()).expect("copy groups");
    let copied = cccc_core::group_copy::import(&copied_groups, &copy, "", "").expect("import");
    voice::register_origin(
        &copied_home,
        &origin,
        "copy-analyst",
        "copy-thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("copy origin");
    let copied_reply = call_unchecked(
        &copied_home,
        "reply",
        json!({"group_id":copied.group_id,"by":"user","_voice_origin":origin,
               "reply_to":legacy_source,"text":"Do not send to the copy"}),
    );
    assert!(!copied_reply.ok, "imported source must not be readdressed");
    assert_eq!(copied_reply.error.expect("error").code, "requester_unknown");

    let stale_source = ask("Second request");
    let mut replacement = worker;
    replacement.generation = uuid::Uuid::new_v4().to_string();
    group.actors[0] = replacement.clone();
    groups.save(&group).expect("replacement group");
    let mut added = cccc_contracts::Event::new("actor.add", &group.group_id);
    added.data.insert("actor".into(), json!(replacement));
    ledger::append(&path, &added).expect("replacement registration");
    let replay = call_unchecked(&home, "reply", first_answer);
    assert!(replay.ok, "an already-recorded reply stays idempotent");
    assert_eq!(replay.result["duplicate"], true);
    assert_eq!(replay.result["event"]["id"], answer.result["event"]["id"]);
    let before = ledger::read_all(&path).expect("ledger").len();
    for explicit in [false, true] {
        let mut args = json!({"group_id":group.group_id,"by":"user",
            "_voice_origin":origin,"reply_to":stale_source,"text":"Stale answer"});
        if explicit {
            args["to"] = json!(["worker"]);
        }
        let refused = call_unchecked(&home, "reply", args);
        assert!(
            !refused.ok,
            "replacement member must not receive the answer"
        );
        let error = refused.error.expect("error");
        assert_eq!(error.code, "requester_unavailable");
        assert!(error.message.contains("user"), "provide a way to proceed");
    }
    assert_eq!(ledger::read_all(&path).expect("ledger").len(), before);

    let different_recipient = call_unchecked(
        &home,
        "reply",
        json!({
            "group_id":group.group_id,"by":"user","_voice_origin":origin,
            "reply_to":stale_source,"to":["other"],"text":"Coordinate with another member"
        }),
    );
    assert!(
        different_recipient.ok,
        "an explicit different recipient remains available"
    );

    // Old ledgers without an Actor registration are still readable, but an
    // automatic reply must not guess which registration sent the message.
    let unknown = call(
        &home,
        "send",
        json!({"group_id":group.group_id,"by":"other","to":["@chet"],
               "message_mode":"send","text":"No registration fact"}),
    );
    let refused = call_unchecked(
        &home,
        "reply",
        json!({"group_id":group.group_id,"by":"user","_voice_origin":origin,
               "reply_to":unknown.result["event"]["id"],"text":"Do not guess"}),
    );
    assert!(!refused.ok);
    let error = refused.error.expect("unknown sender");
    assert_eq!(error.code, "requester_unknown");
    assert!(error.message.contains("user"));
}

#[test]
fn chet_provenance_is_host_owned_across_message_ports() {
    use cccc_contracts::chet::GROUP_AUTHOR_GENERATION_FIELD as marker;
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("provenance", "").expect("group");
    let mut payload = json!({"group_id":group.group_id,"by":"user","text":"fixture",
        "to":["user"],"reply_to":"fixture","event_id":"fixture","client_id":"fixture"});
    payload
        .as_object_mut()
        .expect("object")
        .insert(marker.into(), json!("forged"));
    let before = ledger::read_all(&groups.ledger_path(&group.group_id).expect("path"))
        .expect("ledger")
        .len();
    for op in [
        "send",
        "reply",
        "send_files",
        "tracked_send",
        "send_cross_group",
        "connect_send",
        "connect_send_files",
        "reply_request_answer",
        "stream_emit",
        "message_upload_preflight",
    ] {
        // A message write takes only its own arguments, so each gets those,
        // and the marker, which it must refuse itself.
        let mut args = payload.as_object().expect("args").clone();
        if let Some(write) = message_write(op) {
            let unknown = cccc_contracts::message_args::unknown(write, args.keys())
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for key in unknown {
                assert_ne!(key, marker, "{op} must accept the marker to refuse it");
                args.remove(&key);
            }
        }
        let result = cccc_daemon::handle_request(
            &home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            },
        );
        assert!(!result.ok, "{op} must reject caller-supplied provenance");
        let error = result.error.expect("error");
        assert_eq!(error.code, "invalid_args", "{op}");
        assert!(
            error
                .message
                .contains("Chet author provenance is host-owned"),
            "{op}: {}",
            error.message
        );
    }
    // An inbound IM message is an ordinary send with its provenance; it cannot
    // carry the marker either.
    let mut im = json!({"group_id":group.group_id,"by":"user","text":"fixture",
        "to":["user"],"message_mode":"send","transport":"im"});
    im.as_object_mut()
        .expect("object")
        .insert(marker.into(), json!("forged"));
    let error = call_unchecked(&home, "send", im)
        .error
        .expect("IM cannot inject provenance");
    assert_eq!(error.code, "invalid_args");
    assert!(
        error
            .message
            .contains("Chet author provenance is host-owned"),
        "{}",
        error.message
    );
    let mut raw = cccc_contracts::Event::new("chat.message", &group.group_id);
    raw.by = "worker".into();
    raw.data.insert(marker.into(), json!("forged"));
    assert!(!call_unchecked(&home, "event_append", json!({"event":raw})).ok);
    raw.data.remove(marker);
    raw.by = cccc_core::actors::CHET_SERVICE_PRINCIPAL.into();
    assert!(!call_unchecked(&home, "event_append", json!({"event":raw})).ok);
    raw.by = "worker".into();
    raw.data.insert(
        "to".into(),
        json!([cccc_core::actors::CHET_SERVICE_PRINCIPAL]),
    );
    raw.data.insert("message_mode".into(), json!("send"));
    raw.data
        .insert("text".into(), json!("not a normalized Chet message"));
    let direct = call_unchecked(&home, "event_append", json!({"event":raw}));
    assert!(!direct.ok, "raw appends must not bypass Chet admission");
    assert_eq!(direct.error.expect("error").code, "invalid_args");
    let after = ledger::read_all(&groups.ledger_path(&group.group_id).expect("path"))
        .expect("ledger")
        .len();
    assert_eq!(after, before, "rejected attempts append no events");
}

fn message_write(op: &str) -> Option<cccc_contracts::message_args::MessageWrite> {
    use cccc_contracts::message_args::MessageWrite;
    Some(match op {
        "send" => MessageWrite::Send,
        "reply" => MessageWrite::Reply,
        "send_files" => MessageWrite::SendFiles,
        "tracked_send" => MessageWrite::TrackedSend,
        "send_cross_group" => MessageWrite::SendCrossGroup,
        _ => return None,
    })
}

#[test]
fn chet_author_is_durable_without_growing_voice_request_index() {
    use cccc_core::{actors, voice_notifications as voice};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path().join("source")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("original", "").expect("group");
    let mut worker = cccc_contracts::Actor::new("worker");
    worker.runtime = cccc_contracts::ActorRuntime::WebModel;
    group.actors.push(worker);
    groups.save(&group).expect("save");
    let origin = "a".repeat(32);
    voice::register_origin(
        &home,
        &origin,
        "generation",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("register origin");
    let mut first = None;
    for index in 0..32 {
        let sent = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"user","to":["worker"],
            "message_mode":"send","text":format!("Chet message {index}"),"_voice_origin":origin}),
        );
        assert!(sent.ok, "Chet send");
        if first.is_none() {
            first = Some(
                serde_json::from_value::<cccc_contracts::Event>(sent.result["event"].clone())
                    .expect("event"),
            );
        }
    }
    let event = first.expect("first event");
    assert!(actors::is_chet_message(&home, &group, &event).expect("verified"));
    assert!(
        !event.data.contains_key(voice::ORIGIN_ARG),
        "secret origin is never written to the ledger"
    );
    assert_eq!(
        event.data[cccc_contracts::chet::GROUP_AUTHOR_GENERATION_FIELD],
        cccc_core::connect_groups::generation(&group)
    );
    let state: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/codex_voice/notifications.json"))
            .expect("notification state"),
    )
    .expect("JSON");
    assert_eq!(
        state["requests"].as_object().expect("requests").len(),
        0,
        "ordinary service messages do not exhaust the Voice request index"
    );
    let mut forged = event.clone();
    forged.id = "not-in-ledger".into();
    assert!(!actors::is_chet_message(&home, &group, &forged).expect("source check"));

    let (copy, _, _) = cccc_core::group_copy::export(&groups, &group.group_id).expect("export");
    let other_home = HomeLayout::from_path(temp.path().join("target")).expect("target home");
    let other_groups = GroupStore::new(other_home.clone()).expect("target groups");
    let imported = cccc_core::group_copy::import(&other_groups, &copy, "", "").expect("import");
    assert_eq!(
        imported.group_id, group.group_id,
        "same ID can be reused in another home"
    );
    let imported_group = other_groups
        .load(&imported.group_id)
        .expect("imported group");
    assert_ne!(
        cccc_core::connect_groups::generation(&imported_group),
        cccc_core::connect_groups::generation(&group)
    );
    let imported_event = ledger::find_event(
        &other_groups
            .ledger_path(&imported.group_id)
            .expect("ledger"),
        &event.id,
    )
    .expect("read")
    .expect("copied event");
    assert!(
        !actors::is_chet_message(&other_home, &imported_group, &imported_event)
            .expect("stale source")
    );
    let reply = call_unchecked(
        &other_home,
        "reply",
        json!({"group_id":imported.group_id,"by":"worker",
        "reply_to":event.id,"text":"reply"}),
    );
    assert!(
        !reply.ok,
        "copied service events cannot silently route a default reply"
    );
    assert!(reply.error.expect("error").message.contains("@chet"));
}

#[test]
fn cross_group_retry_does_not_copy_local_chet_provenance() {
    use cccc_contracts::chet::GROUP_AUTHOR_GENERATION_FIELD as marker;
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut source = groups.create("source", "").expect("source");
    let mut sender = cccc_contracts::Actor::new("worker");
    sender.runtime = cccc_contracts::ActorRuntime::WebModel;
    source.actors.push(sender);
    groups.save(&source).expect("save source");
    let mut target = groups.create("target", "").expect("target");
    let mut recipient = cccc_contracts::Actor::new("peer");
    recipient.runtime = cccc_contracts::ActorRuntime::WebModel;
    target.actors.push(recipient);
    groups.save(&target).expect("save target");
    // Replaying a source event must not promote source-local metadata into a
    // different Group, even if old or imported ledger data contained it.
    let mut source_event = cccc_contracts::Event::new("chat.message", &source.group_id);
    source_event.by = "worker".into();
    source_event.data = json!({"text":"relay", "to":["user"], "message_mode":"send",
        "client_id":"relay-key", "dst_group_id":target.group_id, "dst_to":["peer"],
        "dst_message_mode":"send"})
    .as_object()
    .expect("data")
    .clone();
    source_event
        .data
        .insert(marker.into(), json!("source-generation"));
    ledger::append(
        &groups.ledger_path(&source.group_id).expect("source ledger"),
        &source_event,
    )
    .expect("fixture source");
    let sent = call(
        &home,
        "send_cross_group",
        json!({"group_id":source.group_id,
        "dst_group_id":target.group_id,"by":"worker","text":"relay",
        "to":["peer"],"client_id":"relay-key"}),
    );
    assert!(sent.ok, "relay retry must succeed: {sent:?}");
    assert!(sent.result["dst_event"]["data"].get(marker).is_none());
}

#[test]
fn chet_cross_group_relay_is_rejected_before_side_effects() {
    use cccc_core::voice_notifications as voice;
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut source = groups.create("source", "").expect("source");
    let mut worker = cccc_contracts::Actor::new("worker");
    worker.runtime = cccc_contracts::ActorRuntime::WebModel;
    source.actors.push(worker);
    groups.save(&source).expect("save source");
    let mut target = groups.create("target", "").expect("target");
    let mut peer = cccc_contracts::Actor::new("peer");
    peer.runtime = cccc_contracts::ActorRuntime::WebModel;
    target.actors.push(peer);
    groups.save(&target).expect("save target");
    let origin = "f".repeat(32);
    voice::register_origin(
        &home,
        &origin,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("register origin");

    // An already delivered legacy relay must still reconcile by client_id.
    let legacy = json!({"group_id":source.group_id,"dst_group_id":target.group_id,
        "by":"user","to":["peer"],"text":"Existing relay","message_mode":"send",
        "client_id":"legacy-relay"});
    let sent = call(&home, "send_cross_group", legacy.clone());
    let mut retry = legacy;
    retry["_voice_origin"] = json!(origin);
    let replay = call(&home, "send_cross_group", retry.clone());
    assert_eq!(replay.result["duplicate"], true);
    assert_eq!(
        replay.result["dst_event"]["id"],
        sent.result["dst_event"]["id"]
    );
    retry["operation"] = json!("send_cross_group");
    let preflight_replay = call(&home, "message_upload_preflight", retry);
    assert_eq!(preflight_replay.result["ready"], false);
    assert_eq!(preflight_replay.result["duplicate"], true);

    // A routed reply follows the same relay path, despite starting as reply.
    let inbound = call(
        &home,
        "send_cross_group",
        json!({"group_id":source.group_id,
        "dst_group_id":target.group_id,"by":"worker","to":["peer"],"text":"Question",
        "message_mode":"send"}),
    );
    let inbound_id = inbound.result["dst_event"]["id"]
        .as_str()
        .expect("inbound id");
    let path = |id: &str| groups.ledger_path(id).expect("ledger path");
    // A source-only legacy relay is not a completed result: it must not be
    // resumed with the old user attribution when Chet retries it.
    let mut partial = cccc_contracts::Event::new("chat.message", &source.group_id);
    partial.by = "user".into();
    partial.data = json!({"text":"Interrupted", "to":["user"], "message_mode":"send",
        "client_id":"partial-relay", "dst_group_id":target.group_id,
        "dst_to":["peer"], "dst_message_mode":"send"})
    .as_object()
    .expect("data")
    .clone();
    ledger::append(&path(&source.group_id), &partial).expect("partial source");
    let before_source = ledger::read_all(&path(&source.group_id))
        .expect("source ledger")
        .len();
    let before_target = ledger::read_all(&path(&target.group_id))
        .expect("target ledger")
        .len();
    let new_relay = json!({"group_id":source.group_id,"dst_group_id":target.group_id,
        "by":"user","to":["peer"],"text":"New relay","message_mode":"send",
        "_voice_origin":origin});
    let mut upload = new_relay.clone();
    upload["operation"] = json!("send_cross_group");
    upload["has_attachments"] = json!(true);
    let mut files = new_relay.clone();
    files["paths"] = json!(["must-not-be-read.txt"]);
    let routed_reply = json!({"group_id":target.group_id,"by":"user",
        "reply_to":inbound_id,"text":"Answer","_voice_origin":origin});
    let mut routed_upload = routed_reply.clone();
    routed_upload["operation"] = json!("reply");
    routed_upload["has_attachments"] = json!(true);
    let mut incomplete = new_relay.clone();
    incomplete["client_id"] = json!("partial-relay");
    let mut incomplete_upload = incomplete.clone();
    incomplete_upload["operation"] = json!("send_cross_group");
    incomplete_upload["has_attachments"] = json!(true);
    for (op, args) in [
        ("send_cross_group", new_relay),
        ("message_upload_preflight", upload),
        ("send_files", files),
        ("reply", routed_reply),
        ("message_upload_preflight", routed_upload),
        ("send_cross_group", incomplete),
        ("message_upload_preflight", incomplete_upload),
    ] {
        let result = call_unchecked(&home, op, args);
        assert!(!result.ok, "{op} must reject a Chet relay");
        let error = result.error.expect("error");
        assert_eq!(error.code, "chet_cross_group_unsupported", "{op}");
        assert!(
            error.message.contains("group_id"),
            "{op}: {}",
            error.message
        );
    }
    assert_eq!(
        ledger::read_all(&path(&source.group_id))
            .expect("source ledger")
            .len(),
        before_source
    );
    assert_eq!(
        ledger::read_all(&path(&target.group_id))
            .expect("target ledger")
            .len(),
        before_target
    );

    let direct = call(
        &home,
        "send",
        json!({"group_id":target.group_id,"by":"user",
        "to":["peer"],"text":"Direct","message_mode":"send","_voice_origin":origin}),
    );
    assert_eq!(
        direct.result["event"]["by"],
        cccc_core::actors::CHET_SERVICE_PRINCIPAL
    );
    let response = call(
        &home,
        "reply",
        json!({"group_id":target.group_id,"by":"peer",
        "reply_to":direct.result["event"]["id"],"text":"Received"}),
    );
    assert_eq!(
        response.result["event"]["data"]["to"],
        json!([cccc_core::actors::CHET_SERVICE_PRINCIPAL])
    );
}

#[test]
fn chet_handoffs_are_tracked_for_voice_only_while_the_recording_lease_is_active() {
    use cccc_core::{voice_notifications as voice, voice_recording_lease as lease};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("Voice handoff", "").expect("group");
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    group.actors.push(actor);
    groups.save(&group).expect("save");
    let token = "4".repeat(32);
    voice::register_origin(
        &home,
        &token,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("origin");
    let send = |text: &str| {
        call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"user",
        "to":["worker"],"message_mode":"send","text":text,"_voice_origin":token}),
        )
    };
    let before = send("Before call");
    let acquired = lease::update(
        &home,
        "",
        "",
        &json!({"action":"acquire",
        "owner_id":"codex-voice:fixture","capture_mode":"codex_voice",
        "dispatch_target":"codex_voice","by":"user"}),
    )
    .expect("lease");
    let during = send("During call");
    let lease_id = acquired["lease_id"].as_str().expect("lease id");
    assert!(lease::release(&home, "", "codex-voice:fixture", lease_id).expect("release"));
    // A slow Voice turn that sends only after release is indistinguishable
    // from a text turn; it remains a Group message without Voice tracking.
    let after = send("After call");
    let state: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/codex_voice/notifications.json")).expect("state"),
    )
    .expect("JSON");
    let requests = state["requests"].as_object().expect("requests");
    assert_eq!(requests.len(), 1);
    assert!(
        requests
            .values()
            .any(|request| request["source"]["event_id"] == during.result["event"]["id"])
    );
    assert!(
        !requests
            .values()
            .any(|request| request["source"]["event_id"] == before.result["event"]["id"])
    );
    assert!(
        !requests
            .values()
            .any(|request| request["source"]["event_id"] == after.result["event"]["id"])
    );
}

#[test]
fn chet_connect_operations_refuse_to_impersonate_the_user() {
    use cccc_core::voice_notifications as voice;
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("Connect source", "").expect("group");
    let path = groups.ledger_path(&group.group_id).expect("ledger");
    let origin = "3".repeat(32);
    voice::register_origin(
        &home,
        &origin,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("register origin");
    let mut incoming = cccc_contracts::Event::new("chat.message", &group.group_id);
    incoming.by = "connect:remote".into();
    incoming.data.insert("connect_message".into(), json!({}));
    ledger::append(&path, &incoming).expect("incoming fixture");
    let before = ledger::read_all(&path).expect("ledger").len();
    let args = json!({"group_id":group.group_id,"by":"user","_voice_origin":origin,
        "instance_id":"remote","target_group_id":"target","client_id":"id",
        "to":["peer"],"message_mode":"send","text":"Question"});
    let mut files = args.clone();
    files["paths"] = json!(["must-not-be-read.txt"]);
    let reply = json!({"group_id":group.group_id,"by":"user","_voice_origin":origin,
        "reply_to":incoming.id,"text":"Answer"});
    let mut upload = reply.clone();
    upload["operation"] = json!("reply");
    upload["has_attachments"] = json!(true);
    for (op, payload) in [
        ("connect_send", args),
        ("connect_send_files", files),
        ("reply", reply),
        ("message_upload_preflight", upload),
    ] {
        let result = call_unchecked(&home, op, payload);
        assert!(!result.ok, "{op} must not impersonate user");
        let error = result.error.expect("error");
        assert_eq!(error.code, "chet_cross_group_unsupported", "{op}");
        assert!(error.message.contains("Connect"), "{op}: {}", error.message);
    }
    assert_eq!(ledger::read_all(&path).expect("ledger").len(), before);
}

#[test]
fn chet_web_admission_is_checked_without_storing_the_host_marker() {
    use cccc_core::actors::{CHET_SERVICE_PRINCIPAL, CHET_WEB_ADMISSION_ARG};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Chet Web admission", "").expect("group");
    let make = |op: &str, allowed: bool| {
        let mut args = json!({"group_id":group.group_id,"by":"user","to":["@chet"],
            "text":"Please help","message_mode":"send"})
        .as_object()
        .expect("args")
        .clone();
        args.insert(CHET_WEB_ADMISSION_ARG.into(), json!(allowed));
        if op == "message_upload_preflight" {
            args.insert("operation".into(), json!("send"));
            args.insert("has_attachments".into(), json!(true));
        }
        DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        }
    };
    for op in ["message_upload_preflight", "send"] {
        let result = cccc_daemon::handle_request(&home, &make(op, false));
        assert!(!result.ok, "group-only Web access cannot address Chet");
    }
    let allowed = cccc_daemon::handle_request(&home, &make("send", true));
    assert!(allowed.ok, "{allowed:?}");
    assert_eq!(
        allowed.result["event"]["data"]["to"],
        json!([CHET_SERVICE_PRINCIPAL])
    );
    assert!(
        allowed.result["event"]["data"]
            .get(CHET_WEB_ADMISSION_ARG)
            .is_none()
    );
}

#[test]
fn voice_origin_is_registered_at_canonical_send_and_reply_without_public_metadata() {
    use cccc_core::{voice_notifications as voice, voice_recording_lease as lease};
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Voice source", "").expect("group");
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    group.actors.push(actor);
    store.save(&group).expect("save");
    let token = "b".repeat(32);
    voice::register_origin(
        &home,
        &token,
        "analyst",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("origin");
    // Chet's handoffs are tracked for Voice only while a call owns the
    // recording lease; its text-only Group messages use the Group receiver.
    let acquired = lease::update(
        &home,
        "",
        "",
        &json!({"action":"acquire","owner_id":"codex-voice:fixture",
            "capture_mode":"codex_voice","dispatch_target":"codex_voice","by":"user"}),
    )
    .expect("call lease");
    let first = call(
        &home,
        "send",
        json!({"group_id":group.group_id,"by":"user","to":["worker"],"text":"Investigate","message_mode":"send","_voice_origin":token}),
    );
    let correction = call(
        &home,
        "reply",
        json!({"group_id":group.group_id,"by":"user","to":["worker"],"text":"Also inspect the second case","reply_to":first.result["event"]["id"],"_voice_origin":token}),
    );
    for source in [&first, &correction] {
        assert!(
            source.result["event"]["data"]
                .get("_voice_origin")
                .is_none()
        );
        for text in ["Received", "Finished"] {
            call(
                &home,
                "reply",
                json!({"group_id":group.group_id,"by":"worker","to":["user"],"text":text,"reply_to":source.result["event"]["id"]}),
            );
        }
    }
    voice::scan(&home).expect("scan canonical replies");
    assert_eq!(voice::snapshot(&home).expect("snapshot").messages.len(), 4);
    assert!(
        lease::release(
            &home,
            "",
            "codex-voice:fixture",
            acquired["lease_id"].as_str().expect("lease ID"),
        )
        .expect("release")
    );
    let ledger =
        ledger::read_all(&store.ledger_path(&group.group_id).expect("path")).expect("ledger");
    assert!(
        !serde_json::to_string(&ledger)
            .expect("JSON")
            .contains(&token)
    );
}

#[test]
fn duplicate_client_id_returns_the_original_event() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = call(&home, "group_create", json!({"title":"idempotency"}));
    let group_id = group.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"lead","by":"user"}),
    );
    let args = json!({
        "group_id":group_id,
        "by":"user",
        "to":["lead"],
        "text":"only once",
        "message_mode":"send",
        "client_id":"client-1"
    });

    let first = call(&home, "send", args.clone());
    let second = call(&home, "send", args);
    let tail = call(
        &home,
        "ledger_tail",
        json!({"group_id":group_id,"kind":"chat","limit":20}),
    );

    assert_eq!(first.result["event"]["id"], second.result["event"]["id"]);
    assert_eq!(second.result["duplicate"], true);
    assert_eq!(tail.result["events"].as_array().map(Vec::len), Some(1));
}

#[test]
fn directed_message_wakes_an_explicitly_stopped_actor_and_delivers_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = call(&home, "group_create", json!({"title":"offline replay"}));
    let group_id = group.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":project,"by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runtime":"custom",
            "submit":"newline",
            "command":["sh","-c","stty -echo; IFS= read -r preamble; IFS= read -r message; printf 'PREAMBLE:%s\\nRECEIVED:%s' \"$preamble\" \"$message\"; sleep 2"],
            "enabled":false,
            "by":"user"
        }),
    );
    let sent = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake delivery","message_mode":"send"}),
    );
    assert_eq!(sent.result["message_mode"], "send");
    let sent_event_id = sent.result["event"]["id"].as_str().expect("sent event id");
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(actors.result["actors"][0]["enabled"], true);

    assert_eq!(
        wait_for_accepted_delivery(&home, group_id, sent_event_id),
        1
    );
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);
    let _ = call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
}

fn wait_for_accepted_delivery(home: &HomeLayout, group_id: &str, source_event_id: &str) -> usize {
    let ledger_path = GroupStore::new(home.clone())
        .expect("store")
        .ledger_path(group_id)
        .expect("ledger path");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let accepted = ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .filter(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == source_event_id
                    && event.data["actor_id"] == "peer1"
                    && event.data["state"] == "accepted"
            })
            .count();
        if accepted > 0 {
            return accepted;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "runtime did not accept delivery for {source_event_id}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = call_unchecked(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn call_unchecked(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
