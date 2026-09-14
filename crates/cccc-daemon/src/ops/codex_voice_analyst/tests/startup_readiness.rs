use super::super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn new_session_is_not_ready_until_the_same_thread_can_resume() {
    for fail_resume in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let endpoint = format!(
            "ws://{}",
            listener.local_addr().expect("read listener address")
        );
        let checked = Arc::new(AtomicBool::new(false));
        let observed = checked.clone();
        // Mock only the provider boundary: creation does not materialize history.
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept test connection");
            let mut socket = accept_async(stream)
                .await
                .expect("complete accept async in fixture");
            let mut materialized = false;
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let request: Value =
                    serde_json::from_str(&text).expect("complete from str in fixture");
                let result = match request["method"]
                    .as_str()
                    .expect("complete as str in fixture")
                {
                    "initialize" => json!({}),
                    "thread/start" => json!({"thread":{"id":"new-thread"}}),
                    "thread/name/set" => {
                        assert_eq!(request["params"]["threadId"], "new-thread");
                        materialized = true;
                        json!({})
                    }
                    "thread/read" => {
                        assert!(materialized);
                        assert_eq!(request["params"]["threadId"], "new-thread");
                        assert_eq!(request["params"]["includeTurns"], true);
                        observed.store(true, Ordering::SeqCst);
                        if fail_resume {
                            socket.send(Message::Text(json!({"id":request["id"],"error":{"code":-32600,"message":"no rollout found"}}).to_string().into())).await.expect("send test event");
                            continue;
                        }
                        json!({"thread":{"id":"new-thread","turns":[]}})
                    }
                    other => panic!("startup must not send model work: {other}"),
                };
                socket
                    .send(Message::Text(
                        json!({"id":request["id"],"result":result})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .expect("send test event");
            }
        });
        let result = AnalystSession::connect_for_test(
            WorkspaceBinding {
                root: std::env::current_dir().expect("complete current dir in fixture"),
            },
            "probe".into(),
            endpoint,
            "codex".into(),
        )
        .await;
        assert!(
            checked.load(Ordering::SeqCst),
            "startup exposed an unresumable thread"
        );
        assert_eq!(result.is_err(), fail_resume);
        drop(result);
        server.abort();
    }
}
