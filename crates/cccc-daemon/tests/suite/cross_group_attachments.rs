use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, blobs, ledger};
use serde_json::{Map, Value, json};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    origin: String,
    receiver: String,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let origin = store.create("origin", "").expect("origin").group_id;
    let receiver = store.create("receiver", "").expect("receiver").group_id;
    call(
        &home,
        "actor_add",
        json!({"group_id":receiver,"actor_id":"receiver","by":"user"}),
    );
    // A paused Group records messages without starting a runtime.
    store
        .mutate(&receiver, |group| {
            group.state = cccc_contracts::GroupState::Paused;
            Ok(())
        })
        .expect("pause fixture");
    Fixture {
        _temp: temp,
        home,
        store,
        origin,
        receiver,
    }
}

impl Fixture {
    fn count(&self, group_id: &str) -> usize {
        ledger::read_all(&self.store.ledger_path(group_id).expect("ledger path"))
            .expect("ledger")
            .len()
    }
}

#[test]
fn cross_group_reply_delivers_its_files_to_both_groups_and_settles_the_obligation() {
    let f = fixture();
    let sent = call(
        &f.home,
        "send_cross_group",
        json!({
            "group_id":f.origin,"dst_group_id":f.receiver,"by":"user",
            "to":["receiver"],"text":"question","message_mode":"request_reply"
        }),
    );
    let parent = sent.result["dst_event"]["id"].clone();
    let blob = blobs::store(&f.home, &f.receiver, b"png bytes").expect("blob");
    assert!(blobs::resolve(&f.home, &f.origin, &blob.path).is_err());

    let reply = call(
        &f.home,
        "reply",
        json!({
            "group_id":f.receiver,"by":"receiver","reply_to":parent,"text":"see image",
            "attachments":[{"path":blob.path,"kind":"file","mime_type":"image/png","title":"shot.png"}]
        }),
    );

    let expected = json!([{
        "kind":"image","path":blob.path,"title":"shot.png","mime_type":"image/png",
        "bytes":9,"sha256":blob.sha256
    }]);
    assert_eq!(reply.result["event"]["data"]["attachments"], expected);
    assert_eq!(reply.result["dst_event"]["data"]["attachments"], expected);
    assert_eq!(reply.result["dst_event"]["group_id"], f.origin);
    let delivered = blobs::resolve(&f.home, &f.origin, &blob.path).expect("readable in origin");
    assert_eq!(std::fs::read(delivered).expect("read"), b"png bytes");
    let status = call(
        &f.home,
        "ledger_statuses",
        json!({"group_id":f.receiver,"event_ids":[parent]}),
    );
    assert_eq!(
        status.result["statuses"][parent.as_str().expect("parent")]["obligation_status"]["receiver"]
            ["replied"],
        true
    );
}

#[test]
fn invalid_cross_group_attachments_write_nothing_in_either_group() {
    let f = fixture();
    let own = blobs::store(&f.home, &f.origin, b"origin bytes").expect("own blob");
    let corrupt = blobs::store(&f.home, &f.origin, b"before change").expect("corrupt fixture");
    std::fs::write(
        blobs::resolve(&f.home, &f.origin, &corrupt.path).expect("source path"),
        b"after changes",
    )
    .expect("change stored source");
    let foreign = blobs::store(&f.home, &f.receiver, b"receiver bytes").expect("foreign");
    // A sparse file just over the limit, stored as a content-addressed name.
    let oversized = "f".repeat(64);
    let file = std::fs::File::create(
        f.store
            .state_dir(&f.origin)
            .expect("state")
            .join("blobs")
            .join(&oversized),
    )
    .expect("oversized");
    file.set_len(100 * 1024 * 1024 + 1).expect("size");
    let before = (f.count(&f.origin), f.count(&f.receiver));

    for (attachment, code) in [
        (json!({"path":corrupt.path}), "invalid_attachments"),
        (json!({"path":foreign.path}), "invalid_attachments"),
        (
            json!({"path":own.path,"sha256":"0".repeat(64)}),
            "invalid_attachments",
        ),
        (json!({"path":own.path,"bytes":1}), "invalid_attachments"),
        (
            json!({"path":format!("state/blobs/{oversized}")}),
            "attachment_too_large",
        ),
    ] {
        let rejected = call_raw(
            &f.home,
            "send_cross_group",
            json!({
                "group_id":f.origin,"dst_group_id":f.receiver,"by":"user","to":["receiver"],
                "text":"files","message_mode":"send","attachments":[attachment]
            }),
        );
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some(code),
            "{attachment}"
        );
        assert_eq!((f.count(&f.origin), f.count(&f.receiver)), before);
    }
    assert!(blobs::resolve(&f.home, &f.receiver, &own.path).is_err());
    assert!(blobs::resolve(&f.home, &f.receiver, &oversized).is_err());
}

#[test]
fn a_retried_cross_group_send_completes_its_files_once() {
    let f = fixture();
    let blob = blobs::store(&f.home, &f.origin, b"report").expect("blob");
    let args = json!({
        "group_id":f.origin,"dst_group_id":f.receiver,"by":"user","to":["receiver"],
        "text":"report","client_id":"files-once","message_mode":"send",
        "attachments":[{"path":blob.path,"mime_type":"text/plain","title":"report.txt"}]
    });
    let before = (f.count(&f.origin), f.count(&f.receiver));

    // Fail after the source audit entry: the destination ledger cannot be written.
    let path = f.store.ledger_path(&f.receiver).expect("ledger path");
    let held = path.with_extension("retry-fixture");
    std::fs::rename(&path, &held).expect("hold ledger");
    std::fs::create_dir(&path).expect("block ledger");
    let pending = call_raw(&f.home, "send_cross_group", args.clone());
    std::fs::remove_dir(&path).expect("unblock ledger");
    std::fs::rename(&held, &path).expect("restore ledger");
    assert!(!pending.ok);
    assert_eq!(
        (f.count(&f.origin), f.count(&f.receiver)),
        (before.0 + 1, before.1)
    );

    // An upload retry of the interrupted send must reach the commit, not stop
    // at preflight as if the destination copy existed.
    let preflight = json!({
        "operation":"send_cross_group","group_id":f.origin,"dst_group_id":f.receiver,
        "by":"user","to":["receiver"],"message_mode":"send","client_id":"files-once",
        "has_attachments":true
    });
    assert_eq!(
        call(&f.home, "message_upload_preflight", preflight.clone()).result["ready"],
        true
    );

    let recovered = call(&f.home, "send_cross_group", args.clone());
    assert_eq!(
        recovered.result["dst_event"]["data"]["attachments"][0]["sha256"],
        blob.sha256
    );
    assert!(blobs::resolve(&f.home, &f.receiver, &blob.path).is_ok());
    let repeated = call(&f.home, "send_cross_group", args);
    assert_eq!(repeated.result["duplicate"], true);
    let replay = call(&f.home, "message_upload_preflight", preflight);
    assert_eq!(replay.result["duplicate"], true);
    assert_eq!(
        replay.result["result"]["dst_event"]["id"],
        recovered.result["dst_event"]["id"]
    );
    assert_eq!(
        (f.count(&f.origin), f.count(&f.receiver)),
        (before.0 + 1, before.1 + 1)
    );
}

#[test]
fn cross_group_invalid_scope_is_rejected_before_upload_or_source_recording() {
    let f = fixture();
    let blob = blobs::store(&f.home, &f.origin, b"scope payload").expect("blob");
    let unattached = f._temp.path().join("unattached");
    std::fs::create_dir(&unattached).expect("unattached directory");
    let args = json!({
        "group_id":f.origin,"dst_group_id":f.receiver,"by":"user","to":["receiver"],
        "text":"scope payload","message_mode":"send","path":unattached,
        "attachments":[{"path":blob.path}]
    });
    let before = (f.count(&f.origin), f.count(&f.receiver));
    let mut preflight = args.clone();
    preflight
        .as_object_mut()
        .expect("object")
        .remove("attachments");
    preflight["operation"] = json!("send_cross_group");
    preflight["has_attachments"] = json!(true);
    let mut outcomes = Vec::new();
    for (op, request) in [
        ("message_upload_preflight", preflight),
        ("send_cross_group", args),
    ] {
        let rejected = call_raw(&f.home, op, request);
        outcomes.push((
            op,
            rejected.ok,
            rejected.error.map(|error| error.code),
            (f.count(&f.origin), f.count(&f.receiver)),
            blobs::resolve(&f.home, &f.receiver, &blob.path).is_ok(),
        ));
    }
    assert!(
        outcomes.iter().all(|(_, ok, code, counts, placed)| !ok
            && code.as_deref() == Some("scope_not_attached")
            && *counts == before
            && !placed),
        "{outcomes:?}"
    );
}

#[test]
fn cross_group_unusable_destination_blob_is_rejected_without_recording_or_replacing_it() {
    let mut outcomes = Vec::new();
    for corrupt_file in [false, true] {
        let f = fixture();
        let blob = blobs::store(&f.home, &f.origin, b"original payload").expect("blob");
        let destination = f
            .store
            .state_dir(&f.receiver)
            .expect("state")
            .join("blobs")
            .join(&blob.sha256);
        std::fs::create_dir_all(destination.parent().expect("parent")).expect("blobs directory");
        if corrupt_file {
            std::fs::write(&destination, b"modified payload").expect("corrupt fixture");
        } else {
            std::fs::create_dir(&destination).expect("blocked fixture");
        }
        let before = (f.count(&f.origin), f.count(&f.receiver));
        let rejected = call_raw(
            &f.home,
            "send_cross_group",
            json!({
                "group_id":f.origin,"dst_group_id":f.receiver,"by":"user","to":["receiver"],
                "text":"original payload","message_mode":"send","attachments":[{"path":blob.path}]
            }),
        );
        outcomes.push((
            corrupt_file,
            rejected.ok,
            (f.count(&f.origin), f.count(&f.receiver)),
            before,
        ));
        if corrupt_file {
            assert_eq!(
                std::fs::read(&destination).expect("existing file"),
                b"modified payload"
            );
        } else {
            assert!(destination.is_dir());
        }
    }
    assert!(
        outcomes
            .iter()
            .all(|(_, ok, counts, before)| !ok && counts == before),
        "{outcomes:?}"
    );
}

#[test]
fn upload_preflight_accepts_cross_group_sends_and_replies_but_not_outsiders() {
    let f = fixture();
    let sent = call(
        &f.home,
        "send_cross_group",
        json!({
            "group_id":f.origin,"dst_group_id":f.receiver,"by":"user",
            "to":["receiver"],"text":"question","message_mode":"request_reply"
        }),
    );
    let parent = sent.result["dst_event"]["id"].clone();
    let send = call(
        &f.home,
        "message_upload_preflight",
        json!({
            "operation":"send_cross_group","group_id":f.origin,"dst_group_id":f.receiver,
            "by":"user","to":["receiver"],"message_mode":"send","has_attachments":true
        }),
    );
    assert_eq!(send.result["ready"], true);
    let reply = call(
        &f.home,
        "message_upload_preflight",
        json!({
            "operation":"reply","group_id":f.receiver,"by":"receiver","reply_to":parent,
            "has_attachments":true
        }),
    );
    assert_eq!(reply.result["ready"], true);
    let outsider = call_raw(
        &f.home,
        "message_upload_preflight",
        json!({
            "operation":"send_cross_group","group_id":f.origin,"dst_group_id":f.receiver,
            "by":"stranger","to":["receiver"],"message_mode":"send","has_attachments":true
        }),
    );
    assert_eq!(
        outsider.error.as_ref().map(|error| error.code.as_str()),
        Some("permission_denied")
    );
}

#[test]
fn scope_files_sent_to_another_group_arrive_without_the_source_scope() {
    let f = fixture();
    let scope = f._temp.path().join("project");
    std::fs::create_dir_all(&scope).expect("scope");
    std::fs::write(scope.join("frame.png"), b"frame").expect("file");
    call(
        &f.home,
        "attach",
        json!({"group_id":f.origin,"path":scope,"by":"user"}),
    );
    let args = json!({
        "group_id":f.origin,"dst_group_id":f.receiver,"by":"user","to":["receiver"],
        "message_mode":"send","client_id":"scope-file","paths":["frame.png"],"path":scope
    });

    let sent = call(&f.home, "send_files", args.clone());

    let delivered = &sent.result["dst_event"]["data"];
    assert_eq!(delivered["text"], "[files] frame.png");
    assert_eq!(delivered["attachments"][0]["kind"], "image");
    assert!(delivered.get("path").is_none());
    let path = delivered["attachments"][0]["path"].as_str().expect("path");
    assert_eq!(
        std::fs::read(blobs::resolve(&f.home, &f.receiver, path).expect("delivered"))
            .expect("read"),
        b"frame"
    );
    let before = (f.count(&f.origin), f.count(&f.receiver));
    assert_eq!(call(&f.home, "send_files", args).result["duplicate"], true);
    assert_eq!((f.count(&f.origin), f.count(&f.receiver)), before);
}

#[test]
fn a_local_message_claiming_another_origin_cannot_reply_into_that_group() {
    let f = fixture();
    let genuine = call(
        &f.home,
        "send_cross_group",
        json!({
            "group_id":f.receiver,"dst_group_id":f.origin,"by":"user",
            "to":["user"],"text":"genuine","message_mode":"send"
        }),
    );
    // A message the receiver kept to itself was never sent to the origin.
    let private = call(
        &f.home,
        "send",
        json!({"group_id":f.receiver,"by":"user","to":["user"],"text":"private","message_mode":"send"}),
    );
    // A plain send keeps caller-supplied provenance fields and author as data.
    let forged = call(
        &f.home,
        "send",
        json!({
            "group_id":f.origin,"by":format!("{}::user", f.receiver),"to":["user"],
            "text":"forged","message_mode":"send",
            "src_group_id":f.receiver,"src_event_id":private.result["event"]["id"]
        }),
    );
    let blob = blobs::store(&f.home, &f.origin, b"payload").expect("blob");
    let before = f.count(&f.receiver);

    for reply_to in
        [
            forged.result["event"]["id"].clone(),
            json!(call(
            &f.home,
            "send",
            json!({
                "group_id":f.origin,"by":"user","to":["user"],"text":"invented",
                "message_mode":"send","src_group_id":f.receiver,"src_event_id":"no-such-event"
            }),
        )
        .result["event"]["id"]),
        ]
    {
        let rejected = call_raw(
            &f.home,
            "reply",
            json!({
                "group_id":f.origin,"by":"user","reply_to":reply_to,"text":"leak",
                "to":[format!("{}::user", f.receiver)],
                "attachments":[{"path":blob.path}]
            }),
        );
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_recipient")
        );
    }
    assert_eq!(f.count(&f.receiver), before);
    assert!(blobs::resolve(&f.home, &f.receiver, &blob.path).is_err());

    // The genuine delivery still accepts a reply with the file.
    let reply = call(
        &f.home,
        "reply",
        json!({
            "group_id":f.origin,"by":"user","reply_to":genuine.result["dst_event"]["id"],
            "text":"answer","to":[format!("{}::user", f.receiver)],
            "attachments":[{"path":blob.path}]
        }),
    );
    assert_eq!(reply.result["dst_event"]["group_id"], f.receiver);
}

#[cfg(unix)]
#[test]
fn an_interrupted_cross_group_reply_upload_is_completed_not_replayed() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    // An agent asks, so the answer may go back as Mail.
    call(
        &f.home,
        "actor_add",
        json!({"group_id":f.origin,"actor_id":"asker","by":"user"}),
    );
    f.store
        .mutate(&f.origin, |group| {
            group.state = cccc_contracts::GroupState::Paused;
            Ok(())
        })
        .expect("pause origin");
    let sent = call(
        &f.home,
        "send_cross_group",
        json!({
            "group_id":f.origin,"dst_group_id":f.receiver,"by":"asker",
            "to":["receiver"],"text":"question","message_mode":"send"
        }),
    );
    let parent = sent.result["dst_event"]["id"].clone();
    let blob = blobs::store(&f.home, &f.receiver, b"answer file").expect("blob");
    let reply = json!({
        "group_id":f.receiver,"by":"receiver","reply_to":parent,"text":"answer",
        "message_mode":"mail","client_id":"reply-once","attachments":[{"path":blob.path}]
    });
    let preflight = json!({
        "operation":"reply","group_id":f.receiver,"by":"receiver","reply_to":parent,
        "message_mode":"mail","client_id":"reply-once","has_attachments":true
    });

    // The origin stays readable but refuses the far copy.
    let origin_ledger = f.store.ledger_path(&f.origin).expect("ledger path");
    std::fs::set_permissions(&origin_ledger, std::fs::Permissions::from_mode(0o444))
        .expect("read-only");
    if std::fs::OpenOptions::new()
        .append(true)
        .open(&origin_ledger)
        .is_ok()
    {
        eprintln!("ledger write-failure case skipped: this process bypasses file mode bits");
        return;
    }
    let pending = call_raw(&f.home, "reply", reply.clone());
    std::fs::set_permissions(&origin_ledger, std::fs::Permissions::from_mode(0o644))
        .expect("writable");
    assert!(!pending.ok);

    assert_eq!(
        call(&f.home, "message_upload_preflight", preflight.clone()).result["ready"],
        true
    );
    let recovered = call(&f.home, "reply", reply);
    assert_eq!(recovered.result["dst_event"]["group_id"], f.origin);
    assert_eq!(recovered.result["message_mode"], "mail");

    // A replay carries the reply op's result: local event, mode, far copy.
    let replay = call(&f.home, "message_upload_preflight", preflight.clone());
    assert_eq!(replay.result["duplicate"], true);
    for field in ["event", "dst_event", "message_mode"] {
        assert_eq!(
            replay.result["result"][field], recovered.result[field],
            "{field}"
        );
    }
    // Reusing the client_id for another parent conflicts, as the reply op does.
    let other = call(
        &f.home,
        "send",
        json!({"group_id":f.receiver,"by":"user","to":["user"],"text":"other","message_mode":"send"}),
    );
    let mut moved = preflight;
    moved["reply_to"] = other.result["event"]["id"].clone();
    assert_eq!(
        call_raw(&f.home, "message_upload_preflight", moved)
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("idempotency_conflict")
    );
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = call_raw(home, op, args);
    assert!(response.ok, "{op} failed: {:?}", response.error);
    response
}

fn call_raw(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
