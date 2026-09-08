use anyhow::{Context, Result};
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

use crate::dispatch::dispatch;
use crate::dispatch_concurrency::DispatchLocks;

const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub fn spawn_connection<S>(
    stream: S,
    home: HomeLayout,
    shutdown: watch::Sender<bool>,
    dispatch_locks: DispatchLocks,
) -> tokio::task::JoinHandle<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = handle(stream, home, &shutdown, &dispatch_locks).await {
            tracing::warn!(%error, "daemon connection failed");
        }
    })
}

async fn handle<S>(
    stream: S,
    home: HomeLayout,
    shutdown: &watch::Sender<bool>,
    dispatch_locks: &DispatchLocks,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut stream = BufReader::new(stream);
    loop {
        let mut bytes = Vec::new();
        let mut limited = (&mut stream).take((MAX_REQUEST_BYTES + 1) as u64);
        let count =
            tokio::time::timeout(REQUEST_READ_TIMEOUT, limited.read_until(b'\n', &mut bytes))
                .await
                .context("daemon request read timed out")??;
        if count == 0 {
            break;
        }
        let oversized = bytes.len() > MAX_REQUEST_BYTES;
        if oversized {
            write_response(
                stream.get_mut(),
                &DaemonResponse::failure("request_too_large", "request exceeds 16 MiB"),
            )
            .await?;
            break;
        }
        let request = match serde_json::from_slice::<DaemonRequest>(&bytes) {
            Ok(request) => request,
            Err(error) => {
                write_response(
                    stream.get_mut(),
                    &DaemonResponse::failure("invalid_request", error.to_string()),
                )
                .await?;
                continue;
            }
        };
        if request.op == "events_stream" {
            return crate::server_events_stream::handle(
                stream,
                home,
                request,
                shutdown.subscribe(),
            )
            .await;
        }
        if request.op == "term_attach" {
            return crate::server_terminal_attach::handle(
                stream,
                home,
                request,
                shutdown.subscribe(),
            )
            .await;
        }
        let response = response(&home, request, shutdown, dispatch_locks).await;
        write_response(stream.get_mut(), &response).await?;
    }
    Ok(())
}

async fn response(
    home: &HomeLayout,
    request: DaemonRequest,
    shutdown: &watch::Sender<bool>,
    dispatch_locks: &DispatchLocks,
) -> DaemonResponse {
    let should_shutdown = request.op == "shutdown";
    let permit = dispatch_locks.acquire(&request).await;
    let home = home.clone();
    let request_for_dispatch = request.clone();
    let response = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        dispatch(&home, &request_for_dispatch)
    })
    .await
    .unwrap_or_else(|error| DaemonResponse::failure("dispatch_failed", error.to_string()));
    if should_shutdown && response.ok {
        shutdown.send(true).ok();
    }
    response
}

async fn write_response<W>(write: &mut W, response: &DaemonResponse) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut payload = serde_json::to_vec(response)?;
    payload.push(b'\n');
    write.write_all(&payload).await?;
    write.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::handle;
    use crate::dispatch_concurrency::DispatchLocks;
    use cccc_contracts::Event;
    use cccc_core::{GroupStore, HomeLayout, ledger};
    use serde_json::{Value, json};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::watch;

    #[tokio::test]
    async fn malformed_connection_does_not_panic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let (mut client, server) = tokio::io::duplex(1024);
        let (shutdown, _) = watch::channel(false);
        let lock = DispatchLocks::default();
        let task = tokio::spawn(async move { handle(server, home, &shutdown, &lock).await });
        client.write_all(b"not-json\n").await.expect("write");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .expect("read");
        assert!(response.contains("invalid_request"));
        client.shutdown().await.expect("shutdown");
        assert!(task.await.expect("join").is_ok());
    }

    #[tokio::test]
    async fn connection_handles_multiple_requests() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let (mut client, server) = tokio::io::duplex(4096);
        let (shutdown, _) = watch::channel(false);
        let lock = DispatchLocks::default();
        let task = tokio::spawn(async move { handle(server, home, &shutdown, &lock).await });
        client
            .write_all(
                b"{\"v\":1,\"op\":\"ping\",\"args\":{}}\n{\"v\":1,\"op\":\"ping\",\"args\":{}}\n",
            )
            .await
            .expect("write");
        let mut reader = BufReader::new(&mut client);
        for _ in 0..2 {
            let mut response = String::new();
            reader.read_line(&mut response).await.expect("read");
            let response: cccc_contracts::DaemonResponse =
                serde_json::from_str(&response).expect("response");
            assert!(response.ok);
        }
        drop(reader);
        client.shutdown().await.expect("shutdown");
        assert!(task.await.expect("join").is_ok());
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn raw_terminal_write_does_not_block_actor_stop_dispatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("backpressured input", "").expect("group");
        store
            .mutate(&group.group_id, |doc| {
                let mut actor = cccc_contracts::Actor::new("peer");
                actor.runtime = cccc_contracts::ActorRuntime::Custom;
                actor.runner = cccc_contracts::RunnerKind::Pty;
                cccc_core::actors::add(doc, actor)
            })
            .expect("actor");
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(), actor_id: "peer".into(),
            runner: cccc_contracts::RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), "stty raw -echo; touch ready; while [ ! -f release ]; do sleep 0.02; done; dd bs=65536 count=16 iflag=fullblock of=/dev/null 2>/dev/null".into()],
            cwd: temp.path().into(), env: Default::default(), cols: 80, rows: 24,
        }).expect("runtime");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !temp.path().join("ready").exists() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let locks = DispatchLocks::default();
        let (shutdown, _) = watch::channel(false);
        let input = serde_json::from_value(json!({"v":1,"op":"terminal_write","args":{
            "group_id":group.group_id,"actor_id":"peer","data":"x".repeat(1024*1024)
        }}))
        .expect("input request");
        let (input_home, input_shutdown, input_locks) =
            (home.clone(), shutdown.clone(), locks.clone());
        let writing = tokio::spawn(async move {
            super::response(&input_home, input, &input_shutdown, &input_locks).await
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let backpressured = !writing.is_finished();
        let stop = serde_json::from_value(json!({"v":1,"op":"actor_stop","args":{
            "group_id":group.group_id,"actor_id":"peer","by":"user"
        }}))
        .expect("stop request");
        let stopped = tokio::time::timeout(
            Duration::from_secs(2),
            super::response(&home, stop, &shutdown, &locks),
        )
        .await;
        std::fs::write(temp.path().join("release"), b"").expect("release fixture");
        let written = writing.await.expect("writer task");
        let _ = cccc_runtime::stop(&group.group_id, "peer");
        assert!(backpressured, "fixture must block a real input write");
        assert!(
            stopped
                .expect("stop must remain dispatchable during raw input")
                .ok
        );
        assert!(
            !written.ok,
            "interrupted input must not be reported fully written"
        );
    }

    #[tokio::test]
    async fn catalog_dispatch_keeps_permissions_without_outer_lifecycle_locks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("catalog", "").expect("group");
        let locks = DispatchLocks::default();
        let mutation: cccc_contracts::DaemonRequest = serde_json::from_value(json!({
            "v":1,"op":"settings_update","args":{}
        }))
        .expect("mutation");
        let _global = locks.acquire(&mutation).await;
        let (shutdown, _) = watch::channel(false);
        for (by, ok) in [("peer", true), ("another_actor", false)] {
            let request = serde_json::from_value(json!({
                "v":1,"op":"capability_state",
                "args":{"group_id":group.group_id,"actor_id":"peer","by":by,"view":"mcp_catalog"}
            }))
            .expect("request");
            let response = tokio::time::timeout(
                Duration::from_secs(2),
                super::response(&home, request, &shutdown, &locks),
            )
            .await
            .expect("catalog dispatch is independent of the outer lifecycle lock");
            assert_eq!(response.ok, ok);
            if ok {
                assert!(response.result["visible_tools"].is_array());
            } else {
                assert_eq!(response.error.expect("error").code, "permission_denied");
            }
        }
    }

    #[tokio::test]
    async fn connection_upgrades_to_the_events_stream() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("events stream", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
        let (mut client, server) = tokio::io::duplex(16 * 1024);
        let (shutdown, _) = watch::channel(false);
        let lock = DispatchLocks::default();
        let task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { handle(server, home, &shutdown, &lock).await }
        });
        client
            .write_all(
                format!(
                    "{{\"v\":1,\"op\":\"events_stream\",\"args\":{{\"group_id\":\"{}\"}}}}\n",
                    group.group_id
                )
                .as_bytes(),
            )
            .await
            .expect("write stream request");
        let mut reader = BufReader::new(&mut client);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("handshake");
        let handshake: cccc_contracts::DaemonResponse =
            serde_json::from_str(&line).expect("handshake JSON");
        assert!(handshake.ok, "{:?}", handshake.error);

        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"text":"live","message_mode":"send","to":["@all"]})
            .as_object()
            .cloned()
            .expect("event data");
        ledger::append(&ledger_path, &event).expect("append live event");
        line.clear();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
            .await
            .expect("stream item timeout")
            .expect("stream item");
        let item: Value = serde_json::from_str(&line).expect("stream item JSON");
        assert_eq!(item["t"], "event");
        assert_eq!(item["event"]["id"], event.id);

        drop(reader);
        client.shutdown().await.expect("client shutdown");
        assert!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .expect("closed stream retires promptly")
                .expect("join")
                .is_ok()
        );
    }

    #[tokio::test]
    async fn mismatched_shutdown_fence_does_not_signal_the_daemon() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let (mut client, server) = tokio::io::duplex(1024);
        let (shutdown, receiver) = watch::channel(false);
        let lock = DispatchLocks::default();
        let task = tokio::spawn(async move { handle(server, home, &shutdown, &lock).await });
        client
            .write_all(
                format!(
                    "{{\"v\":1,\"op\":\"shutdown\",\"args\":{{\"expected_pid\":{}}}}}\n",
                    u64::from(std::process::id()) + 1
                )
                .as_bytes(),
            )
            .await
            .expect("write");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .expect("read");
        let response: cccc_contracts::DaemonResponse =
            serde_json::from_str(&response).expect("response");
        assert_eq!(
            response.error.expect("owner mismatch").code,
            "daemon_owner_mismatch"
        );
        assert!(!*receiver.borrow());
        client.shutdown().await.expect("shutdown connection");
        assert!(task.await.expect("join").is_ok());
    }
}
