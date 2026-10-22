//! Whether Mail still awaits its recipient. The unread tick uses this to decide
//! a Mail notice is due, and delivery re-checks it right before sending one, so
//! a notice whose Mail was read, answered, or delivered meanwhile is dropped.

use std::collections::HashSet;
use std::io;

use crate::{GroupDoc, HomeLayout, inbox};

/// A Mail awaits its recipient until it is read, replied to, or handed to the
/// recipient's runtime (`accepted`, or `ambiguous` once it may have been).
pub(crate) fn awaiting(read: bool, replied: bool, delivery_state: Option<&str>) -> bool {
    !read
        && !replied
        && !delivery_state.is_some_and(|state| matches!(state, "accepted" | "ambiguous"))
}

/// The ids among `mail_ids` that still await `actor_id`. Mail from before the
/// actor's current generation, or no longer in the ledger, does not.
pub fn still_awaited(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    mail_ids: &[String],
) -> io::Result<HashSet<String>> {
    if mail_ids.is_empty() {
        return Ok(HashSet::new());
    }
    inbox::projection::still_awaited(home, group, actor_id, mail_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GroupStore, ledger};
    use cccc_contracts::Event;
    use cccc_contracts::{Actor, GroupState};
    use serde_json::json;

    fn message(group_id: &str, by: &str, data: serde_json::Value) -> Event {
        let mut event = Event::new("chat.message", group_id);
        event.by = by.into();
        event.data = data.as_object().cloned().expect("data");
        event
    }

    #[test]
    fn mail_stops_awaiting_once_read_answered_or_delivered() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let created = store.create("pending mail", "").expect("group");
        store
            .mutate(&created.group_id, |group| {
                group.state = GroupState::Active;
                crate::actors::add(group, Actor::new("peer"))
            })
            .expect("actor");
        let group = store.load(&created.group_id).expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let mail = |text: &str| {
            message(
                &group.group_id,
                "user",
                json!({"to":["peer"],"text":text,"message_mode":"mail"}),
            )
        };
        let (unread, answered, delivered) = (mail("a"), mail("b"), mail("c"));
        let send = message(
            &group.group_id,
            "user",
            json!({"to":["peer"],"text":"d","message_mode":"send"}),
        );
        for event in [&unread, &answered, &delivered, &send] {
            ledger::append(&ledger_path, event).expect("append");
        }
        let reply = message(
            &group.group_id,
            "peer",
            json!({"text":"done","reply_to":answered.id}),
        );
        ledger::append(&ledger_path, &reply).expect("reply");
        let mut handed = Event::new("runtime.delivery", &group.group_id);
        handed.data = json!({"actor_id":"peer","source_event_id":delivered.id,"state":"accepted"})
            .as_object()
            .cloned()
            .expect("delivery");
        ledger::append(&ledger_path, &handed).expect("delivery");
        let ids = [&unread, &answered, &delivered, &send]
            .map(|event| event.id.clone())
            .to_vec();

        let awaited = still_awaited(&home, &group, "peer", &ids).expect("check");
        assert_eq!(
            awaited,
            HashSet::from([unread.id.clone()]),
            "only unread Mail awaits"
        );

        inbox::consume_unread(&home, &group, "peer", "peer", 50).expect("read");
        assert!(
            still_awaited(&home, &group, "peer", &ids)
                .expect("check")
                .is_empty()
        );
    }
}
