use super::super::super::opencode::stream::SessionStream;
use super::*;

async fn observe(
    stream: &mut SessionStream,
    control: &super::super::super::acp::AcpLifecycleControl,
    event: Value,
) {
    stream
        .observe(&event, FAKE_SESSION_ID, control)
        .await
        .expect("backend event");
}

fn user(id: &str, text: &str) -> [Value; 2] {
    [
        json!({"type":"message.updated", "properties":{"info":{"sessionID":FAKE_SESSION_ID,"id":id,"role":"user"}}}),
        json!({"type":"message.part.updated", "properties":{"part":{"sessionID":FAKE_SESSION_ID,"id":format!("{id}-text"),"messageID":id,"type":"text","text":text}}}),
    ]
}

fn assistant(id: &str, parent: &str) -> Value {
    json!({"type":"message.updated", "properties":{"info":{"sessionID":FAKE_SESSION_ID,"id":id,"parentID":parent,"role":"assistant"}}})
}

fn part(message: &str, text: &str) -> Value {
    json!({"type":"message.part.updated", "properties":{"part":{"sessionID":FAKE_SESSION_ID,"id":format!("{message}-text"),"messageID":message,"type":"text","text":text}}})
}

fn status(busy: bool) -> Value {
    json!({"type":"session.status", "properties":{"sessionID":FAKE_SESSION_ID,"status":{"type":if busy {"busy"} else {"idle"}}}})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_native_output_survives_early_rpc_and_previous_idle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = fake_acp_without_user_echo(temp.path(), true);
    let (protocol, process) =
        launch_fake_acp(temp.path(), &executable, PromptCompletion::SessionEvents).await;
    let mut events = protocol.subscribe();
    let control = protocol.lifecycle_control();
    let mut stream = SessionStream::default();
    let prompt = protocol.start_prompt(FAKE_SESSION_ID, "controlled", "owned prompt");
    tokio::pin!(prompt);
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut prompt)
            .await
            .is_err(),
        "an early successful RPC must not bypass persisted admission"
    );
    for event in user("u1", "owned prompt") {
        observe(&mut stream, &control, event).await;
    }
    let old_turn = prompt.await.expect("persisted prompt admitted");
    observe(&mut stream, &control, status(true)).await;
    observe(&mut stream, &control, assistant("a1", "u1")).await;
    observe(&mut stream, &control, part("a1", "old answer")).await;
    protocol
        .register_native_input("native", "follow up")
        .await
        .expect("register native");
    for event in user("u2", "follow up") {
        observe(&mut stream, &control, event).await;
    }
    observe(&mut stream, &control, status(false)).await;
    observe(&mut stream, &control, status(true)).await;
    observe(&mut stream, &control, assistant("a2", "u2")).await;
    observe(&mut stream, &control, part("a2", "new ")).await;
    observe(&mut stream, &control, json!({"type":"message.part.delta", "properties":{
        "sessionID":FAKE_SESSION_ID,"messageID":"a2","partID":"a2-text","field":"text","delta":"answer"
    }})).await;
    // A complete part snapshot repeats the already delivered prefix.
    observe(&mut stream, &control, part("a2", "new answer")).await;
    observe(&mut stream, &control, status(false)).await;

    let mut texts = HashMap::<String, String>::new();
    let mut completed = Vec::new();
    let mut native_turn = None;
    tokio::time::timeout(Duration::from_secs(2), async {
        while completed.len() < 2 {
            let event = events.recv().await.expect("projected event");
            let params = &event.message["params"];
            if event.requested_delegation_id.as_deref() == Some("native") {
                assert_eq!(
                    completed,
                    [old_turn.clone()],
                    "native receipt must follow the old turn's terminal"
                );
                native_turn = Some(
                    params["turnId"]
                        .as_str()
                        .expect("native attachment")
                        .to_owned(),
                );
            }
            match event.message["method"].as_str() {
                Some("item/agentMessage/delta") => texts
                    .entry(params["turnId"].as_str().expect("delta turn id").into())
                    .or_default()
                    .push_str(params["delta"].as_str().expect("delta text")),
                Some("turn/completed") => completed.push(
                    params["turn"]["id"]
                        .as_str()
                        .expect("completed turn id")
                        .to_owned(),
                ),
                _ => {}
            }
        }
    })
    .await
    .expect("both turns finish");
    let native_turn = native_turn.expect("native correlation");
    assert_ne!(native_turn, old_turn);
    assert_eq!(texts[&old_turn], "old answer");
    assert_eq!(texts[&native_turn], "new answer");
    assert_eq!(completed, [old_turn, native_turn]);
    protocol.close().await;
    process.stop().expect("stop fake ACP");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_consumption_and_provider_failure_follow_the_same_stream() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = fake_acp_without_user_echo(temp.path(), true);
    let (protocol, process) =
        launch_fake_acp(temp.path(), &executable, PromptCompletion::SessionEvents).await;
    let mut events = protocol.subscribe();
    let control = protocol.lifecycle_control();
    let mut stream = SessionStream::default();
    for (id, error_name, expected_status) in [
        ("cancel", "MessageAbortedError", "cancelled"),
        ("fail", "APIError", "failed"),
    ] {
        protocol
            .register_native_input(id, id)
            .await
            .expect("register native");
        for event in user(id, id) {
            observe(&mut stream, &control, event).await;
        }
        // A persisted/noReply user message alone must not create work.
        assert!(events.try_recv().is_err());
        observe(&mut stream, &control, status(true)).await;
        observe(&mut stream, &control, assistant(id, id)).await;
        observe(&mut stream, &control, part(id, "partial")).await;
        // Provider completion hooks can rewrite stored text. This is not a
        // second additive chunk, and must not break the lifecycle subscription.
        observe(
            &mut stream,
            &control,
            part(id, "rewritten by a completion plugin"),
        )
        .await;
        let mut failed = assistant(id, id);
        failed["properties"]["info"]["error"] =
            json!({"name":error_name,"data":{"message":"provider stopped"}});
        observe(&mut stream, &control, failed).await;
        observe(&mut stream, &control, status(false)).await;
        let done = next_method(&mut events, "turn/completed").await;
        assert_eq!(done.message["params"]["turn"]["status"], expected_status);
    }
    protocol.close().await;
    process.stop().expect("stop fake ACP");
}
