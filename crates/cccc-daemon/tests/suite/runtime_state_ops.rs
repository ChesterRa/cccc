use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
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
            json!({"group_id":group_id,"by":"user","to":["headless1"],"text":text}),
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
    assert!(coalesced.contains("[cccc] user → headless1: first"));
    assert!(coalesced.contains("[cccc] user → headless1: second"));
    assert!(coalesced.contains("Use cccc_message_reply for replies"));
    assert!(
        turn.result["turn"]["system_prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains("headless1"))
    );
    let event_ids = turn.result["turn"]["event_ids"]
        .as_array()
        .cloned()
        .expect("event ids");
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
        Some("non_contiguous_turn_events")
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
    assert_eq!(completed.result["cursor_committed"], true);
    assert_eq!(completed.result["turn_id"], turn_id);
    let idle = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1"}),
    );
    assert_eq!(idle.result["status"], "idle");
}

#[cfg(unix)]
#[test]
fn codex_headless_starts_a_provider_and_delivers_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"codex headless"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
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
        json!({"group_id":group_id,"by":"user","to":["codex-headless"],"text":"do the work"}),
    );
    assert_eq!(sent.result["delivery"]["queued"], 1);
    let event_id = sent.result["event"]["id"].as_str().expect("event id");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let unread = call(
            &home,
            "inbox_list",
            json!({"group_id":group_id,"actor_id":"codex-headless","unread_only":true}),
        );
        let still_unread = unread.result["messages"]
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["id"] == event_id));
        if !still_unread {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "headless message was not consumed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
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
        json!({"group_id":group_id,"by":"user","to":["claude-headless"],"text":"do the work"}),
    );
    assert_eq!(sent.result["delivery"]["queued"], 1);
    let event_id = sent.result["event"]["id"].as_str().expect("event id");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let unread = call(
            &home,
            "inbox_list",
            json!({"group_id":group_id,"actor_id":"claude-headless"}),
        );
        let consumed = unread.result["messages"]
            .as_array()
            .is_some_and(|events| events.iter().all(|event| event["id"] != event_id));
        if consumed {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Claude headless message was not consumed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
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
