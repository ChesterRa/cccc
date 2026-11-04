use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

fn web_instruction_fixture() -> (tempfile::TempDir, HomeLayout, String, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let instructions = workspace.join("AGENTS.md");
    std::fs::write(&instructions, "original instructions").expect("original instructions");
    let created = call(&home, "group_create", json!({"title":"instruction notice"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    let store = GroupStore::new(home.clone()).expect("store");
    let scope = cccc_core::scope::detect(&workspace).expect("workspace scope");
    cccc_core::group_scope::attach(&store, &group_id, scope).expect("attach workspace");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","by":"user"}),
    );
    store
        .mutate(&group_id, |group| {
            group.running = true;
            Ok(())
        })
        .expect("running Group");
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    (temp, home, group_id, instructions)
}

fn web_notice_turn(home: &HomeLayout, group_id: &str, transport: &str) -> Value {
    call(
        home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"next","message_mode":"send"}),
    );
    let response = call(
        home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":transport}),
    );
    assert_eq!(response.result["status"], "work_available");
    response.result["turn"].clone()
}

fn complete_web_notice_turn(home: &HomeLayout, group_id: &str, turn: &Value) {
    call(
        home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done","turn_id":turn["turn_id"],"event_ids":turn["event_ids"]}),
    );
}

#[test]
fn web_scope_change_is_monitored_before_the_next_delivery() {
    for method in ["attach", "group_use", "group_detach_scope", "actor_update"] {
        let (temp, home, group_id, _) = web_instruction_fixture();
        let previous_scope = GroupStore::new(home.clone())
            .expect("store")
            .load(&group_id)
            .expect("group")
            .active_scope_key;
        let workspace = temp.path().join("new-workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let instructions = workspace.join("AGENTS.md");
        std::fs::write(&instructions, "first version").expect("original instructions");
        let attached = call(
            &home,
            "attach",
            json!({"group_id":group_id,"by":"user","path":workspace,"activate":method == "attach"}),
        );
        if method == "group_use" {
            call(
                &home,
                "group_use",
                json!({"group_id":group_id,"by":"user","path":workspace}),
            );
        } else if method == "group_detach_scope" {
            call(
                &home,
                "group_detach_scope",
                json!({"group_id":group_id,"by":"user","scope_key":previous_scope}),
            );
        } else if method == "actor_update" {
            call(
                &home,
                "actor_update",
                json!({"group_id":group_id,"actor_id":"web1","by":"user","patch":{"default_scope_key":attached.result["scope_key"]}}),
            );
        }
        // This is the group_show projection that cccc_file consults on every
        // call, including calls made during a previously delivered turn.
        let shown = call(
            &home,
            "group_show",
            json!({"group_id":group_id,"by":"web1"}),
        );
        assert_eq!(
            shown.result["effective_scope"]["path"],
            json!(workspace.canonicalize().expect("workspace root")),
            "{method}"
        );
        std::fs::write(instructions, "changed before the next message")
            .expect("changed instructions");
        let turn = web_notice_turn(&home, &group_id, "web_model_pull");
        assert!(
            turn["coalesced_text"]
                .as_str()
                .expect("text")
                .contains("AGENTS.md changed"),
            "{method}"
        );
    }
}

#[test]
fn web_session_retains_instruction_baselines_when_switching_workspaces() {
    let (temp, home, group_id, instructions_a) = web_instruction_fixture();
    let workspace_a = instructions_a.parent().expect("first workspace");
    let workspace_b = temp.path().join("second-workspace");
    std::fs::create_dir(&workspace_b).expect("second workspace");
    let instructions_b = workspace_b.join("AGENTS.md");
    std::fs::write(&instructions_b, "first version").expect("second instructions");
    let store = GroupStore::new(home.clone()).expect("store");
    cccc_core::group_scope::attach(
        &store,
        &group_id,
        cccc_core::scope::detect(&workspace_b).expect("second scope"),
    )
    .expect("activate second");
    let first_b = web_notice_turn(&home, &group_id, "web_model_pull");
    assert!(
        !first_b["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("INSTRUCTION FILES CHANGED")
    );
    complete_web_notice_turn(&home, &group_id, &first_b);

    std::fs::write(&instructions_a, "changed while away").expect("change first");
    cccc_core::group_scope::attach(
        &store,
        &group_id,
        cccc_core::scope::detect(workspace_a).expect("first scope"),
    )
    .expect("reactivate first");
    let second_a = web_notice_turn(&home, &group_id, "web_model_pull");
    assert!(
        second_a["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("AGENTS.md changed")
    );
    complete_web_notice_turn(&home, &group_id, &second_a);

    std::fs::write(&instructions_b, "changed while away").expect("change second");
    cccc_core::group_scope::attach(
        &store,
        &group_id,
        cccc_core::scope::detect(&workspace_b).expect("second scope"),
    )
    .expect("reactivate second");
    let second_b = web_notice_turn(&home, &group_id, "web_model_pull");
    assert!(
        second_b["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("AGENTS.md changed")
    );
}

#[test]
fn pulled_web_turn_delivers_the_instruction_notice_only_once() {
    let (_temp, home, group_id, instructions) = web_instruction_fixture();
    std::fs::write(instructions, "changed before first turn").expect("changed instructions");
    let first = web_notice_turn(&home, &group_id, "web_model_pull");
    assert!(
        first["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("AGENTS.md changed")
    );
    complete_web_notice_turn(&home, &group_id, &first);
    let second = web_notice_turn(&home, &group_id, "web_model_pull");
    assert!(
        !second["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("INSTRUCTION FILES CHANGED")
    );
}

#[test]
fn browser_notice_waits_for_submission_and_keeps_later_changes() {
    let (_temp, home, group_id, instructions) = web_instruction_fixture();
    std::fs::write(&instructions, "first change").expect("first change");
    let first = web_notice_turn(&home, &group_id, "web_model_browser");
    assert!(
        first["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("AGENTS.md changed")
    );
    // This arrives after the payload was formed, and must not be swallowed by
    // the successful submission of that earlier payload.
    std::fs::write(&instructions, "second change").expect("second change");
    call(
        &home,
        "web_model_browser_delivery_record",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","turn_id":first["turn_id"],"event_ids":first["event_ids"],"delivery_id":"first","browser_delivery":{"state":"submitted"}}),
    );
    complete_web_notice_turn(&home, &group_id, &first);

    let second = web_notice_turn(&home, &group_id, "web_model_browser");
    assert!(
        second["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("AGENTS.md changed")
    );
    call(
        &home,
        "web_model_browser_delivery_record",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","turn_id":second["turn_id"],"event_ids":second["event_ids"],"delivery_id":"second","browser_delivery":{"state":"ambiguous"}}),
    );
    complete_web_notice_turn(&home, &group_id, &second);

    let third = web_notice_turn(&home, &group_id, "web_model_browser");
    assert!(
        third["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("AGENTS.md changed")
    );
    call(
        &home,
        "web_model_browser_delivery_record",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","turn_id":third["turn_id"],"event_ids":third["event_ids"],"delivery_id":"third","browser_delivery":{"state":"submitted"}}),
    );
    complete_web_notice_turn(&home, &group_id, &third);
    let fourth = web_notice_turn(&home, &group_id, "web_model_browser");
    assert!(
        !fourth["coalesced_text"]
            .as_str()
            .expect("text")
            .contains("INSTRUCTION FILES CHANGED")
    );
}

#[test]
fn web_model_actor_uses_the_structured_turn_contract_without_a_terminal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"runtime state"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","by":"user"}),
    );
    GroupStore::new(home.clone())
        .expect("group store")
        .mutate(group_id, |group| {
            group.running = true;
            Ok(())
        })
        .expect("enable structured runtime fixture");
    assert!(cccc_runtime::status(group_id, "web1").is_err());

    for text in ["first", "second"] {
        call(
            &home,
            "send",
            json!({"group_id":group_id,"by":"user","to":["web1"],"text":text,"message_mode":"send"}),
        );
    }
    let turn = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"","by":"web1"}),
    );
    assert_eq!(turn.result["status"], "work_available");
    assert_eq!(
        turn.result["turn"]["messages"].as_array().map(Vec::len),
        Some(2)
    );
    let coalesced = turn.result["turn"]["coalesced_text"]
        .as_str()
        .expect("coalesced text");
    assert!(coalesced.contains("[cccc] user → web1 [event_id="));
    assert!(coalesced.contains("]: first"));
    assert!(coalesced.contains("]: second"));
    assert!(!coalesced.contains(cccc_core::system_prompt::NEW_MESSAGE_MODE_GUIDANCE));
    assert!(
        turn.result["turn"]["system_prompt"]
            .as_str()
            .is_some_and(|prompt| {
                prompt.contains("web1")
                    && prompt.contains(cccc_core::system_prompt::NEW_MESSAGE_MODE_GUIDANCE)
                    && prompt.contains(cccc_core::system_prompt::EXISTING_MESSAGE_REPLY_GUIDANCE)
            })
    );
    let event_ids = turn.result["turn"]["event_ids"]
        .as_array()
        .cloned()
        .expect("event ids");
    for event_id in &event_ids {
        let event_id = event_id.as_str().expect("event id");
        assert!(coalesced.contains(&format!("[event_id={event_id} sent=")));
    }
    let turn_id = turn.result["turn"]["turn_id"]
        .as_str()
        .expect("turn id")
        .to_owned();
    let rejected = raw_call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done","turn_id":turn_id,"event_ids":[event_ids[1].clone()]}),
    );
    assert!(!rejected.ok);
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("completion_conflict")
    );
    let stale = raw_call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done","turn_id":"wrong-turn","event_ids":event_ids}),
    );
    assert!(!stale.ok);
    assert_eq!(
        stale.error.as_ref().map(|error| error.code.as_str()),
        Some("stale_turn")
    );

    let completed = call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done","event_ids":event_ids}),
    );
    assert!(completed.result.get("cursor_committed").is_none());
    assert_eq!(completed.result["turn_id"], turn_id);
    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
    );
    assert_eq!(
        inbox.result["messages"],
        json!([]),
        "direct runtime work must not enter the Mail Inbox"
    );
    let idle = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
    );
    assert_eq!(idle.result["status"], "idle");
}

#[test]
fn a_pulled_turn_withdraws_a_mail_notice_whose_mail_was_already_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"stale pull"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","by":"user"}),
    );
    let store = GroupStore::new(home.clone()).expect("group store");
    store
        .mutate(group_id, |group| {
            group.running = true;
            Ok(())
        })
        .expect("enable structured runtime fixture");
    let sent = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"review","message_mode":"mail"}),
    );
    let mail_id = sent.result["event"]["id"]
        .as_str()
        .expect("mail id")
        .to_owned();
    // The unread tick issued a notice for that Mail.
    let mut notice = cccc_contracts::Event::new("system.notify", group_id);
    notice.by = "system".into();
    notice.data = json!({
        "kind":"mail_notice","title":"Mail waiting","message":"You have 1 Mail item(s) waiting.",
        "target_actor_id":"web1","context":{"actor_id":"web1","source_event_ids":[mail_id]}
    })
    .as_object()
    .cloned()
    .expect("notice");
    cccc_core::ledger::append(&store.ledger_path(group_id).expect("ledger"), &notice)
        .expect("notice");

    // The actor reads its Mail before pulling its next turn.
    call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
    );
    let turn = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"","by":"web1"}),
    );
    let pulled = turn.result["turn"]["event_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !pulled.contains(&json!(notice.id)),
        "a stale Mail waiting notice must not be handed over: {:?}",
        turn.result
    );
    let ledger =
        cccc_core::ledger::read_all(&store.ledger_path(group_id).expect("ledger")).expect("events");
    assert!(ledger.iter().any(|event| {
        event.kind == "runtime.delivery"
            && event.data["source_event_id"] == notice.id
            && event.data["state"] == "withdrawn"
    }));
}

#[test]
fn claimed_browser_mail_turn_survives_idle_heartbeat_and_late_submission_receipt() {
    for status in ["idle", "waiting"] {
        let (_temp, home, group_id, notice) = browser_mail_notice_fixture();
        let accepted = call(
            &home,
            "runtime_wait_next_turn",
            json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"}),
        );
        assert_eq!(accepted.result["status"], "work_available");
        let turn = &accepted.result["turn"];
        assert_eq!(turn["event_ids"], json!([notice.id]));
        call(
            &home,
            "inbox_read",
            json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
        );
        // A status heartbeat does not finish the browser owner's active turn.
        call(
            &home,
            "headless_set_status",
            json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":status}),
        );
        let next = call(
            &home,
            "runtime_wait_next_turn",
            json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"}),
        );
        let recorded = call(
            &home,
            "web_model_browser_delivery_record",
            json!({"group_id":group_id,"actor_id":"web1","by":"web1",
                "turn_id":turn["turn_id"],"event_ids":turn["event_ids"],"delivery_id":"late-submission",
                "browser_delivery":{"state":"submitted","detail":"isolated external submit evidence"}}),
        );
        assert_eq!(
            recorded.result["event"]["kind"],
            "web_model.browser_delivery.submitted"
        );
        let states = notice_delivery_states(&home, &group_id, &notice.id);
        assert_eq!(
            next.result["status"], "turn_in_progress",
            "{status}: {states:?}"
        );
        assert_eq!(next.result["active_turn_id"], turn["turn_id"]);
        assert_eq!(
            states,
            vec!["claimed", "accepted"],
            "the owned turn was never withdrawn"
        );
        call(
            &home,
            "runtime_complete_turn",
            json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done",
                "turn_id":turn["turn_id"],"event_ids":turn["event_ids"]}),
        );
        let idle = call(
            &home,
            "runtime_wait_next_turn",
            json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"}),
        );
        assert_eq!(
            idle.result["status"], "idle",
            "completion releases the heartbeat's turn"
        );
    }
}

#[test]
fn stranded_browser_mail_claim_remains_uncertain_until_real_submission_evidence_arrives() {
    let (_temp, home, group_id, notice) = browser_mail_notice_fixture();
    let accepted = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"}),
    );
    let turn = &accepted.result["turn"];
    // A claim and its active projection are separate writes. Retain the durable
    // claim while simulating a missing projection after that partial boundary.
    cccc_core::integration_state::group_update(
        &GroupStore::new(home.clone()).expect("store"),
        &group_id,
        "runtime_states",
        |value| {
            value
                .as_object_mut()
                .expect("runtime states")
                .remove("web1");
            Ok(())
        },
    )
    .expect("lose only active projection");
    call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
    );
    let next = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"}),
    );
    assert_eq!(next.result["status"], "idle");
    let before = notice_delivery_states(&home, &group_id, &notice.id);
    assert_eq!(
        before,
        vec!["claimed", "ambiguous"],
        "a durable claim cannot prove the notice was unsent"
    );
    call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"new work","message_mode":"send"}),
    );
    let new_turn = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"}),
    );
    assert_eq!(new_turn.result["status"], "work_available");
    assert_ne!(new_turn.result["turn"]["turn_id"], turn["turn_id"]);
    call(
        &home,
        "web_model_browser_delivery_record",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1",
            "turn_id":turn["turn_id"],"event_ids":turn["event_ids"],"delivery_id":"late-submission",
            "browser_delivery":{"state":"submitted","detail":"isolated external submit evidence"}}),
    );
    assert_eq!(
        notice_delivery_states(&home, &group_id, &notice.id),
        vec!["claimed", "ambiguous", "accepted"]
    );
    let active = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_browser"}),
    );
    assert_eq!(active.result["status"], "turn_in_progress");
    assert_eq!(
        active.result["active_turn_id"], new_turn.result["turn"]["turn_id"],
        "late evidence never resubmits the notice or replaces newer work"
    );
}

fn browser_mail_notice_fixture() -> (tempfile::TempDir, HomeLayout, String, cccc_contracts::Event) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"browser Mail ownership"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","by":"user"}),
    );
    let store = GroupStore::new(home.clone()).expect("store");
    store
        .mutate(&group_id, |group| {
            group.running = true;
            Ok(())
        })
        .expect("fixture running");
    let mail = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"review","message_mode":"mail"}),
    );
    let mut notice = cccc_contracts::Event::new("system.notify", &group_id);
    notice.by = "system".into();
    notice.data = json!({"kind":"mail_notice","target_actor_id":"web1","message":"Mail waiting",
        "context":{"actor_id":"web1","source_event_ids":[mail.result["event"]["id"]]}})
    .as_object()
    .expect("notice")
    .clone();
    cccc_core::ledger::append(&store.ledger_path(&group_id).expect("ledger"), &notice)
        .expect("notice");
    (temp, home, group_id, notice)
}

fn notice_delivery_states(home: &HomeLayout, group_id: &str, notice_id: &str) -> Vec<String> {
    cccc_core::ledger::read_all(
        &GroupStore::new(home.clone())
            .expect("store")
            .ledger_path(group_id)
            .expect("ledger"),
    )
    .expect("ledger")
    .into_iter()
    .filter(|event| event.kind == "runtime.delivery" && event.data["source_event_id"] == notice_id)
    .map(|event| event.data["state"].as_str().expect("state").to_owned())
    .collect()
}

#[test]
fn managed_terminal_actor_exposes_daemon_owned_structured_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"managed status"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"codex-1","runtime":"codex","by":"user"}),
    );

    let status = call(
        &home,
        "headless_status",
        json!({"group_id":group_id,"actor_id":"codex-1","by":"user"}),
    );
    assert_eq!(status.result["state"]["status"], "idle");

    let write = raw_call(
        &home,
        "headless_set_status",
        json!({"group_id":group_id,"actor_id":"codex-1","status":"working","by":"user"}),
    );
    assert_eq!(
        write.error.as_ref().map(|error| error.code.as_str()),
        Some("provider_managed_headless")
    );
}
#[test]
fn runtime_wait_rejects_an_unknown_explicit_transport_without_claiming_work() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"invalid transport"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"web1","runtime":"web_model",
            "by":"user"
        }),
    );
    GroupStore::new(home.clone())
        .expect("group store")
        .mutate(group_id, |group| {
            group.running = true;
            Ok(())
        })
        .expect("enable structured runtime fixture");
    call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["web1"],"text":"pending",
            "message_mode":"send"
        }),
    );

    let response = raw_call(
        &home,
        "runtime_wait_next_turn",
        json!({
            "group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_typo"
        }),
    );
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_transport")
    );
    let ledger_path = GroupStore::new(home.clone())
        .expect("group store")
        .ledger_path(group_id)
        .expect("ledger path");
    assert!(
        cccc_core::ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .all(|event| event.kind != "runtime.delivery")
    );
}

#[cfg(unix)]
#[test]
fn terminal_actor_requires_an_attached_project_scope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"headless scope"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"codex-terminal",
            "runtime":"codex",
            "command":["sh","-c","while IFS= read -r line; do :; done"],
            "by":"user"
        }),
    );

    let started = raw_call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"codex-terminal","by":"user"}),
    );

    assert!(
        !started.ok,
        "scope-less terminal actor unexpectedly started"
    );
    assert_eq!(
        started.error.as_ref().map(|error| error.code.as_str()),
        Some("missing_project_root")
    );
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn raw_call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
// Included by the crate-level integration test harness.
