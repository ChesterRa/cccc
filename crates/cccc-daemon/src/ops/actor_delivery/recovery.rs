use cccc_contracts::Event;
use serde_json::json;

pub(super) fn notice_event(
    group_id: &str,
    actor_id: &str,
    unread_count: usize,
    count_is_lower_bound: bool,
    latest_event_id: &str,
) -> Event {
    let count = if count_is_lower_bound {
        format!("at least {unread_count}")
    } else {
        unread_count.to_string()
    };
    let mut event = Event::new("system.notify", group_id);
    event.id = format!("unread-recovery:{latest_event_id}");
    event.by = "system".into();
    event.data = json!({
        "kind":"info",
        "priority":"normal",
        "title":"Unread collaboration messages",
        "message":format!(
            "You have {count} unread collaboration messages. Use cccc_inbox_list to review them, then cccc_inbox_mark_read after handling them. This restart recovery notice does not advance the unread cursor."
        ),
        "target_actor_id":actor_id,
        "im_visibility":"internal",
        "context":{
            "kind":"unread_recovery",
            "unread_count":unread_count,
            "count_is_lower_bound":count_is_lower_bound,
        },
        "requires_ack":false,
        "related_event_id":latest_event_id,
    })
    .as_object()
    .cloned()
    .expect("recovery notice data");
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_one_transient_notice_without_copying_unread_bodies() {
        let event = notice_event("g1", "peer1", 24, false, "event-last");

        assert_eq!(event.id, "unread-recovery:event-last");
        assert_eq!(event.kind, "system.notify");
        assert_eq!(event.data["target_actor_id"], "peer1");
        assert_eq!(event.data["context"]["unread_count"], 24);
        assert!(event.data["message"].as_str().is_some_and(|message| {
            message.contains("24 unread collaboration messages")
                && message.contains("does not advance the unread cursor")
        }));
    }
}
