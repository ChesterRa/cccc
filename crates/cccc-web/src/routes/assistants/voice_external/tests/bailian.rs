use super::*;

#[tokio::test]
async fn bailian_waits_for_admission_streams_pcm_and_keeps_late_final_results() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket =
            tokio_tungstenite::accept_hdr_async(stream, |request: &Request, response: Response| {
                assert_eq!(
                    request.headers()["authorization"],
                    "Bearer test-bailian-key"
                );
                Ok(response)
            })
            .await
            .expect("upgrade");
        let start: Value = serde_json::from_str(
            socket
                .next()
                .await
                .expect("start")
                .expect("frame")
                .to_text()
                .expect("json"),
        )
        .expect("start json");
        assert_eq!(start["header"]["action"], "run-task");
        assert_eq!(start["payload"]["model"], "fun-asr-realtime");
        assert_eq!(start["payload"]["parameters"]["sample_rate"], 16000);
        let task = start["header"]["task_id"].as_str().expect("task id");
        socket
            .send(Message::Text(
                json!({"header":{"task_id":task,"event":"task-started"}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("admit");
        assert_eq!(
            socket
                .next()
                .await
                .expect("audio")
                .expect("frame")
                .into_data()
                .len(),
            6400
        );
        socket
            .send(bailian_sentence(task, 1, "你好", true))
            .await
            .expect("final sentence");
        socket
            .send(bailian_sentence(task, 1, "你好", true))
            .await
            .expect("duplicate sentence");
        assert_eq!(
            socket
                .next()
                .await
                .expect("tail")
                .expect("frame")
                .into_data()
                .len(),
            2
        );
        let stop: Value = serde_json::from_str(
            socket
                .next()
                .await
                .expect("stop")
                .expect("frame")
                .to_text()
                .expect("json"),
        )
        .expect("stop json");
        assert_eq!(stop["header"]["action"], "finish-task");
        assert_eq!(stop["header"]["task_id"], task);
        socket
            .send(bailian_sentence(task, 2, "世界", true))
            .await
            .expect("late final");
        socket
            .send(Message::Text(
                json!({"header":{"task_id":task,"event":"task-finished"}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("finished");
    });
    let opened = connection::connect_at(
        Provider::Bailian,
        config("test-bailian-key"),
        "zh-CN",
        &endpoint,
    )
    .await
    .expect("connect");
    let (_temp, home) = home();
    let mut active = active::Active::from_opened(
        &home,
        &json!({"session_id":"session-test","capture_mode":"prompt","dispatch_target":"composer"}),
        opened,
    )
    .expect("active");
    active.audio(&vec![0; 6402]).await.expect("send PCM");
    let mut finals = Vec::new();
    for _ in 0..2 {
        let message = active
            .reader
            .next()
            .await
            .expect("response")
            .expect("frame");
        finals.extend(active.receive(message).expect("parse"));
    }
    assert_eq!(
        finals.len(),
        1,
        "a repeated final sentence must not duplicate input"
    );
    active.stop(json!(9)).await.expect("finish");
    while !active.completed {
        let message = active
            .reader
            .next()
            .await
            .expect("response")
            .expect("frame");
        finals.extend(active.receive(message).expect("parse"));
    }
    assert_eq!(finals.len(), 2);
    assert_eq!(active.transcript.text(), "你好\n世界");
    assert_eq!(
        active.transcript.final_event(&active.model, true, json!(9))["partial"],
        false
    );
    assert!(
        !active.persist,
        "direct dictation must not create meeting artifacts"
    );
    server.await.expect("server");
}

fn bailian_sentence(task: &str, id: u64, text: &str, finalized: bool) -> Message {
    Message::Text(json!({"header":{"task_id":task,"event":"result-generated"},"payload":{"output":{"sentence":{
        "sentence_id":id,"begin_time":id*100,"end_time":id*100+80,"text":text,"sentence_end":finalized
    }}}}).to_string().into())
}

#[tokio::test]
async fn stopping_without_audio_completes_without_an_empty_provider_recognition() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("upgrade");
        let start: Value = serde_json::from_str(
            socket
                .next()
                .await
                .expect("start")
                .expect("frame")
                .to_text()
                .expect("text"),
        )
        .expect("json");
        socket
            .send(Message::Text(
                json!({"header":{"task_id":start["header"]["task_id"],"event":"task-started"}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("admit");
        assert!(
            !matches!(
                socket.next().await,
                Some(Ok(Message::Text(_) | Message::Binary(_)))
            ),
            "empty capture must not upload or finish an empty recognition task"
        );
    });
    let opened = connection::connect_at(Provider::Bailian, config("test-key"), "auto", &endpoint)
        .await
        .expect("connect");
    let (_temp, home) = home();
    let mut active = active::Active::from_opened(&home, &json!({"capture_mode":"prompt"}), opened)
        .expect("active");
    active.stop(json!(2)).await.expect("stop");
    assert!(active.completed);
    assert!(active.transcript.text().is_empty());
    drop(active);
    server.await.expect("server");
}
