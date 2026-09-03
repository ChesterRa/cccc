use serde_json::Value;
use std::io;
use tokio::sync::oneshot;

pub(super) enum PendingKind {
    Request {
        method: String,
        requested_session_id: Option<String>,
        response: oneshot::Sender<io::Result<Value>>,
    },
    Prompt {
        turn_id: String,
        delegation_id: String,
        expected_user_text: String,
        observed_user_text: String,
        buffered_notifications: Vec<Value>,
        buffered_bytes: usize,
        response: Option<oneshot::Sender<io::Result<String>>>,
    },
}
