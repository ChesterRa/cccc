use super::*;

fn inner() -> (Inner, broadcast::Receiver<AnalystEvent>) {
    let (events, receiver) = broadcast::channel(128);
    (
        Inner {
            http: reqwest::Client::new(),
            endpoint: "http://127.0.0.1:1".into(),
            session_id: "ses_fixture".into(),
            generation: "generation".into(),
            events,
            state: Mutex::new(events::State::default()),
            running: AtomicBool::new(true),
        },
        receiver,
    )
}

fn observe(state: &mut events::State, inner: &Inner, kind: &str, mut data: Value) {
    data["sessionID"] = json!("ses_fixture");
    state
        .observe(inner, &json!({"type":kind,"data":data}))
        .expect("V2 event");
}

fn enqueue(state: &mut events::State, inner: &Inner, id: &str, text: &str) {
    observe(
        state,
        inner,
        "session.inbox.enqueued",
        json!({"inboxID":id,"item":{"type":"user","payload":{"text":text},"delivery":"queue"}}),
    );
}

fn deliver(state: &mut events::State, inner: &Inner, id: &str) {
    observe(
        state,
        inner,
        "session.inbox.delivered",
        json!({"inboxID":id}),
    );
    observe(
        state,
        inner,
        "session.step.started",
        json!({"assistantMessageID":format!("assistant-{id}")}),
    );
}

fn answer(state: &mut events::State, inner: &Inner, id: &str) {
    observe(
        state,
        inner,
        "session.text.delta",
        json!({"assistantMessageID":format!("assistant-{id}"),"ordinal":0,"delta":format!("answer-{id}")}),
    );
    observe(
        state,
        inner,
        "session.step.ended",
        json!({"assistantMessageID":format!("assistant-{id}"),"finish":"stop"}),
    );
}

fn drain(receiver: &mut broadcast::Receiver<AnalystEvent>) -> Vec<AnalystEvent> {
    std::iter::from_fn(|| receiver.try_recv().ok()).collect()
}

#[test]
fn v2_queued_inputs_have_separate_turns_and_do_not_settle_on_an_rpc_receipt() {
    let (inner, mut receiver) = inner();
    let mut state = events::State::default();
    state.controlled = Some(("msg_first".into(), "delegation-first".into()));
    enqueue(&mut state, &inner, "msg_first", "first");
    deliver(&mut state, &inner, "msg_first");
    enqueue(&mut state, &inner, "msg_second", "second");
    enqueue(&mut state, &inner, "msg_third", "third");
    assert_eq!(state.active.as_ref().expect("first active").id, "msg_first");
    answer(&mut state, &inner, "msg_first");
    assert_eq!(
        drain(&mut receiver)
            .iter()
            .filter(|e| e.message["method"] == "turn/completed")
            .count(),
        0
    );
    deliver(&mut state, &inner, "msg_second");
    // A delayed assistant event from the old turn cannot enter the new answer.
    observe(
        &mut state,
        &inner,
        "session.text.delta",
        json!({"assistantMessageID":"assistant-msg_first","delta":"MUST-NOT-APPEAR"}),
    );
    answer(&mut state, &inner, "msg_second");
    deliver(&mut state, &inner, "msg_third");
    answer(&mut state, &inner, "msg_third");
    observe(&mut state, &inner, "session.execution.succeeded", json!({}));
    observe(&mut state, &inner, "session.execution.succeeded", json!({}));
    let events = drain(&mut receiver);
    let completed = events
        .iter()
        .filter(|e| e.message["method"] == "turn/completed")
        .map(|e| e.message["params"]["turn"]["id"].as_str().expect("turn ID"))
        .collect::<Vec<_>>();
    assert_eq!(completed, ["msg_first", "msg_second", "msg_third"]);
    assert!(
        !events
            .iter()
            .any(|e| e.message.to_string().contains("MUST-NOT-APPEAR"))
    );
    assert!(state.active.is_none());
}

#[test]
fn v2_tool_continuations_and_length_limits_are_not_successful_turn_completions() {
    let (inner, mut receiver) = inner();
    let mut state = events::State::default();
    enqueue(&mut state, &inner, "msg_tool", "tool");
    deliver(&mut state, &inner, "msg_tool");
    observe(
        &mut state,
        &inner,
        "session.step.ended",
        json!({"assistantMessageID":"assistant-msg_tool","finish":"tool-calls"}),
    );
    assert!(state.active.is_some());
    observe(
        &mut state,
        &inner,
        "session.step.started",
        json!({"assistantMessageID":"continuation"}),
    );
    observe(
        &mut state,
        &inner,
        "session.step.ended",
        json!({"assistantMessageID":"continuation","finish":"length"}),
    );
    observe(&mut state, &inner, "session.execution.succeeded", json!({}));
    let events = drain(&mut receiver);
    let terminal = events
        .iter()
        .find(|e| e.message["method"] == "turn/completed")
        .expect("terminal");
    assert_eq!(terminal.message["params"]["turn"]["status"], "failed");
}

#[test]
fn v2_native_receipts_use_exact_input_and_mcp_results_stay_structured() {
    let (inner, mut receiver) = inner();
    let mut state = events::State::default();
    native_input::register(
        &mut state.native,
        "native-delegation".into(),
        "exact text".into(),
    )
    .expect("register");
    enqueue(&mut state, &inner, "msg_native", "exact text");
    deliver(&mut state, &inner, "msg_native");
    observe(
        &mut state,
        &inner,
        "session.tool.input.started",
        json!({"assistantMessageID":"assistant-msg_native","id":"call","name":"cccc_cccc_voice_secretary_task"}),
    );
    observe(
        &mut state,
        &inner,
        "session.tool.called",
        json!({"assistantMessageID":"assistant-msg_native","id":"call","input":{"action":"report","task_id":"current"}}),
    );
    observe(
        &mut state,
        &inner,
        "session.tool.success",
        json!({"assistantMessageID":"assistant-msg_native","id":"call","content":[{"type":"text","text":"{\"task_id\":\"current\",\"accepted\":true}"}]}),
    );
    let events = drain(&mut receiver);
    assert_eq!(
        events[0].requested_delegation_id.as_deref(),
        Some("native-delegation")
    );
    let receipt = events
        .iter()
        .find(|e| e.message["params"]["item"]["type"] == "mcpToolCall")
        .expect("MCP result");
    assert_eq!(
        receipt.message["params"]["item"]["tool"],
        "cccc_voice_secretary_task"
    );
    assert_eq!(
        receipt.message["params"]["item"]["result"]["structuredContent"]["accepted"],
        true
    );
    assert_eq!(receipt.message["params"]["turnId"], "msg_native");
}

#[test]
fn v2_stream_decoder_handles_encoded_events_fragmentation_and_line_limits() {
    let event = json!({"type":"session.execution.succeeded","data":{"sessionID":"ses_fixture"}});
    let encoded = format!(
        "data: {}\r\n\r\n",
        serde_json::to_string(&event.to_string()).expect("encoded JSON")
    );
    let mut buffer = lifecycle::SseLineBuffer::new(1024);
    let mut decoded = Vec::new();
    for fragment in encoded.as_bytes().chunks(3) {
        decoded.extend(decode_chunk(&mut buffer, fragment).expect("fragment"));
    }
    assert_eq!(decoded, [event]);
    assert!(decode_chunk(&mut lifecycle::SseLineBuffer::new(8), encoded.as_bytes()).is_err());
}

#[test]
fn v2_configuration_preserves_other_servers_and_uses_the_fixed_actor_mcp() {
    let config=configuration(Some(r#"{"snapshots":false,"mcp":{"servers":{"other":{"type":"local","command":["other"]}}}}"#),json!({"command":"cccc","args":["mcp"],"env":[{"name":"CCCC_ACTOR_ID","value":"peer"}]}),Some(("voice","fixed instructions"))).expect("config");
    assert_eq!(config["snapshots"], false);
    assert_eq!(
        config["mcp"]["servers"]["other"]["command"],
        json!(["other"])
    );
    assert_eq!(
        config["mcp"]["servers"]["cccc"]["command"],
        json!(["cccc", "mcp"])
    );
    assert_eq!(
        config["mcp"]["servers"]["cccc"]["environment"]["CCCC_ACTOR_ID"],
        "peer"
    );
    assert_eq!(config["agents"]["voice"]["system"], "fixed instructions");
}

#[test]
fn v2_completed_text_is_available_without_deltas_and_parts_keep_separate_ids() {
    let (inner, mut receiver) = inner();
    let mut state = events::State::default();
    enqueue(&mut state, &inner, "msg_text", "text");
    deliver(&mut state, &inner, "msg_text");
    for (ordinal, text) in [(0, "first part"), (1, "second part")] {
        observe(
            &mut state,
            &inner,
            "session.text.ended",
            json!({"assistantMessageID":"assistant-msg_text","ordinal":ordinal,"text":text}),
        );
    }
    let events = drain(&mut receiver);
    let items = events
        .iter()
        .filter(|e| e.message["method"] == "item/completed")
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    assert_ne!(
        items[0].message["params"]["item"]["id"],
        items[1].message["params"]["item"]["id"]
    );
    assert_eq!(items[1].message["params"]["item"]["text"], "second part");
}

#[tokio::test]
async fn v2_final_snapshot_survives_completion_in_the_analyst_lifecycle() {
    use crate::ops::codex_voice_lifecycle::{
        AnalystLifecycleEvent, collect_normalized_events_for_test,
    };
    for delta in ["", "partial"] {
        let (inner, mut receiver) = inner();
        let mut state = events::State::default();
        enqueue(&mut state, &inner, "msg_final", "question");
        deliver(&mut state, &inner, "msg_final");
        if !delta.is_empty() {
            observe(
                &mut state,
                &inner,
                "session.text.delta",
                json!({
                    "assistantMessageID":"assistant-msg_final","ordinal":0,"delta":delta
                }),
            );
        }
        observe(
            &mut state,
            &inner,
            "session.text.ended",
            json!({
                "assistantMessageID":"assistant-msg_final","ordinal":0,"text":"Complete authoritative answer"
            }),
        );
        observe(
            &mut state,
            &inner,
            "session.step.ended",
            json!({
                "assistantMessageID":"assistant-msg_final","finish":"stop"
            }),
        );
        observe(&mut state, &inner, "session.execution.succeeded", json!({}));
        let collected = collect_normalized_events_for_test(drain(&mut receiver), None).await;
        let completed = collected.iter().find_map(|event| match event {
            AnalystLifecycleEvent::Completed { status, result, .. } => {
                Some((status.as_str(), result.as_str()))
            }
            _ => None,
        });
        assert_eq!(
            completed,
            Some(("completed", "Complete authoritative answer"))
        );
    }
}

#[test]
fn v2_promoted_inbox_input_steers_the_current_turn() {
    let (inner, mut receiver) = inner();
    let mut state = events::State::default();
    enqueue(&mut state, &inner, "msg_current", "initial task");
    deliver(&mut state, &inner, "msg_current");
    native_input::register(
        &mut state.native,
        "steering-delegation".into(),
        "change direction".into(),
    )
    .expect("native input");
    enqueue(&mut state, &inner, "msg_steer", "change direction");
    observe(
        &mut state,
        &inner,
        "session.inbox.delivery.changed",
        json!({"inboxID":"msg_steer","delivery":"steer"}),
    );
    observe(
        &mut state,
        &inner,
        "session.inbox.delivered",
        json!({"inboxID":"msg_steer"}),
    );
    let events = drain(&mut receiver);
    assert_eq!(
        state.active.as_ref().expect("current turn").id,
        "msg_current"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.message["method"] == "turn/completed")
    );
    let associated = events
        .iter()
        .find(|event| event.requested_delegation_id.as_deref() == Some("steering-delegation"))
        .expect("steering associated");
    assert_eq!(associated.message["params"]["turn"]["id"], "msg_current");
    answer(&mut state, &inner, "msg_current");
    observe(&mut state, &inner, "session.execution.succeeded", json!({}));
    let terminal = drain(&mut receiver);
    assert_eq!(
        terminal
            .iter()
            .filter(|event| event.message["method"] == "turn/completed")
            .count(),
        1
    );
    assert_eq!(
        terminal.last().expect("completion").message["params"]["turn"]["id"],
        "msg_current"
    );
}

#[tokio::test]
async fn v2_cancelled_controlled_input_settles_and_allows_the_next_prompt() {
    use axum::{Json, Router, routing::post};
    use std::future::IntoFuture;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("isolated backend");
    let (mut inner, mut receiver) = inner();
    inner.endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(axum::serve(listener, Router::new().route(
        "/api/session/ses_fixture/prompt", post(|| async {
            Json(json!({"data":{"id":"msg_next","sessionID":"ses_fixture","delivery":"queue"}}))
        })
    )).into_future());
    let inner = Arc::new(inner);
    {
        let mut state = inner.state.lock().await;
        state.controlled = Some(("msg_cancelled".into(), "cancelled-delegation".into()));
        enqueue(&mut state, &inner, "msg_cancelled", "cancel me");
        enqueue(&mut state, &inner, "msg_unrelated", "other native input");
        observe(
            &mut state,
            &inner,
            "session.inbox.cancelled",
            json!({"inboxID":"msg_unrelated"}),
        );
        assert!(
            state.controlled.is_some(),
            "another input cannot cancel CCCC ownership"
        );
        observe(
            &mut state,
            &inner,
            "session.inbox.cancelled",
            json!({"inboxID":"msg_cancelled"}),
        );
        observe(
            &mut state,
            &inner,
            "session.inbox.cancelled",
            json!({"inboxID":"msg_cancelled"}),
        );
    }
    let events = drain(&mut receiver);
    let cancelled = events
        .iter()
        .filter(|event| event.message["method"] == "turn/completed")
        .collect::<Vec<_>>();
    assert_eq!(
        cancelled.len(),
        1,
        "one terminal receipt for the cancelled delegation"
    );
    assert_eq!(
        cancelled[0].message["params"]["turn"]["id"],
        "msg_cancelled"
    );
    assert_eq!(
        cancelled[0].message["params"]["turn"]["status"],
        "cancelled"
    );
    assert!(
        events
            .iter()
            .any(|event| event.requested_delegation_id.as_deref() == Some("cancelled-delegation"))
    );
    let collected = crate::ops::codex_voice_lifecycle::collect_normalized_events_for_test(
        events,
        Some("cancelled-delegation"),
    )
    .await;
    assert!(
        collected.iter().any(|event| matches!(
            event,
            crate::ops::codex_voice_lifecycle::AnalystLifecycleEvent::Completed {
                status, delegation_id, ..
            } if status == "cancelled" && delegation_id == "cancelled-delegation"
        )),
        "the pending Voice delegation receives its terminal event"
    );
    let (stop, _) = watch::channel(false);
    let client = OpenCodeClient {
        inner,
        stop,
        lease: None,
    };
    assert_eq!(
        client
            .start_prompt("next-delegation", "next input")
            .await
            .expect("ownership released"),
        "msg_next"
    );
    {
        let mut state = client.inner.state.lock().await;
        enqueue(
            &mut state,
            &client.inner,
            "msg_native_active",
            "native task",
        );
        deliver(&mut state, &client.inner, "msg_native_active");
        drain(&mut receiver);
        observe(
            &mut state,
            &client.inner,
            "session.inbox.cancelled",
            json!({"inboxID":"msg_next"}),
        );
        assert!(state.controlled.is_none());
        assert_eq!(
            state.active.as_ref().expect("native turn retained").id,
            "msg_native_active"
        );
        let events = drain(&mut receiver);
        assert!(
            !events
                .iter()
                .any(|event| event.message["method"] == "turn/started"),
            "queued cancellation cannot announce an overlapping native turn"
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message["params"]["turn"]["id"], "msg_next");
    }
    server.abort();
}

#[tokio::test]
async fn v2_admission_uses_the_servers_ordered_id_and_correlates_events_before_the_rpc_returns() {
    use axum::{Json, Router, extract::State, routing::post};
    use std::future::IntoFuture;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback fixture");
    let (mut inner, mut receiver) = inner();
    inner.endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let inner = Arc::new(inner);
    let app=Router::new().route("/api/session/ses_fixture/prompt",post(|State(inner):State<Arc<Inner>>,Json(body):Json<Value>|async move{
        assert!(body.get("id").is_none(),"the server owns its ordered IDs");
        assert_eq!(body["delivery"],"queue");
        let observer=Arc::clone(&inner);
        tokio::spawn(async move{
            let mut state=observer.state.lock().await;
            enqueue(&mut state,&observer,"msg_178dfd929001RIbVQUVN1hsh03","input");
            deliver(&mut state,&observer,"msg_178dfd929001RIbVQUVN1hsh03");
        });
        Json(json!({"data":{"id":"msg_178dfd929001RIbVQUVN1hsh03","sessionID":"ses_fixture","delivery":"queue"}}))
    })).with_state(Arc::clone(&inner));
    let server = tokio::spawn(axum::serve(listener, app).into_future());
    let (stop, _) = watch::channel(false);
    let client = OpenCodeClient {
        inner,
        stop,
        lease: None,
    };
    let id = client
        .start_prompt("owned-delegation", "input")
        .await
        .expect("durable admission");
    let started = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .expect("start observed")
        .expect("event");
    assert_eq!(
        started.requested_delegation_id.as_deref(),
        Some("owned-delegation")
    );
    assert_eq!(started.message["params"]["turn"]["id"], id);
    server.abort();
}

#[tokio::test]
async fn v2_cancellation_never_substitutes_an_unfenced_session_interrupt() {
    let (inner, _) = inner();
    let inner = Arc::new(inner);
    {
        let mut state = inner.state.lock().await;
        enqueue(&mut state, &inner, "msg_active", "active");
        deliver(&mut state, &inner, "msg_active");
    }
    let (stop, _) = watch::channel(false);
    let client = OpenCodeClient {
        inner,
        stop,
        lease: None,
    };
    assert_eq!(
        client
            .cancel("old-turn")
            .await
            .expect_err("stale turn")
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        client
            .cancel("msg_active")
            .await
            .expect_err("no expected-turn API")
            .kind(),
        io::ErrorKind::Unsupported
    );
}

#[tokio::test]
async fn v2_unresolved_admission_disconnects_without_replaying_or_remaining_busy() {
    use axum::{Router, http::StatusCode, routing::post};
    use std::future::IntoFuture;
    use std::sync::atomic::AtomicUsize;

    for cancel in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("isolated backend");
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let handler_calls = Arc::clone(&calls);
        let handler_entered = Arc::clone(&entered);
        let app = Router::new().route(
            "/api/session/ses_fixture/prompt",
            post(move || {
                let calls = Arc::clone(&handler_calls);
                let entered = Arc::clone(&handler_entered);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    entered.notify_one();
                    if cancel {
                        std::future::pending::<()>().await;
                    }
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }),
        );
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(axum::serve(listener, app).into_future());
        let (mut inner, mut receiver) = inner();
        inner.endpoint = endpoint;
        let (stop, mut stopped) = watch::channel(false);
        let client = OpenCodeClient {
            inner: Arc::new(inner),
            stop,
            lease: None,
        };
        if cancel {
            let request = client.start_prompt("unknown-delegation", "synthetic input");
            tokio::pin!(request);
            tokio::select! {
                result = &mut request => panic!("held request unexpectedly returned: {result:?}"),
                () = entered.notified() => {}
            }
        } else {
            client
                .start_prompt("unknown-delegation", "synthetic input")
                .await
                .expect_err("backend failure");
        }
        assert!(
            !client.running(),
            "unresolved admission cannot stay available"
        );
        stopped.changed().await.expect("observer is stopped");
        assert!(*stopped.borrow());
        let event = receiver.try_recv().expect("explicit lifecycle failure");
        assert_eq!(event.generation, "generation");
        assert_eq!(event.message["method"], "cccc/managedAgent/disconnected");
        assert_eq!(
            client
                .start_prompt("later-delegation", "different input")
                .await
                .expect_err("a new owner is required")
                .kind(),
            io::ErrorKind::BrokenPipe,
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "no hidden replay");
        server.abort();
    }
}
