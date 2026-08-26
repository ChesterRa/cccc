use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

#[test]
fn headless_actor_uses_structured_turns_without_a_pty() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"runtime state"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"headless1","runtime":"custom","runner":"headless","by":"user"}),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"headless1","by":"user"}),
    );
    assert!(cccc_runtime::status(group_id, "headless1").is_err());
    call(
        &home,
        "headless_set_status",
        json!({"group_id":group_id,"actor_id":"headless1","status":"working","task_id":"task-1"}),
    );
    let state = call(
        &home,
        "headless_status",
        json!({"group_id":group_id,"actor_id":"headless1"}),
    );
    assert_eq!(state.result["state"]["status"], "working");
    assert_eq!(state.result["state"]["task_id"], "task-1");

    for text in ["first", "second"] {
        call(
            &home,
            "send",
            json!({"group_id":group_id,"by":"user","to":["headless1"],"text":text,"message_mode":"send"}),
        );
    }
    let turn = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"","by":"headless1"}),
    );
    assert_eq!(turn.result["status"], "work_available");
    assert_eq!(
        turn.result["turn"]["messages"].as_array().map(Vec::len),
        Some(2)
    );
    let coalesced = turn.result["turn"]["coalesced_text"]
        .as_str()
        .expect("coalesced text");
    assert!(coalesced.contains("[cccc] user → headless1 [event_id="));
    assert!(coalesced.contains("message_mode=send]: first"));
    assert!(coalesced.contains("message_mode=send]: second"));
    assert!(!coalesced.contains(cccc_core::system_prompt::MESSAGE_DELIVERY_GUIDANCE));
    assert!(
        turn.result["turn"]["system_prompt"]
            .as_str()
            .is_some_and(|prompt| {
                prompt.contains("headless1")
                    && prompt.contains(cccc_core::system_prompt::MESSAGE_DELIVERY_GUIDANCE)
            })
    );
    let event_ids = turn.result["turn"]["event_ids"]
        .as_array()
        .cloned()
        .expect("event ids");
    for event_id in &event_ids {
        let event_id = event_id.as_str().expect("event id");
        assert!(coalesced.contains(&format!("[event_id={event_id} message_mode=send]")));
    }
    let turn_id = turn.result["turn"]["turn_id"]
        .as_str()
        .expect("turn id")
        .to_owned();
    let rejected = raw_call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1","status":"done","turn_id":turn_id,"event_ids":[event_ids[1].clone()]}),
    );
    assert!(!rejected.ok);
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("completion_conflict")
    );
    let stale = raw_call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1","status":"done","turn_id":"wrong-turn","event_ids":event_ids}),
    );
    assert!(!stale.ok);
    assert_eq!(
        stale.error.as_ref().map(|error| error.code.as_str()),
        Some("stale_turn")
    );

    let completed = call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1","status":"done","event_ids":event_ids}),
    );
    assert!(completed.result.get("cursor_committed").is_none());
    assert_eq!(completed.result["turn_id"], turn_id);
    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1"}),
    );
    assert_eq!(
        inbox.result["messages"],
        json!([]),
        "direct runtime work must not enter the Mail Inbox"
    );
    let idle = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1"}),
    );
    assert_eq!(idle.result["status"], "idle");
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
            "runner":"headless","by":"user"
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
fn local_headless_requires_an_attached_project_scope() {
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
            "actor_id":"codex-headless",
            "runtime":"codex",
            "runner":"headless",
            "command":["sh","-c","while IFS= read -r line; do :; done"],
            "by":"user"
        }),
    );

    let started = raw_call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"user"}),
    );

    assert!(
        !started.ok,
        "scope-less headless actor unexpectedly started"
    );
    assert_eq!(
        started.error.as_ref().map(|error| error.code.as_str()),
        Some("missing_project_root")
    );
}

#[cfg(unix)]
#[test]
fn codex_headless_restores_working_after_waiting_flags_clear() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let waiting_log = temp.path().join("waiting");
    let active_log = temp.path().join("active");
    let created = call(&home, "group_create", json!({"title":"codex waiting"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":temp.path(),"by":"user"}),
    );
    let fake_app_server = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      ;;
    *'"method":"thread/start"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"thread":{"id":"thread-1"}}}\n' "$id"
      ;;
    *'"method":"turn/start"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-%s"}}}\n' "$id" "$id"
      printf '{"jsonrpc":"2.0","method":"thread/status/changed","params":{"threadId":"thread-1","status":{"type":"active","activeFlags":["waitingOnUserInput"]}}}\n'
      : > "$CCCC_WAITING_LOG"
      sleep 1
      printf '{"jsonrpc":"2.0","method":"thread/status/changed","params":{"threadId":"thread-1","status":{"type":"active","activeFlags":[]}}}\n'
      : > "$CCCC_ACTIVE_LOG"
      sleep 1
      printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn-%s","status":"completed"}}}\n' "$id"
      ;;
  esac
done
"#;
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"codex-headless",
            "runtime":"codex",
            "runner":"headless",
            "command":["sh","-c",fake_app_server],
            "env":{
                "CCCC_WAITING_LOG":waiting_log,
                "CCCC_ACTIVE_LOG":active_log
            },
            "by":"user"
        }),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"user"}),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !waiting_log.is_file() {
        assert!(
            std::time::Instant::now() < deadline,
            "waiting status not emitted"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    loop {
        let state = call(
            &home,
            "headless_status",
            json!({"group_id":group_id,"actor_id":"codex-headless"}),
        );
        if state.result["state"]["status"] == "waiting" {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "waiting state not observed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !active_log.is_file() {
        assert!(
            std::time::Instant::now() < deadline,
            "active status not emitted"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    loop {
        let state = call(
            &home,
            "headless_status",
            json!({"group_id":group_id,"actor_id":"codex-headless"}),
        );
        if state.result["state"]["status"] == "working" {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "working state not restored"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"user"}),
    );
}

#[cfg(unix)]
#[test]
fn codex_headless_starts_a_provider_and_delivers_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let resume_log = temp.path().join("resume-attempted");
    let server_request_log = temp.path().join("server-request-response");
    let created = call(&home, "group_create", json!({"title":"codex headless"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":temp.path(),"by":"user"}),
    );
    let fake_app_server = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      ;;
    *'"method":"thread/start"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"thread":{"id":"thread-1"}}}\n' "$id"
      ;;
    *'"method":"thread/resume"'*)
      printf 'attempted' > "$CCCC_RESUME_LOG"
      printf '{"jsonrpc":"2.0","id":%s,"error":{"message":"saved thread unavailable"}}\n' "$id"
      ;;
    *'"method":"turn/start"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"turn":{"id":"turn-%s"}}}\n' "$id" "$id"
      printf '{"jsonrpc":"2.0","id":"input-1","method":"item/tool/requestUserInput","params":{"turnId":"turn-%s"}}\n' "$id"
      case "$line" in
        *'fail this turn'*)
          printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn-%s","status":"failed","error":{"message":"provider failed"}}}}\n' "$id"
          ;;
        *)
          printf '{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn-%s","status":"completed"}}}\n' "$id"
          ;;
      esac
      ;;
    *'"id":"input-1"'*)
      printf '%s' "$line" > "$CCCC_SERVER_REQUEST_LOG"
      ;;
  esac
done
"#;
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"codex-headless",
            "runtime":"codex",
            "runner":"headless",
            "command":["sh","-c",fake_app_server],
            "env":{
                "CCCC_RESUME_LOG":resume_log,
                "CCCC_SERVER_REQUEST_LOG":server_request_log
            },
            "by":"user"
        }),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"user"}),
    );
    let running = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(running.result["actors"][0]["running"], true);
    assert!(running.result["actors"][0]["pid"].as_u64().is_some());

    let sent = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["codex-headless"],"text":"do the work","message_mode":"send"}),
    );
    assert_eq!(sent.result["message_mode"], "send");
    let event_id = sent.result["event"]["id"].as_str().expect("event id");
    let headless_events_path = home
        .groups_dir()
        .join(group_id)
        .join("state/headless/events.jsonl");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let headless_events = loop {
        let events = std::fs::read_to_string(&headless_events_path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Vec<_>>();
        let has_terminal = events.iter().any(|event| {
            event["data"]["event_id"] == event_id
                && matches!(
                    event["type"].as_str(),
                    Some("headless.turn.completed" | "headless.turn.failed")
                )
        });
        if has_terminal {
            break events;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "headless terminal event was not recorded"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let started_index = headless_events
        .iter()
        .position(|event| {
            event["type"] == "headless.turn.started" && event["data"]["event_id"] == event_id
        })
        .expect("turn started event");
    let terminal_index = headless_events
        .iter()
        .position(|event| {
            event["data"]["event_id"] == event_id
                && matches!(
                    event["type"].as_str(),
                    Some("headless.turn.completed" | "headless.turn.failed")
                )
        })
        .expect("turn terminal event");
    assert!(
        started_index < terminal_index,
        "provider terminal event preceded turn acceptance: {headless_events:?}"
    );
    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"codex-headless"}),
    );
    assert_eq!(
        inbox.result["messages"],
        json!([]),
        "direct provider work must not enter the Mail Inbox"
    );
    assert!(headless_events.iter().any(|event| {
        event["type"] == "headless.control.started" && event["data"]["control_kind"] == "bootstrap"
    }));
    assert!(headless_events.iter().any(|event| {
        event["type"] == "headless.control.completed"
            && event["data"]["control_kind"] == "bootstrap"
    }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !server_request_log.is_file() {
        assert!(
            std::time::Instant::now() < deadline,
            "headless provider server request received no response"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let server_response: Value = serde_json::from_str(
        &std::fs::read_to_string(&server_request_log).expect("server request response"),
    )
    .expect("valid server request response");
    assert_eq!(server_response["id"], "input-1");
    assert_eq!(server_response["error"]["code"], -32601);

    let failed = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["codex-headless"],"text":"fail this turn","message_mode":"send"}),
    );
    let failed_event_id = failed.result["event"]["id"]
        .as_str()
        .expect("failed event id");
    let succeeded = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["codex-headless"],"text":"continue after failure","message_mode":"send"}),
    );
    let succeeded_event_id = succeeded.result["event"]["id"]
        .as_str()
        .expect("succeeded event id");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let recovery_events = loop {
        let events = std::fs::read_to_string(&headless_events_path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Vec<_>>();
        let failed_recorded = events.iter().any(|event| {
            event["type"] == "headless.turn.failed" && event["data"]["event_id"] == failed_event_id
        });
        let success_recorded = events.iter().any(|event| {
            event["type"] == "headless.turn.completed"
                && event["data"]["event_id"] == succeeded_event_id
        });
        if failed_recorded && success_recorded {
            break events;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "later headless turn did not progress after provider failure: {events:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let failed_index = recovery_events
        .iter()
        .position(|event| {
            event["type"] == "headless.turn.failed" && event["data"]["event_id"] == failed_event_id
        })
        .expect("failed terminal event");
    let success_index = recovery_events
        .iter()
        .position(|event| {
            event["type"] == "headless.turn.completed"
                && event["data"]["event_id"] == succeeded_event_id
        })
        .expect("later completed event");
    assert!(failed_index < success_index);
    let state = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(
        state.result["actors"][0]["effective_working_reason"],
        "provider_headless_session"
    );

    call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"user"}),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"user"}),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !resume_log.is_file() {
        assert!(
            std::time::Instant::now() < deadline,
            "headless restart did not attempt provider-thread resume"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let restarted = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(restarted.result["actors"][0]["running"], true);
    assert_eq!(
        restarted.result["actors"][0]["runtime_session_status"],
        "usable"
    );
    call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"codex-headless","by":"user"}),
    );
    let stopped = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(stopped.result["actors"][0]["running"], false);
}

#[cfg(unix)]
#[test]
fn claude_headless_stream_json_stops_with_its_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"claude headless"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":temp.path(),"by":"user"}),
    );
    let fake_claude = r#"
while IFS= read -r line; do
  printf '{"type":"assistant","message":{"id":"message-1","content":[{"type":"text","text":"done"}]}}\n'
  printf '{"type":"result","subtype":"success"}\n'
done
"#;
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"claude-headless",
            "runtime":"claude",
            "runner":"headless",
            "command":["sh","-c",fake_claude],
            "by":"user"
        }),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"claude-headless","by":"user"}),
    );
    let sent = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["claude-headless"],"text":"do the work","message_mode":"send"}),
    );
    assert_eq!(sent.result["message_mode"], "send");
    let event_id = sent.result["event"]["id"].as_str().expect("event id");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let statuses = call(
            &home,
            "ledger_statuses",
            json!({"group_id":group_id,"event_ids":[event_id]}),
        );
        if statuses.result["statuses"][event_id]["obligation_status"]["claude-headless"]["delivery_state"]
            == "accepted"
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Claude headless message was not accepted by the runtime"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"claude-headless","by":"claude-headless"}),
    );
    assert_eq!(
        inbox.result["messages"],
        json!([]),
        "direct provider work must not enter the Mail Inbox"
    );
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    let stopped = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(stopped.result["actors"][0]["running"], false);
    assert_eq!(
        stopped.result["actors"][0]["effective_working_state"],
        "stopped"
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
