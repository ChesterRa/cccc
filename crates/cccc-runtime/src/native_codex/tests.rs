use super::*;
use std::sync::Arc;
use tokio::io::DuplexStream;
use tokio_tungstenite::tungstenite::protocol::Role;

async fn pair() -> (WebSocketStream<DuplexStream>, WebSocketStream<DuplexStream>) {
    let (client, server) = tokio::io::duplex(8192);
    (
        WebSocketStream::from_raw_socket(client, Role::Client, None).await,
        WebSocketStream::from_raw_socket(server, Role::Server, None).await,
    )
}

async fn incoming(socket: &mut WebSocketStream<DuplexStream>, method: &str) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("bounded fixture request")
        .expect("request frame")
        .expect("WebSocket frame");
    let request: Value =
        serde_json::from_str(message.to_text().expect("text frame")).expect("JSON");
    assert_eq!(request["method"], method);
    request
}

async fn answer(socket: &mut WebSocketStream<DuplexStream>, id: &Value, result: Value) {
    socket
        .send(Message::Text(
            json!({"id":id,"result":result}).to_string().into(),
        ))
        .await
        .expect("fixture reply");
}

#[tokio::test]
async fn qualification_uses_metadata_only_and_returns_the_exact_current_turn() {
    let (mut client, mut server) = pair().await;
    let peer = tokio::spawn(async move {
        let request = incoming(&mut server, "thread/read").await;
        assert_eq!(
            request["params"],
            json!({"threadId":"native","includeTurns":false})
        );
        answer(
            &mut server,
            &request["id"],
            json!({"thread":{
                "canAcceptDirectInput":true,"status":{"type":"active","activeFlags":[]}
            }}),
        )
        .await;
        let request = incoming(&mut server, "thread/turns/list").await;
        assert_eq!(
            request["params"],
            json!({"threadId":"native","limit":1,"sortDirection":"desc","itemsView":"notLoaded"})
        );
        answer(&mut server, &request["id"], json!({"data":[{"id":"exact-turn","status":"inProgress","items":[]}],"nextCursor":null})).await;
    });
    assert_eq!(
        prepare_steer(&mut client, "native")
            .await
            .expect("metadata"),
        Some("exact-turn".into())
    );
    peer.await.expect("fixture");
}

#[tokio::test]
async fn waiting_unknown_or_idle_metadata_never_submits_input() {
    for thread in [
        json!({"canAcceptDirectInput":true,"status":{"type":"active","activeFlags":["waitingOnApproval"]}}),
        json!({"canAcceptDirectInput":true,"status":{"type":"active","activeFlags":["waitingOnUserInput"]}}),
        json!({"canAcceptDirectInput":true,"status":{"type":"active","activeFlags":["future-flag"]}}),
        json!({"canAcceptDirectInput":true,"status":{"type":"active"}}),
        json!({"canAcceptDirectInput":null,"status":{"type":"active","activeFlags":[]}}),
        json!({"canAcceptDirectInput":false,"status":{"type":"active","activeFlags":[]}}),
        json!({"canAcceptDirectInput":true,"status":{"type":"idle"}}),
    ] {
        let (mut client, mut server) = pair().await;
        let peer = tokio::spawn(async move {
            let request = incoming(&mut server, "thread/read").await;
            answer(&mut server, &request["id"], json!({"thread":thread})).await;
        });
        assert_eq!(
            prepare_steer(&mut client, "native")
                .await
                .expect("read-only"),
            None
        );
        peer.await.expect("fixture");
    }
}

#[tokio::test]
async fn missing_or_settled_turn_is_not_an_admission_target() {
    for data in [
        json!([]),
        json!([{"id":"ended","status":"completed"}]),
        json!([{"id":"","status":"inProgress"}]),
    ] {
        let (mut client, mut server) = pair().await;
        let peer = tokio::spawn(async move {
            let request = incoming(&mut server, "thread/read").await;
            answer(&mut server, &request["id"], json!({"thread":{"canAcceptDirectInput":true,"status":{"type":"active","activeFlags":[]}}})).await;
            let request = incoming(&mut server, "thread/turns/list").await;
            answer(
                &mut server,
                &request["id"],
                json!({"data":data,"nextCursor":null}),
            )
            .await;
        });
        assert_eq!(
            prepare_steer(&mut client, "native")
                .await
                .expect("read-only"),
            None
        );
        peer.await.expect("fixture");
    }
}

#[tokio::test]
async fn steering_requires_exact_receipt_and_preserves_the_whole_input() {
    let (mut client, mut server) = pair().await;
    let peer = tokio::spawn(async move {
        let request = incoming(&mut server, "turn/steer").await;
        assert_eq!(
            request["params"],
            json!({"threadId":"native","expectedTurnId":"exact-turn",
            "input":[{"type":"text","text":"first\n界😀 \"quoted\""}]})
        );
        answer(&mut server, &request["id"], json!({"turnId":"exact-turn"})).await;
    });
    assert_eq!(
        submit_steer(
            &mut client,
            "native",
            "exact-turn",
            "first\n界😀 \"quoted\"",
            &AtomicBool::new(false)
        )
        .await,
        NativeCodexSteerOutcome::Accepted
    );
    peer.await.expect("fixture");
}

#[tokio::test]
async fn only_definitive_rejection_can_use_the_ordinary_queue() {
    use NativeCodexSteerOutcome::{Ambiguous, NotSubmitted};
    for (code, expected) in [
        (-32600, NotSubmitted),
        (-32601, NotSubmitted),
        (-32602, Ambiguous),
        (-32603, Ambiguous),
    ] {
        let (mut client, mut server) = pair().await;
        let peer = tokio::spawn(async move {
            let request = incoming(&mut server, "turn/steer").await;
            server.send(Message::Text(json!({"id":request["id"],"error":{"code":code,"message":"private native diagnostic"}}).to_string().into())).await.expect("error response");
        });
        assert_eq!(
            submit_steer(
                &mut client,
                "native",
                "exact-turn",
                "input",
                &AtomicBool::new(false)
            )
            .await,
            expected
        );
        peer.await.expect("fixture");
    }
}

#[tokio::test]
async fn malformed_or_mismatched_success_is_ambiguous() {
    for response in [json!({"turnId":"another-turn"}), json!({}), Value::Null] {
        let (mut client, mut server) = pair().await;
        let peer = tokio::spawn(async move {
            let request = incoming(&mut server, "turn/steer").await;
            answer(&mut server, &request["id"], response).await;
        });
        assert_eq!(
            submit_steer(
                &mut client,
                "native",
                "exact-turn",
                "input",
                &AtomicBool::new(false)
            )
            .await,
            NativeCodexSteerOutcome::Ambiguous
        );
        peer.await.expect("fixture");
    }
}

#[tokio::test]
async fn cancellation_after_write_or_disconnection_does_not_authorize_replay() {
    for cancel in [true, false] {
        let (mut client, mut server) = pair().await;
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        let peer = tokio::spawn(async move {
            incoming(&mut server, "turn/steer").await;
            if cancel {
                signal.store(true, Ordering::Release);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        assert_eq!(
            submit_steer(&mut client, "native", "exact-turn", "input", &cancelled).await,
            NativeCodexSteerOutcome::Ambiguous
        );
        peer.await.expect("fixture");
    }
}

#[tokio::test]
async fn cancellation_before_write_is_unsubmitted() {
    let (mut client, _server) = pair().await;
    assert_eq!(
        submit_steer(
            &mut client,
            "native",
            "exact-turn",
            "input",
            &AtomicBool::new(true)
        )
        .await,
        NativeCodexSteerOutcome::NotSubmitted
    );
}

#[tokio::test]
async fn successive_inputs_use_the_same_turn_and_do_not_follow_a_replacement() {
    let (mut client, mut server) = pair().await;
    let peer = tokio::spawn(async move {
        for index in 0..3 {
            let request = incoming(&mut server, "turn/steer").await;
            assert_eq!(request["params"]["expectedTurnId"], "exact-turn");
            if index < 2 {
                answer(&mut server, &request["id"], json!({"turnId":"exact-turn"})).await;
            } else {
                server.send(Message::Text(json!({"id":request["id"],"error":{"code":-32600,"message":"active turn changed"}}).to_string().into())).await.expect("definitive rejection");
            }
        }
    });
    for expected in [
        NativeCodexSteerOutcome::Accepted,
        NativeCodexSteerOutcome::Accepted,
        NativeCodexSteerOutcome::NotSubmitted,
    ] {
        assert_eq!(
            submit_steer(
                &mut client,
                "native",
                "exact-turn",
                "input",
                &AtomicBool::new(false)
            )
            .await,
            expected
        );
    }
    peer.await.expect("fixture");
}

#[tokio::test]
async fn unanswered_write_has_a_bounded_ambiguous_outcome() {
    let (mut client, mut server) = pair().await;
    let peer = tokio::spawn(async move {
        incoming(&mut server, "turn/steer").await;
        tokio::time::sleep(Duration::from_millis(10100)).await;
    });
    assert_eq!(
        submit_steer(
            &mut client,
            "native",
            "exact-turn",
            "input",
            &AtomicBool::new(false)
        )
        .await,
        NativeCodexSteerOutcome::Ambiguous
    );
    peer.await.expect("fixture");
}
