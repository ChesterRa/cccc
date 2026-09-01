use cccc_contracts::Event;

pub(super) const DELIVERY_REPLY_GUIDANCE: &str = "[cccc] To reply, use cccc_message_reply with that message's event_id; use cccc_message_send only for a new message.";

pub(super) fn append(events: &[Event], mut rendered: String) -> String {
    if events.iter().any(|event| event.kind == "chat.message") {
        rendered.push_str("\n\n");
        rendered.push_str(DELIVERY_REPLY_GUIDANCE);
    }
    rendered
}
