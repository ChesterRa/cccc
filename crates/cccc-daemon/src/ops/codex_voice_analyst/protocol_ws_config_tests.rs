use super::*;
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::{Error, error::CapacityError};

const HISTORY_BYTES: usize = 20 << 20;

#[tokio::test]
async fn connect_accepts_messages_larger_than_the_old_default_cap() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind server");
    let address = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut server = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept websocket");
        // tungstenite sends this text message as one unfragmented frame.
        server
            .send(Message::Text("x".repeat(HISTORY_BYTES).into()))
            .await
    });

    let mut socket = connect_with_retry(&format!("ws://{address}"))
        .await
        .expect("connect");
    let message = tokio::time::timeout(Duration::from_secs(10), socket.next()).await;
    // Join even on a capacity error, so the test leaves no detached server task.
    drop(socket);
    let sent = server.await.expect("join server");
    let message = message
        .expect("read deadline")
        .expect("stream ended")
        .expect("oversized message must be delivered, not rejected as Capacity");
    sent.expect("send message");
    assert_eq!(message.into_data().len(), HISTORY_BYTES);
}

#[tokio::test]
async fn resume_with_excluded_turns_accepts_large_thread_preview() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind server");
    let address = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut server = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept websocket");
        let request = server.next().await.expect("request").expect("read request");
        let request: Value = serde_json::from_slice(&request.into_data()).expect("request JSON");
        assert_eq!(request["method"], "thread/resume");
        assert_eq!(request["params"]["excludeTurns"], true);
        let response = json!({
            "id": request["id"],
            "result": {"thread": {
                "id": "resumed-thread",
                "preview": "x".repeat(HISTORY_BYTES),
                "turns": [],
            }},
        });
        server
            .send(Message::Text(response.to_string().into()))
            .await
    });

    let socket = connect_with_retry(&format!("ws://{address}"))
        .await
        .expect("connect");
    let client = ProtocolClient::new(socket, "resume-test".into(), None);
    let result = client
        .request(
            "thread/resume",
            json!({"threadId": "resumed-thread", "excludeTurns": true}),
            Duration::from_secs(10),
        )
        .await;
    client.close().await;
    let sent = server.await.expect("join server");
    let result = result.expect("resume must succeed even when metadata exceeds 16 MiB");
    sent.expect("send response");
    assert_eq!(result["thread"]["id"], "resumed-thread");
    assert_eq!(result["thread"]["turns"], json!([]));
    assert_eq!(
        result["thread"]["preview"].as_str().expect("preview").len(),
        HISTORY_BYTES,
    );
}

#[tokio::test]
async fn connect_rejects_frames_above_the_bounded_cap() {
    const MAX_BYTES: usize = 64 << 20;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind server");
    let address = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let server = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept websocket");
        let mut stream = server.into_inner();
        // An unmasked, final text frame advertises an oversized payload. The client
        // must reject the header without needing to receive or allocate the payload.
        let mut header = vec![0x81, 127];
        header.extend_from_slice(&((MAX_BYTES + 1) as u64).to_be_bytes());
        stream.write_all(&header).await.expect("write header");
    });

    let mut socket = connect_with_retry(&format!("ws://{address}"))
        .await
        .expect("connect");
    let message = tokio::time::timeout(Duration::from_secs(10), socket.next()).await;
    drop(socket);
    server.await.expect("join server");
    let error = message
        .expect("read deadline")
        .expect("stream ended")
        .expect_err("frame cap must remain enforced");
    assert!(
        matches!(
            error,
            Error::Capacity(CapacityError::MessageTooLong { size, max_size })
                if size == MAX_BYTES + 1 && max_size == MAX_BYTES
        ),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn connect_rejects_fragmented_messages_above_the_bounded_cap() {
    use tokio_tungstenite::tungstenite::protocol::frame::{
        Frame,
        coding::{Data, OpCode},
    };

    const MAX_BYTES: usize = 64 << 20;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind server");
    let address = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut server = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept websocket");
        // Each frame fits the new frame cap, but their combined message does not.
        server
            .send(Message::Frame(Frame::message(
                vec![b'x'; 40 << 20],
                OpCode::Data(Data::Text),
                false,
            )))
            .await?;
        server
            .send(Message::Frame(Frame::message(
                vec![b'x'; (24 << 20) + 1],
                OpCode::Data(Data::Continue),
                true,
            )))
            .await
    });

    let mut socket = connect_with_retry(&format!("ws://{address}"))
        .await
        .expect("connect");
    let message = tokio::time::timeout(Duration::from_secs(10), socket.next()).await;
    drop(socket);
    let sent = server.await.expect("join server");
    let error = message
        .expect("read deadline")
        .expect("stream ended")
        .expect_err("aggregate message cap must remain enforced");
    assert!(
        matches!(
            error,
            Error::Capacity(CapacityError::MessageTooLong { size, max_size })
                if size == MAX_BYTES + 1 && max_size == MAX_BYTES
        ),
        "unexpected error: {error:?}"
    );
    sent.expect("send fragmented message");
}
