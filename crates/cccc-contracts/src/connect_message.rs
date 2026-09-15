//! Logical delivery identity is independent of short-lived transport proofs.
use crate::{Event, connect::ConnectActor};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectGroupAddress {
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_id: String,
    pub group_id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectAttachment {
    pub sha256: String,
    pub bytes: u64,
    pub name: String,
    pub mime_type: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectReplyReference {
    pub delivery_id: String,
    pub event_id: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub delivery_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_origin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_id: String,
    pub source: ConnectGroupAddress,
    pub sender: ConnectActor,
    pub source_event_id: String,
    pub target: ConnectGroupAddress,
    pub recipients: Vec<ConnectActor>,
    pub text: String,
    pub format: String,
    pub insight: Option<String>,
    pub message_mode: String,
    pub attachments: Vec<ConnectAttachment>,
    pub reply_to: Option<ConnectReplyReference>,
    pub created_at: String,
    pub deliver_before: String,
    pub reply_before: String,
}

/// A cancellation is a control message, not a new chat request or runtime stop.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectCancellation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub delivery_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_origin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_id: String,
    pub source: ConnectGroupAddress,
    pub sender: ConnectActor,
    pub source_event_id: String,
    pub target: ConnectGroupAddress,
    pub original_delivery_id: String,
    pub original_message_sha256: String,
    pub original_source_event_id: String,
    pub original_deliver_before: String,
    pub created_at: String,
    pub deliver_before: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ConnectWork {
    Message(Box<ConnectMessage>),
    Cancel(Box<ConnectCancellation>),
}

impl ConnectWork {
    pub fn connection_id(&self) -> Option<&str> {
        match self {
            Self::Message(m) => m.connection_id.as_deref(),
            Self::Cancel(c) => c.connection_id.as_deref(),
        }
    }
    pub fn delivery_id(&self) -> &str {
        match self {
            Self::Message(message) => &message.delivery_id,
            Self::Cancel(cancel) => &cancel.delivery_id,
        }
    }
    pub fn account_origin(&self) -> &str {
        match self {
            Self::Message(message) => &message.account_origin,
            Self::Cancel(cancel) => &cancel.account_origin,
        }
    }
    pub fn account_id(&self) -> &str {
        match self {
            Self::Message(message) => &message.account_id,
            Self::Cancel(cancel) => &cancel.account_id,
        }
    }
    pub fn source(&self) -> &ConnectGroupAddress {
        match self {
            Self::Message(message) => &message.source,
            Self::Cancel(cancel) => &cancel.source,
        }
    }
    pub fn sender(&self) -> &ConnectActor {
        match self {
            Self::Message(message) => &message.sender,
            Self::Cancel(cancel) => &cancel.sender,
        }
    }
    pub fn source_event_id(&self) -> &str {
        match self {
            Self::Message(message) => &message.source_event_id,
            Self::Cancel(cancel) => &cancel.source_event_id,
        }
    }
    pub fn target(&self) -> &ConnectGroupAddress {
        match self {
            Self::Message(message) => &message.target,
            Self::Cancel(cancel) => &cancel.target,
        }
    }
    pub fn created_at(&self) -> &str {
        match self {
            Self::Message(message) => &message.created_at,
            Self::Cancel(cancel) => &cancel.created_at,
        }
    }
    pub fn deliver_before(&self) -> &str {
        match self {
            Self::Message(message) => &message.deliver_before,
            Self::Cancel(cancel) => &cancel.deliver_before,
        }
    }
    pub fn attachments(&self) -> &[ConnectAttachment] {
        match self {
            Self::Message(message) => &message.attachments,
            Self::Cancel(_) => &[],
        }
    }
}

/// Only active work lives in the outbox. Final status is appended to the source ledger.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectOutboxEntry {
    pub work: ConnectWork,
    pub source_event: Event,
    pub progress: ConnectDeliveryProgress,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectDeliveryProgress {
    pub source_projected: bool,
    pub attempts: u32,
    /// A POST may have reached the target. Messages query their original receipt;
    /// cancellation replays the same idempotent control. Expiry stays unconfirmed.
    pub needs_receipt: bool,
    pub next_attempt_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectDeliveryReceipt {
    pub delivery_id: String,
    pub message_sha256: String,
    pub event_id: String,
    pub delivered_at: String,
}

/// Attachment bytes are transient wire data, never duplicated in the durable outbox.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectBlobPayload {
    pub sha256: String,
    pub base64: String,
}
