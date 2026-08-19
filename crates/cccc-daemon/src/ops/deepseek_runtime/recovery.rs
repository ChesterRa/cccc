use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};

/// Rebuild the DeepSeek cursor from durable terminal events after a daemon
/// restart. Only a contiguous unread prefix is eligible; no prompt or output
/// is replayed here.
pub fn recover(home: &HomeLayout, group: &GroupDoc, actor: &Actor, limit: usize) -> usize {
    let Ok(unread) = cccc_core::inbox::list_unread(home, group, &actor.id, limit.max(1), "all")
    else {
        return 0;
    };
    let mut recovered = 0;
    for event in unread {
        if !has_completed_event(home, group, actor, &event.id) {
            break;
        }
        match cccc_core::inbox::advance(home, &group.group_id, &actor.id, &event.id) {
            Ok(true) => recovered += 1,
            Ok(false) => continue,
            Err(_) => break,
        }
    }
    recovered
}

pub(super) fn has_completed_event(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event_id: &str,
) -> bool {
    let _ = actor;
    crate::ops::local_headless::contains_event_dedupe(
        home,
        &group.group_id,
        &format!("deepseek.turn:headless.turn.completed:{event_id}"),
    )
    .unwrap_or(false)
}
