use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

#[test]
fn exact_completion_is_replayable_but_mismatched_receipts_are_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = setup(&home);
    let turn = next_turn(&home, &group_id);
    let args = completion_args(&group_id, &turn, "delivery-a");

    let first = call(&home, "runtime_complete_turn", args.clone());
    let restarted = HomeLayout::from_path(home.root().to_owned()).expect("restart home");
    let replay = call(&restarted, "runtime_complete_turn", args.clone());
    assert_eq!(replay.result, first.result);
    assert_eq!(replay.result["delivery_id"], "delivery-a");

    for mismatch in [
        json!({"delivery_id":"delivery-b"}),
        json!({"event_ids":["different-event"]}),
        json!({"status":"partial"}),
    ] {
        let mut changed = args.clone();
        changed.extend(mismatch.as_object().cloned().expect("object"));
        let rejected = raw_call(&home, "runtime_complete_turn", changed);
        assert!(!rejected.ok);
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("completion_conflict")
        );
    }
}

#[test]
fn browser_observations_do_not_overwrite_runtime_delivery_or_create_mail_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = setup(&home);
    let turn = next_turn(&home, &group_id);
    let event_ids = turn["event_ids"].clone();
    let event_id = event_ids[0].as_str().expect("event id").to_owned();
    let base = json!({
        "group_id":group_id,
        "actor_id":"web1",
        "by":"web1",
        "turn_id":turn["turn_id"],
        "event_ids":event_ids,
        "delivery_id":"delivery-retry"
    });

    for (state, detail) in [("failed", "send unavailable"), ("submitting", "")] {
        let mut args = base.as_object().cloned().expect("args");
        args.insert(
            "browser_delivery".into(),
            json!({"state":state,"detail":detail,"provider":"chatgpt"}),
        );
        let recorded = call(&home, "web_model_browser_delivery_record", args);
        assert_eq!(
            recorded.result["event"]["kind"],
            format!("web_model.browser_delivery.{state}")
        );
    }

    let statuses = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[event_id]}),
    );
    assert_eq!(
        // next_turn used the native MCP pull path, which already accepted
        // this source. A later browser observation cannot undo that handoff.
        statuses.result["statuses"][&event_id]["obligation_status"]["web1"]["delivery_state"],
        "accepted"
    );
    assert!(
        statuses.result["statuses"][&event_id]
            .get("read_status")
            .is_none()
    );
    let unread = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
    );
    assert_eq!(unread.result["messages"], json!([]));
}

#[cfg(unix)]
#[test]
fn precommit_ledger_failure_does_not_create_a_completion_receipt() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = setup(&home);
    let turn = next_turn(&home, &group_id);
    let args = completion_args(&group_id, &turn, "delivery-failure");
    let ledger = GroupStore::new(home.clone())
        .expect("store")
        .ledger_path(&group_id)
        .expect("ledger");
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o444)).expect("lock ledger");
    let failed = raw_call(&home, "runtime_complete_turn", args.clone());
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o644))
        .expect("unlock ledger");
    assert!(!failed.ok);

    let completed = call(&home, "runtime_complete_turn", args);
    assert_eq!(completed.result["delivery_id"], "delivery-failure");
}

pub(crate) fn setup(home: &HomeLayout) -> String {
    let created = call(home, "group_create", json!({"title":"completion"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","by":"user"}),
    );
    call(
        home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    call(
        home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"work","message_mode":"send"}),
    );
    group_id
}

pub(crate) fn next_turn(home: &HomeLayout, group_id: &str) -> Value {
    call(
        home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1"}),
    )
    .result["turn"]
        .clone()
}

pub(crate) fn completion_args(
    group_id: &str,
    turn: &Value,
    delivery_id: &str,
) -> Map<String, Value> {
    json!({
        "group_id":group_id,
        "actor_id":"web1",
        "by":"web1",
        "turn_id":turn["turn_id"],
        "event_ids":turn["event_ids"],
        "status":"done",
        "delivery_id":delivery_id
    })
    .as_object()
    .cloned()
    .expect("args")
}

pub(crate) fn call(home: &HomeLayout, op: &str, args: impl Into<Value>) -> DaemonResponse {
    let response = raw_call(
        home,
        op,
        args.into().as_object().cloned().unwrap_or_else(Map::new),
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

pub(crate) fn raw_call(home: &HomeLayout, op: &str, args: Map<String, Value>) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        },
    )
}

#[test]
fn missing_target_and_presend_failure_keep_read_or_unread_mail_retryable() {
    for read_mail in [false, true] {
        let temp = tempfile::tempdir().expect("isolated mail state");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let created = call(&home, "group_create", json!({"title":"browser retry"}));
        let gid = created.result["group"]["group_id"].as_str().expect("group");
        call(
            &home,
            "actor_add",
            json!({"group_id":gid,"actor_id":"web","runtime":"web_model",
            "by":"user","env":{"CCCC_WEB_MODEL_DELIVERY_MODE":"browser"}}),
        );
        call(
            &home,
            "actor_start",
            json!({"group_id":gid,"actor_id":"web","by":"user"}),
        );
        let source = call(
            &home,
            "send",
            json!({"group_id":gid,"by":"user","to":["web"],
            "text":"retain this report","message_mode":"mail"}),
        );
        let id = source.result["event"]["id"].as_str().expect("source");
        let actor = json!({"group_id":gid,"actor_id":"web","by":"web"});
        if read_mail {
            call(&home, "inbox_read", actor.clone());
        }
        let promoted = call(
            &home,
            "message_deliver",
            json!({"group_id":gid,"by":"user",
            "source_event_id":id,"actor_ids":["web"]}),
        );
        assert_eq!(
            promoted.result["delivery_state"], "failed",
            "missing target retained a claim"
        );
        let path = GroupStore::new(home.clone())
            .expect("store")
            .ledger_path(gid)
            .expect("ledger");
        let events = cccc_core::ledger::read_all(&path).expect("events");
        let failure = events
            .iter()
            .rev()
            .find(|event| event.kind == "runtime.delivery" && event.data["source_event_id"] == id)
            .expect("failed promotion");
        assert_eq!(failure.data["state"], "failed");
        assert!(
            failure.data["reason"]
                .as_str()
                .is_some_and(|text| text.contains("target"))
        );
        let home = HomeLayout::from_path(home.root().to_path_buf()).expect("reopen state");
        cccc_core::web_model_connectors::save_browser_target(
            &home,
            gid,
            "web",
            Some(json!({"kind":"existing_chat","url":"https://chatgpt.com/c/test-callback"})),
        )
        .expect("configure callback");
        let mut wait = actor.clone();
        wait["transport"] = json!("web_model_browser");
        let reserved = call(&home, "runtime_wait_next_turn", wait.clone());
        assert_eq!(reserved.result["status"], "work_available");
        let turn = &reserved.result["turn"];
        assert_eq!(turn["event_ids"], json!([id]));
        let mut record = json!({"group_id":gid,"actor_id":"web","by":"web",
            "turn_id":turn["turn_id"],"event_ids":[id],"delivery_id":"pre-submit-test",
            "browser_delivery":{"state":"failed","detail":"not_sent_chat_busy","provider":"chatgpt"}});
        call(&home, "web_model_browser_delivery_record", record.clone());
        let retry = call(&home, "runtime_wait_next_turn", wait.clone());
        assert_eq!(
            retry.result["status"], "work_available",
            "failed handoff owns the turn"
        );
        assert_eq!(
            retry.result["turn"]["event_ids"],
            json!([id]),
            "original mail lost"
        );
        assert_eq!(retry.result["turn"]["turn_id"], turn["turn_id"]);
        record["browser_delivery"]["state"] = json!("submitted");
        record["browser_delivery"]["detail"] = json!("message_echo");
        let submitted = call(&home, "web_model_browser_delivery_record", record.clone());
        record["browser_delivery"]
            .as_object_mut()
            .expect("delivery")
            .remove("detail");
        let reconciled = call(&home, "web_model_browser_delivery_record", record);
        assert_eq!(
            reconciled.result["event"], submitted.result["event"],
            "completion reconciliation must reuse the original browser receipt"
        );
        call(
            &home,
            "runtime_complete_turn",
            json!({"group_id":gid,"actor_id":"web","by":"web",
            "turn_id":turn["turn_id"],"event_ids":[id],"delivery_id":"pre-submit-test","status":"done"}),
        );
        assert_eq!(
            call(&home, "runtime_wait_next_turn", wait).result["status"],
            "idle"
        );
        let events = cccc_core::ledger::read_all(&path).expect("final ledger");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "chat.message")
                .count(),
            1
        );
        let unread = call(&home, "inbox_peek", actor);
        let mail = unread.result["messages"].as_array().expect("mail");
        assert_eq!(
            mail.len(),
            usize::from(!read_mail),
            "delivery changed read state"
        );
        if !read_mail {
            assert_eq!(mail[0]["id"], id);
        }
    }
}

#[test]
fn late_browser_failure_cannot_requeue_an_accepted_or_ambiguous_report() {
    for (observed, expected) in [("submitted", "accepted"), ("ambiguous", "ambiguous")] {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group_id = setup(&home);
        let wait = json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"});
        let turn = call(&home, "runtime_wait_next_turn", wait.clone()).result["turn"].clone();
        let event_id = turn["event_ids"][0].as_str().expect("source event");
        let record = json!({"group_id":group_id,"actor_id":"web1","by":"web1",
            "turn_id":turn["turn_id"],"event_ids":turn["event_ids"],"delivery_id":"late-observation",
            "browser_delivery":{"state":observed,"detail":"current browser outcome","provider":"chatgpt"}});
        call(&home, "web_model_browser_delivery_record", record.clone());
        let mut late_failure = record;
        late_failure["browser_delivery"] = json!({"state":"failed","detail":"late pre-submit error from an earlier attempt","provider":"chatgpt"});
        call(
            &home,
            "web_model_browser_delivery_record",
            late_failure.clone(),
        );
        let statuses = call(
            &home,
            "ledger_statuses",
            json!({"group_id":group_id,"event_ids":[event_id]}),
        );
        assert_eq!(
            statuses.result["statuses"][event_id]["obligation_status"]["web1"]["delivery_state"],
            expected,
            "a delayed failure downgraded {observed}, enabling a duplicate delivery"
        );
        assert_eq!(
            call(&home, "runtime_wait_next_turn", wait.clone()).result["status"],
            "turn_in_progress",
            "a delayed failure released a batch that may already be delivered"
        );
        call(
            &home,
            "runtime_complete_turn",
            completion_args(&group_id, &turn, "late-observation"),
        );
        call(&home, "web_model_browser_delivery_record", late_failure);
        assert_eq!(
            call(&home, "runtime_wait_next_turn", wait).result["status"],
            "idle",
            "a completed report was scheduled again by a late failure"
        );
    }
}
