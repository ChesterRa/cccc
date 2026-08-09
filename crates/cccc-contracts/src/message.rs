use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Reference {
    #[serde(default = "url_kind")]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub sha: String,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Attachment {
    #[serde(default = "file_kind")]
    pub kind: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ChatMessageData {
    pub text: String,
    #[serde(default = "plain_format")]
    pub format: String,
    #[serde(default)]
    pub insight: Option<String>,
    #[serde(default = "normal_priority")]
    pub priority: String,
    #[serde(default)]
    pub reply_required: bool,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub quote_text: Option<String>,
    #[serde(default)]
    pub refs: Vec<Map<String, Value>>,
    #[serde(default)]
    pub attachments: Vec<Map<String, Value>>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub suggested_user_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChatStreamData {
    pub stream_id: String,
    pub op: String,
    #[serde(default = "snapshot_mode")]
    pub mode: String,
    #[serde(default)]
    pub text: String,
    #[serde(default = "plain_format")]
    pub format: String,
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub sender_title: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
}

fn url_kind() -> String {
    "url".into()
}
fn file_kind() -> String {
    "file".into()
}
fn plain_format() -> String {
    "plain".into()
}
fn normal_priority() -> String {
    "normal".into()
}
fn snapshot_mode() -> String {
    "snapshot".into()
}

#[cfg(test)]
mod tests {
    use super::{ChatMessageData, ChatStreamData};

    #[test]
    fn chat_message_contract_carries_peer_insight() {
        let message: ChatMessageData = serde_json::from_value(serde_json::json!({
            "text":"work",
            "insight":"reconsider the dependency boundary"
        }))
        .expect("message contract");
        assert_eq!(
            message.insight.as_deref(),
            Some("reconsider the dependency boundary")
        );
    }

    #[test]
    fn chat_stream_contract_carries_sender_title_snapshot() {
        let stream: ChatStreamData = serde_json::from_value(serde_json::json!({
            "stream_id":"stream-1",
            "op":"update",
            "text":"partial",
            "sender_title":"Review Bot"
        }))
        .expect("stream contract");

        assert_eq!(stream.sender_title.as_deref(), Some("Review Bot"));
    }
}
