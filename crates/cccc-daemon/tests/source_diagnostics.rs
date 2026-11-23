//! Real CLI proxy WebSocket protocol; the native provider is a read-only fixture.
#![cfg(unix)]
use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, local_sessions};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

async fn call(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
    let home = home.clone();
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("args").clone(),
    };
    tokio::task::spawn_blocking(move || cccc_daemon::handle_request(&home, &request))
        .await
        .expect("dispatcher")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn explicit_diagnostics_refresh_only_existing_readiness_and_polling_never_probes() {
    let temp = tempfile::tempdir().expect("Home");
    let home = HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("init");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider boundary");
    let port = listener.local_addr().expect("address").port();
    let connections = Arc::new(AtomicUsize::new(0));
    let observed = connections.clone();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let server_entered = entered.clone();
    let server_release = release.clone();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("proxy");
            observed.fetch_add(1, Ordering::SeqCst);
            let entered = server_entered.clone();
            let release = server_release.clone();
            tokio::spawn(async move {
                let mut socket = tokio_tungstenite::accept_async(stream).await.expect("WS");
                while let Some(Ok(message)) = socket.next().await {
                    if !message.is_text() {
                        continue;
                    }
                    let request: Value =
                        serde_json::from_str(message.to_text().expect("text")).expect("request");
                    let method = request["method"].as_str().expect("method");
                    if method == "initialized" {
                        continue;
                    }
                    let result = match method {
                        "initialize" => json!({}),
                        "thread/loaded/list" => {
                            json!({"data":["ready","failed","slow","busy"],"nextCursor":null})
                        }
                        "thread/queue/list" if request["params"]["threadId"] == "failed" => {
                            let _=socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":request["id"],"error":{"code":-32603,"message":"PRIVATE_ERROR_SECRET"}}).to_string().into())).await;
                            continue;
                        }
                        "thread/queue/list" if request["params"]["threadId"] == "slow" => {
                            entered.notify_one();
                            release.notified().await;
                            json!({"data":[],"nextCursor":null})
                        }
                        "thread/queue/list" => {
                            json!({"data":[{"text":"PRIVATE_QUEUE_PAYLOAD"}],"nextCursor":null})
                        }
                        _ => panic!("unexpected native action: {method}"),
                    };
                    if socket
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            json!({"id":request["id"],"result":result})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    });
    let program = temp.path().join("proxy-fixture");
    std::fs::write(&program,format!("#!/usr/bin/python3\nimport os, socket, select\ns=socket.create_connection(('127.0.0.1',{port}))\nwhile True:\n for ready in select.select([0,s],[],[])[0]:\n  data=os.read(0,65536) if ready==0 else s.recv(65536)\n  if not data: raise SystemExit(0)\n  if ready==0: s.sendall(data)\n  else: os.write(1,data)\n")).expect("proxy fixture");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let host = local_sessions::NativeHost::capture(std::process::id()).expect("host");
    for id in ["missing", "ready", "failed", "slow", "busy"] {
        let source:local_sessions::NativeSession=serde_json::from_value(json!({"source_id":format!("codex:{id}"),"runtime":"codex","native_session_id":id,"host":host,"cwd":temp.path(),"title":id,"revision":format!("revision-{id}"),"codex_can_receive":id=="missing","codex_program":program,"codex_home":temp.path()})).expect("source");
        local_sessions::write(&home, source).expect("register");
    }
    for _ in 0..3 {
        assert!(
            call(&home, "local_session_list", json!({"include_offline":true}))
                .await
                .ok
        );
    }
    assert_eq!(
        connections.load(Ordering::SeqCst),
        0,
        "ordinary polling must never probe"
    );
    for (id, state, ready) in [
        ("missing", "thread_not_loaded", false),
        ("ready", "ready", true),
        ("failed", "receive_check_failed", false),
    ] {
        let source_id = format!("codex:{id}");
        let before = local_sessions::read(&home, &source_id).expect("before");
        let response = call(
            &home,
            "local_session_recheck",
            json!({"source_id":source_id,"by":"user"}),
        )
        .await;
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result["diagnostic"]["state"], state);
        let rendered = serde_json::to_string(&response).expect("safe projection");
        assert!(!rendered.contains("PRIVATE_ERROR_SECRET"));
        assert!(!rendered.contains("PRIVATE_QUEUE_PAYLOAD"));
        let after = local_sessions::read(&home, &source_id).expect("after");
        assert_eq!(before.host, after.host);
        assert_eq!(before.revision, after.revision);
        assert_eq!(after.codex_can_receive, ready);
        assert!(response.result["retry_after_ms"].as_u64().expect("retry") <= 15000);
        let count = connections.load(Ordering::SeqCst);
        let cached = call(
            &home,
            "local_session_recheck",
            json!({"source_id":source_id,"by":"user"}),
        )
        .await;
        assert_eq!(cached.result["check_status"], "cached");
        assert_eq!(connections.load(Ordering::SeqCst), count);
    }
    assert_eq!(
        local_sessions::scan_records(&home)
            .expect("records")
            .0
            .len(),
        5,
        "checks must not create sources"
    );
    let list = call(&home, "local_session_list", json!({"include_offline":true})).await;
    assert_eq!(
        list.result["sources"]
            .as_array()
            .expect("directory")
            .iter()
            .find(|s| s["source_id"] == "codex:ready")
            .expect("ready source")["connected"],
        true
    );
    assert_eq!(
        list.result["source_states"]["codex:missing"]["diagnostic"]["state"],
        "thread_not_loaded"
    );
    let slow_home = home.clone();
    let slow = tokio::spawn(async move {
        call(
            &slow_home,
            "local_session_recheck",
            json!({"source_id":"codex:slow","by":"user"}),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(1), entered.notified())
        .await
        .expect("probe entered");
    let count = connections.load(Ordering::SeqCst);
    let busy = call(
        &home,
        "local_session_recheck",
        json!({"source_id":"codex:busy","by":"user"}),
    )
    .await;
    assert_eq!(busy.result["check_status"], "busy");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        count,
        "concurrent recheck must not start another provider"
    );
    release.notify_one();
    assert!(slow.await.expect("check task").ok);
    server.abort();
}
