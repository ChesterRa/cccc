use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};

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
