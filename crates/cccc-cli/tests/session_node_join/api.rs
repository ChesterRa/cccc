//! The approval UI is the controlled boundary. Every request comes from the real CLI.
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::time::Duration;
use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
    process::Child,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

pub struct Call {
    uri: Uri,
    method: Method,
    authorization: bool,
    pub body: Value,
    reply: oneshot::Sender<Response>,
}
impl Call {
    pub fn answer(self, status: StatusCode, value: Value) {
        assert!(
            self.reply
                .send((status, Json(value)).into_response())
                .is_ok()
        );
    }

    pub fn redirect(self, to: &str) {
        assert!(
            self.reply
                .send((StatusCode::TEMPORARY_REDIRECT, [("location", to)]).into_response())
                .is_ok()
        );
    }
}

pub struct Api {
    pub origin: String,
    pub calls: mpsc::UnboundedReceiver<Call>,
    received: usize,
    serve: JoinHandle<()>,
}
impl Drop for Api {
    fn drop(&mut self) {
        self.serve.abort();
    }
}

impl Api {
    pub async fn start() -> Self {
        async fn request(
            State(tx): State<mpsc::UnboundedSender<Call>>,
            method: Method,
            uri: Uri,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Response {
            let (reply, answer) = oneshot::channel();
            if tx
                .send(Call {
                    uri,
                    method,
                    authorization: headers.contains_key("authorization"),
                    body,
                    reply,
                })
                .is_err()
            {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            answer
                .await
                .unwrap_or_else(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback approval API");
        let origin = format!(
            "http://{}",
            listener.local_addr().expect("listening address")
        );
        let (tx, calls) = mpsc::unbounded_channel();
        let app = Router::new().fallback(request).with_state(tx);
        let serve = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve approval fixture");
        });
        Self {
            origin,
            calls,
            received: 0,
            serve,
        }
    }

    pub async fn next(&mut self, child: &mut Child, suffix: &str) -> Call {
        let received = tokio::select! {
            biased;
            call = self.calls.recv() => Ok(call.expect("approval fixture closed")),
            exit = child.wait() => Err(match exit {
                Ok(status) => format!("exited with {status}"),
                Err(error) => format!("wait failed ({:?})", error.kind()),
            }),
            () = tokio::time::sleep(Duration::from_secs(12)) => {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
                Err(String::from("was still running at the request deadline"))
            }
        };
        let call = match received {
            Ok(call) => call,
            Err(state) => {
                let phase = last_safe_phase(child).await;
                panic!(
                    "expected POST {suffix} (request {}): CLI {state}; {phase}",
                    self.received + 1
                );
            }
        };
        self.received += 1;
        assert_eq!(call.method, Method::POST);
        assert_eq!(
            call.uri.path(),
            format!("/api/v1/session_nodes/join/{suffix}")
        );
        assert!(
            call.uri.query().is_none(),
            "authority belongs only in a POST body"
        );
        assert!(
            !call.authorization,
            "public join must not send local administrator authority"
        );
        call
    }
}

/// Report only fixed stage labels and a numeric OS error. A CLI error, confirmation link,
/// request body, ticket, or captured stderr may contain data that must never reach CI logs.
async fn last_safe_phase(child: &mut Child) -> String {
    let mut bytes = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            stderr.take(8192).read_to_end(&mut bytes),
        )
        .await;
    }
    let text = String::from_utf8_lossy(&bytes);
    let stage = [
        ("Device join cancelled.", "cancelled"),
        ("user's home directory", "user home unavailable"),
        ("MissingUserHome", "user home unavailable"),
        (
            "Cannot initialize the device join HTTP client",
            "HTTP client unavailable",
        ),
        (
            "Cannot generate a private device join code",
            "OS randomness unavailable",
        ),
        ("already runs for this Home", "another connector is running"),
        ("changed during pairing", "reconnect state changed"),
        ("temporarily unavailable", "HTTP request retry"),
        ("Waiting for a hub administrator", "waiting for approval"),
        ("Requesting device approval", "creating HTTP request"),
        ("Error:", "CLI startup error"),
    ]
    .iter()
    .find(|(pattern, _)| text.contains(pattern))
    .map(|(_, stage)| *stage)
    .unwrap_or("no recognized CLI progress");
    let code = text
        .split("(os error ")
        .nth(1)
        .and_then(|code| code.split(')').next())
        .and_then(|code| code.parse::<i32>().ok());
    format!("stage={stage}, os_error={code:?}")
}
