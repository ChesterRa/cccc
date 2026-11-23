//! Read-only, bounded qualification. Never infer an application's origin from
//! missing readiness, and never retain a queue's native input payload.
use super::*;
use cccc_contracts::source_control::ReceiveState;

pub async fn native_codex_receive_state(
    program: &Path,
    home: &Path,
    cwd: &Path,
    thread: &str,
) -> ReceiveState {
    let check = async {
        let mut proxy = match NativeProxy::connect_with_diagnostic(program, home, cwd, true).await {
            Ok(proxy) => proxy,
            Err(error) => {
                return Ok(if error.kind() == io::ErrorKind::NotConnected {
                    ReceiveState::SharedBackendMissing
                } else {
                    ReceiveState::Unknown
                });
            }
        };
        qualify(&mut proxy.socket, thread).await
    };
    match tokio::time::timeout(Duration::from_secs(2), check).await {
        Ok(Ok(state)) => state,
        _ => ReceiveState::ReceiveCheckFailed,
    }
}

async fn qualify<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    thread: &str,
) -> io::Result<ReceiveState> {
    let mut id = 1;
    let mut params = json!({});
    let mut cursors = HashSet::new();
    loop {
        id += 1;
        let Some(page): Option<Page<String>> =
            request(socket, id, "thread/loaded/list", params).await?
        else {
            return Ok(ReceiveState::ReceiveCheckFailed);
        };
        let cursor = page.cursor()?;
        if page.data.iter().any(|id| id == thread) {
            break;
        }
        let Some(cursor) = cursor else {
            return Ok(ReceiveState::ThreadNotLoaded);
        };
        if !cursors.insert(cursor.to_owned()) {
            return Err(invalid_response());
        }
        params = json!({"cursor":cursor});
    }
    let queue: Option<Page<IgnoredAny>> = request(
        socket,
        id + 1,
        "thread/queue/list",
        json!({"threadId":thread,"limit":1}),
    )
    .await?;
    match queue {
        Some(page) => {
            page.cursor()?;
            Ok(ReceiveState::Ready)
        }
        None => Ok(ReceiveState::ReceiveCheckFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::Role;

    #[cfg(unix)]
    #[tokio::test]
    async fn only_positive_missing_backend_evidence_gets_that_label() {
        use std::os::unix::fs::PermissionsExt;
        for (message, expected) in [
            (
                "background server is not running",
                ReceiveState::SharedBackendMissing,
            ),
            ("PRIVATE_PROVIDER_ERROR", ReceiveState::Unknown),
        ] {
            let temp = tempfile::tempdir().expect("provider fixture");
            let program = temp.path().join("native-proxy");
            std::fs::write(
                &program,
                format!("#!/bin/sh\nprintf '%s\\n' '{message}' >&2\nexit 1\n"),
            )
            .expect("fixture");
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
                .expect("executable");
            assert_eq!(
                native_codex_receive_state(&program, temp.path(), temp.path(), "native").await,
                expected
            );
        }
    }

    #[tokio::test]
    async fn an_available_queue_cannot_qualify_a_thread_not_loaded_by_the_shared_server() {
        let (a, b) = tokio::io::duplex(8192);
        let mut client = WebSocketStream::from_raw_socket(a, Role::Client, None).await;
        let mut server = WebSocketStream::from_raw_socket(b, Role::Server, None).await;
        let task = tokio::spawn(async move {
            let loaded = server.next().await.expect("request").expect("frame");
            let loaded: Value =
                serde_json::from_str(loaded.to_text().expect("text")).expect("JSON");
            assert_eq!(loaded["method"], "thread/loaded/list");
            server
                .send(Message::Text(
                    json!({"id":loaded["id"],"result":{"data":[],"nextCursor":null}})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("loaded reply");
            // The very same server supports queue/list and returns success with
            // native text. That fact must not override missing loaded membership.
            let queue = server.next().await.expect("queue control").expect("frame");
            let queue: Value = serde_json::from_str(queue.to_text().expect("text")).expect("JSON");
            assert_eq!(queue["method"], "thread/queue/list");
            server.send(Message::Text(json!({"id":queue["id"],"result":{"data":[{"text":"PRIVATE_NATIVE_PAYLOAD"}],"nextCursor":null}}).to_string().into())).await.expect("queue available");
        });
        assert_eq!(
            qualify(&mut client, "native").await.expect("diagnose"),
            ReceiveState::ThreadNotLoaded
        );
        let queue: Option<Page<IgnoredAny>> = request(
            &mut client,
            99,
            "thread/queue/list",
            json!({"threadId":"native","limit":1}),
        )
        .await
        .expect("independent queue success");
        assert!(queue.is_some());
        task.await.expect("fixture");
    }
}
