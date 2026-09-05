use super::*;
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

type Fields = BTreeMap<String, String>;
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Fields>>>);
struct Visitor(Fields);
impl Visit for Visitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
}
impl Subscriber for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut visitor = Visitor(Fields::new());
        event.record(&mut visitor);
        if visitor.0.contains_key("lifecycle_event") {
            self.0.lock().expect("lock test state").push(visitor.0);
        }
    }
}

#[tokio::test]
async fn real_rpc_path_reports_success_error_and_timeout_without_payloads() {
    use super::super::ProtocolClient;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    // Only the provider is mocked; requests traverse the production websocket client.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let address = listener.local_addr().expect("read listener address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept test connection");
        let mut ws = tokio_tungstenite::accept_async(stream)
            .await
            .expect("complete accept async in fixture");
        while let Some(Ok(Message::Text(text))) = ws.next().await {
            let request: Value = serde_json::from_str(&text).expect("complete from str in fixture");
            let response = match request["method"]
                .as_str()
                .expect("complete as str in fixture")
            {
                "thread/read" => continue, // Exercise the actual request timeout.
                "thread/start" => {
                    json!({"id":request["id"],"error":{"code":-1,"message":"SECRET_MARKER"}})
                }
                _ => json!({"id":request["id"],"result":{"value":42}}),
            };
            ws.send(Message::Text(response.to_string().into()))
                .await
                .expect("send test event");
        }
    });
    let socket = tokio_tungstenite::connect_async(format!("ws://{address}"))
        .await
        .expect("complete connect async in fixture")
        .0;
    let client = ProtocolClient::new(socket, "probe".into());
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.clone());
    let timeout = Duration::from_secs(1);
    let ok = client
        .request("initialize", json!({"secret":"SECRET_MARKER"}), timeout)
        .await
        .expect("complete request in fixture");
    assert_eq!(ok["value"], 42);
    assert!(
        client
            .request("thread/start", json!({}), timeout)
            .await
            .expect_err("operation must fail in this scenario")
            .to_string()
            .contains("SECRET_MARKER")
    );
    assert_eq!(
        client
            .request("thread/read", json!({}), Duration::from_millis(5))
            .await
            .expect_err("operation must fail in this scenario")
            .kind(),
        io::ErrorKind::TimedOut
    );
    client
        .request("turn/start", json!({}), timeout)
        .await
        .expect("complete request in fixture");
    client.close().await;
    server.abort();

    let records = capture.0.lock().expect("lock test state");
    assert_eq!(
        records.len(),
        6,
        "ordinary turn traffic must not emit lifecycle diagnostics"
    );
    for (pair, (phase, success)) in records.chunks_exact(2).zip([
        ("codex.initialize", "true"),
        ("codex.thread_start", "false"),
        ("codex.thread_read", "false"),
    ]) {
        assert_eq!(pair[0]["phase"], phase);
        assert_eq!(pair[0]["lifecycle_event"], "started");
        assert_eq!(pair[1]["phase"], phase);
        assert_eq!(pair[1]["lifecycle_event"], "completed");
        assert_eq!(pair[1]["success"], success);
        assert!(pair[1]["elapsed_ms"].parse::<u64>().is_ok());
    }
    assert!(!format!("{records:?}").contains("SECRET_MARKER"));
}

#[test]
fn synchronous_phase_preserves_results_and_records_failure() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.clone());
    assert_eq!(
        run_sync("runtime.process_cleanup", || Ok(7)).expect("complete run sync in fixture"),
        7
    );
    let error = run_sync::<()>("codex.spawn", || {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    })
    .expect_err("operation must fail in this scenario");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    let records = capture.0.lock().expect("lock test state");
    assert_eq!(records.len(), 4);
    assert_eq!(records[1]["success"], "true");
    assert_eq!(records[3]["success"], "false");
}
