//! The top-level arguments each message-write operation accepts on its IPC
//! entry (CCCC Daemon IPC v1, "Message write arguments").
//!
//! A request with any other top-level key fails with [`UNKNOWN_ARGUMENTS`]
//! before it has an effect, so a misspelt or misplaced field is reported
//! instead of being stored or silently ignored. Only the incoming request is
//! checked; stored events are never compared with these lists.
//!
//! Each list holds the operation's public arguments, the metadata CCCC's own
//! ports attach, and the fields the operation refuses with an error of its
//! own, so that error still reports them. Key validation is separate from the
//! existing authorization checks: accepting a key establishes no provenance or
//! authority. Nested values (`refs`, `attachments`, `question`, `checklist`,
//! `context`) follow their own rules.

use crate::chet::GROUP_AUTHOR_GENERATION_FIELD;

pub const UNKNOWN_ARGUMENTS: &str = "unknown_arguments";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageWrite {
    /// `send` and `message_send`.
    Send,
    Reply,
    SendFiles,
    SendCrossGroup,
    /// `connect_send` and `connect_send_files`.
    ConnectSend,
    TrackedSend,
    SystemNotify,
    SlashSkillDispatch,
    LocalSessionSend,
}

impl MessageWrite {
    /// The operation a `message_upload_preflight` request names.
    pub fn for_upload(operation: &str) -> Option<Self> {
        match operation {
            "send" => Some(Self::Send),
            "reply" => Some(Self::Reply),
            "send_cross_group" => Some(Self::SendCrossGroup),
            _ => None,
        }
    }

    fn lists(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Send => &[CALLER, MESSAGE, CHAT_CHECKED, SEND, IM],
            Self::Reply => &[CALLER, MESSAGE, CHAT_CHECKED, REPLY, IM],
            Self::SendFiles => &[CALLER, MESSAGE, CHAT_CHECKED, SEND_FILES],
            Self::SendCrossGroup => &[CALLER, MESSAGE, CHAT_CHECKED, SEND_CROSS_GROUP],
            Self::ConnectSend => &[CALLER, CONNECT_SEND, CHAT_CHECKED],
            Self::TrackedSend => &[CALLER, TRACKED_SEND],
            Self::SystemNotify => &[CALLER, SYSTEM_NOTIFY],
            Self::SlashSkillDispatch => &[CALLER, SLASH_SKILL_DISPATCH],
            Self::LocalSessionSend => &[LOCAL_SESSION_SEND],
        }
    }

    fn accepts(self, key: &str) -> bool {
        self.lists().iter().any(|list| list.contains(&key))
    }
}

/// The keys `operation` does not accept, sorted.
pub fn unknown<'a>(
    operation: MessageWrite,
    keys: impl IntoIterator<Item = &'a String>,
) -> Vec<&'a str> {
    sorted(keys.into_iter().filter(|key| !operation.accepts(key)))
}

/// The keys a `message_upload_preflight` for `operation` does not accept,
/// sorted: the operation's own arguments plus the preflight's.
pub fn unknown_for_upload<'a>(
    operation: MessageWrite,
    keys: impl IntoIterator<Item = &'a String>,
) -> Vec<&'a str> {
    sorted(
        keys.into_iter()
            .filter(|key| !operation.accepts(key) && !UPLOAD.contains(&key.as_str())),
    )
}

fn sorted<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<&'a str> {
    let mut keys: Vec<&str> = keys.map(String::as_str).collect();
    keys.sort_unstable();
    keys
}

/// The request scope and the caller authority CCCC's ports attach.
const CALLER: &[&str] = &[
    "group_id",
    "by",
    // MCP tools name the calling Actor.
    "actor_id",
    // Session authority captured by the MCP router or the Web Model host.
    "_cccc_native_binding",
    "_cccc_web_binding",
    // The Chet voice origin, attached by the CCCC client.
    "_voice_origin",
    // Chet admission, attached by the Web.
    "_cccc_chet_web_admin",
];

/// The content of a new chat message (CCCS §6.1) and its delivery options.
const MESSAGE: &[&str] = &[
    "text",
    "format",
    "to",
    "message_mode",
    "attachments",
    "refs",
    "insight",
    "require_peer_insight",
    "client_id",
    "suggested_user_message",
    // Only `client_id` selects a replay; this is not a daemon alias.
    "idempotency_key",
];

/// Fields each chat operation checks itself: a question, which only `send`
/// accepts, and fields every chat operation refuses with its own error.
const CHAT_CHECKED: &[&str] = &[
    "question",
    "answer",
    "priority",
    "reply_required",
    "requires_ack",
    GROUP_AUTHOR_GENERATION_FIELD,
];

const SEND: &[&str] = &[
    "path",
    "reply_to",
    "quote_text",
    // Relay provenance.
    "src_group_id",
    "src_event_id",
    // Send-record metadata.
    "dst_group_id",
    "dst_to",
    "dst_message_mode",
];

const REPLY: &[&str] = &["reply_to", "quote_text"];

/// Inbound IM provenance, attached by the Web's IM bridge and no-MCP send. A
/// reply drops it, so the reply still reaches IM subscribers.
pub const IM: &[&str] = &[
    "transport",
    "im_platform",
    "im_chat_id",
    "im_thread_id",
    "source_platform",
    "source_user_id",
    "source_message_id",
];

const SEND_FILES: &[&str] = &["paths", "path", "dst_group_id"];

const SEND_CROSS_GROUP: &[&str] = &["dst_group_id", "path", "reply_to", "quote_text"];

/// Qualified Connect messages; local file/resource and content restrictions
/// remain in the operation that owns them.
const CONNECT_SEND: &[&str] = &[
    "instance_id",
    "target_group_id",
    "client_id",
    "text",
    "format",
    "to",
    "message_mode",
    "insight",
    "require_peer_insight",
    "attachments",
    "paths",
    "refs",
    "suggested_user_message",
];

const TRACKED_SEND: &[&str] = &[
    // The message, always sent with `message_mode="send"`.
    "text",
    "format",
    "to",
    "path",
    "attachments",
    "refs",
    "insight",
    "require_peer_insight",
    "suggested_user_message",
    // The task.
    "title",
    "outcome",
    "goal",
    "status",
    "task_priority",
    "task_type",
    "waiting_on",
    "handoff_to",
    "blocked_by",
    "checklist",
    "assignee",
    "notes",
    "idempotency_key",
    "client_request_id",
    // Refused with their own error.
    "priority",
    "message_priority",
    "reply_required",
    GROUP_AUTHOR_GENERATION_FIELD,
];

const SYSTEM_NOTIFY: &[&str] = &[
    "kind",
    "priority",
    "title",
    "message",
    // Further body text: delivery shows title, message and text in order.
    "text",
    "target_actor_id",
    "to",
    "im_visibility",
    "client_id",
    "context",
    "related_event_id",
    // Refused with their own error.
    "requires_ack",
    "question",
    "answer",
    GROUP_AUTHOR_GENERATION_FIELD,
];

const SLASH_SKILL_DISPATCH: &[&str] = &[
    "task_text",
    "command",
    "capability_id",
    "to",
    "client_id",
    "reply_to",
    "quote_text",
    // Dropped: the dispatch writes its own control reference.
    "control_kind",
    "title",
    "hidden",
    // Refused with their own error: a dispatch is always sent.
    "priority",
    "reply_required",
    "requires_ack",
    "message_mode",
    GROUP_AUTHOR_GENERATION_FIELD,
];

const LOCAL_SESSION_SEND: &[&str] = &[
    "group_id",
    "by",
    "source_id",
    "source_ids",
    "to",
    "text",
    // The conversation's title, when the send creates one.
    "title",
    "client_id",
    "insight",
    "message_mode",
    "require_peer_insight",
    // Captured session authority.
    "_cccc_native_binding",
    "_cccc_native_source_binding",
    // Refused with its own error.
    GROUP_AUTHOR_GENERATION_FIELD,
];

const UPLOAD: &[&str] = &["operation", "has_attachments"];

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn reports_only_the_keys_an_operation_does_not_accept_in_order() {
        let request = keys(&["text", "options", "mdoe", "to", "recipients"]);
        assert_eq!(
            unknown(MessageWrite::Send, &request),
            ["mdoe", "options", "recipients"]
        );
        assert!(unknown(MessageWrite::Send, &keys(&["question", "path"])).is_empty());
    }

    #[test]
    fn each_operation_keeps_its_own_list() {
        let question = keys(&["question"]);
        // A question is checked by the chat operations themselves.
        assert!(unknown(MessageWrite::Reply, &question).is_empty());
        assert_eq!(unknown(MessageWrite::TrackedSend, &question), ["question"]);
        // A tracked send always sends; its mode is not an argument.
        assert_eq!(
            unknown(MessageWrite::TrackedSend, &keys(&["message_mode"])),
            ["message_mode"]
        );
        assert_eq!(
            unknown(MessageWrite::SystemNotify, &keys(&["text", "format"])),
            ["format"]
        );
        assert_eq!(
            unknown(MessageWrite::Send, &keys(&["scope_key"])),
            ["scope_key"]
        );
        assert!(
            unknown(
                MessageWrite::ConnectSend,
                &keys(&["instance_id", "target_group_id", "paths", "client_id"])
            )
            .is_empty()
        );
        assert_eq!(
            unknown(
                MessageWrite::ConnectSend,
                &keys(&["dst_group_id", "idempotency_key", "connect_reply_to"])
            ),
            ["connect_reply_to", "dst_group_id", "idempotency_key"]
        );
    }

    #[test]
    fn an_upload_preflight_accepts_its_own_fields_beside_the_operations() {
        let request = keys(&["operation", "has_attachments", "reply_to", "paths"]);
        let reply = MessageWrite::for_upload("reply").expect("reply");
        assert_eq!(unknown_for_upload(reply, &request), ["paths"]);
        assert_eq!(
            unknown(reply, &request),
            ["has_attachments", "operation", "paths"]
        );
        assert_eq!(MessageWrite::for_upload("send_files"), None);
    }
}
