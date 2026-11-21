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
    net::TcpListener,
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
            serve,
        }
    }

    pub async fn next(&mut self, suffix: &str) -> Call {
        let call = tokio::time::timeout(Duration::from_secs(12), self.calls.recv())
            .await
            .expect("request deadline")
            .expect("request");
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
