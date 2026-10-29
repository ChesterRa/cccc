//! Message identity shared by delivery evidence and pairing receipt checks.
pub(super) const MESSAGES_SCRIPT: &str = include_str!("chatgpt_messages.js");

pub(super) fn with_messages(script: &str, payload: &str) -> String {
    format!("({script})({payload}, ({MESSAGES_SCRIPT})())")
}
