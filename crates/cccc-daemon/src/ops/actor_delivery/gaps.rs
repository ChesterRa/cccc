//! Revisit a send that was recorded before its transport or first claim could be established.
//! The ledger holds the messages. Keep only the affected range for each member.
use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Gap {
    created_at: String,
    // None means an explicit lifecycle catch-up covered the full pending history.
    range: Option<(String, String)>,
    paused: bool,
    revision: u64,
}

fn gaps() -> &'static Mutex<HashMap<Key, Gap>> {
    static GAPS: OnceLock<Mutex<HashMap<Key, Gap>>> = OnceLock::new();
    GAPS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// New sends arrive under the group write permit in ledger order. A subsequent
/// send joins the range before it can overtake the first missing claim.
pub(super) fn extend(group: &GroupDoc, actor: &Actor, event: &Event) -> bool {
    let Ok(mut pending) = gaps().lock() else {
        return false;
    };
    if pending.is_empty() {
        return false;
    }
    let key = (group.group_id.clone(), actor.id.clone());
    let Some(gap) = pending.get_mut(&key) else {
        return false;
    };
    if gap.created_at != actor.created_at {
        return false;
    }
    if let Some((_, last)) = &mut gap.range {
        last.clone_from(&event.id);
    }
    gap.paused = false;
    gap.revision = gap.revision.wrapping_add(1);
    true
}

pub(super) fn record(group: &GroupDoc, actor: &Actor, event: Option<&Event>) {
    if !actor.external_session_id.is_empty() {
        return;
    }
    if let Some(event) = event
        && extend(group, actor, event)
    {
        return;
    }
    if let Ok(mut pending) = gaps().lock() {
        let key = (group.group_id.clone(), actor.id.clone());
        let revision = pending
            .get(&key)
            .map_or(0, |gap| gap.revision.wrapping_add(1));
        pending.insert(
            key,
            Gap {
                created_at: actor.created_at.clone(),
                range: event.map(|e| (e.id.clone(), e.id.clone())),
                paused: false,
                revision,
            },
        );
    }
}

pub(in crate::ops) fn set_group_paused(group_id: &str, paused: bool) {
    if let Ok(mut pending) = gaps().lock() {
        for ((group, _), gap) in pending.iter_mut() {
            if group == group_id {
                gap.paused = paused;
            }
        }
    }
}

pub(super) fn resume_member(group_id: &str, actor_id: &str) -> Option<Gap> {
    let mut pending = gaps().lock().ok()?;
    if pending.is_empty() {
        return None;
    }
    let gap = pending.get_mut(&(group_id.to_owned(), actor_id.to_owned()))?;
    gap.paused = false;
    Some(gap.clone())
}

/// A new send or a fresh failure may have extended the gap while the page was processed.
pub(super) fn complete(group_id: &str, actor_id: &str, snapshot: Option<&Gap>) {
    if let Some(snapshot) = snapshot
        && let Ok(mut pending) = gaps().lock()
    {
        let key = (group_id.to_owned(), actor_id.to_owned());
        if pending.get(&key) == Some(snapshot) {
            pending.remove(&key);
        }
    }
}

pub(super) fn clear(remove: impl Fn(&Key) -> bool) {
    if let Ok(mut gaps) = gaps().lock() {
        gaps.retain(|key, _| !remove(key));
    }
}

pub(super) fn collect_groups(groups: &mut HashSet<String>) {
    if let Ok(gaps) = gaps().lock() {
        groups.extend(
            gaps.iter()
                .filter(|(_, gap)| !gap.paused)
                .map(|((group, _), _)| group.clone()),
        );
    }
}

pub(super) fn recover_group(home: &HomeLayout, group_id: &str) {
    let pending = gaps()
        .lock()
        .map(|gaps| {
            gaps.iter()
                .filter(|((group, _), gap)| group == group_id && !gap.paused)
                .map(|(key, gap)| (key.clone(), gap.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    let group =
        match cccc_core::GroupStore::new(home.clone()).and_then(|store| store.load(group_id)) {
            Ok(group) => group,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                clear(|(group, _)| group == group_id);
                return;
            }
            Err(_) => return,
        };
    for (key, gap) in pending {
        if recover_member(home, &group, &key.1, &gap) {
            complete(&key.0, &key.1, Some(&gap));
        }
    }
}

/// True when the range is settled or the member's normal lifecycle now owns it.
fn recover_member(home: &HomeLayout, group: &GroupDoc, actor_id: &str, gap: &Gap) -> bool {
    let Some(actor) = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id && actor.created_at == gap.created_at)
    else {
        return true;
    };
    if !actor.enabled
        || !actor.external_session_id.is_empty()
        || matches!(group.state, GroupState::Paused | GroupState::Stopped)
        || held_for_switch(group, actor)
    {
        return true;
    }

    // Observe once before reading or claiming a page, even if many sends missed the owner.
    let Ok(owner) = crate::ops::actor_runtime::delivery_owner(&group.group_id, actor_id) else {
        return false;
    };
    if !configured_for_push(actor)
        && !owner
            .as_ref()
            .is_some_and(|owner| owner.state == crate::ops::actor_runtime::OwnerState::Running)
    {
        return true;
    }
    match crate::ops::actor_runtime::confirmation_pause_applies(home, &group.group_id, actor) {
        Ok(false) => {}
        Ok(true) => return true,
        Err(_) => return false,
    }
    let occupied = in_flight()
        .lock()
        .map(|pending| {
            pending
                .iter()
                .filter(|(group_id, id, _)| group_id == &group.group_id && id == actor_id)
                .count()
        })
        .unwrap_or(QUEUE_CAPACITY);
    let slots = QUEUE_CAPACITY.saturating_sub(occupied);
    if slots == 0 {
        return false;
    }
    let range = gap
        .range
        .as_ref()
        .map(|(first, last)| (first.as_str(), last.as_str()));
    let events = match inbox::unclaimed_deliveries(home, group, actor_id, range, slots) {
        Ok(events) => events,
        Err(error) => {
            tracing::warn!(%error, %actor_id, group_id = %group.group_id, "could not read unclaimed deliveries");
            return false;
        }
    };
    let complete = events.len() < slots;
    let transport = owner_transport(home, group, actor, owner.as_ref());
    for event in events {
        match crate::ops::runtime_delivery::claim_first_attempt(
            home, group, actor, &event.id, transport,
        ) {
            Ok(true) => {
                offer_delivery(home, group, actor, &event, transport, true);
            }
            Ok(false) => {}
            Err(_) => return false,
        }
    }
    complete
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::runtime_delivery::{DeliveryOutcome, append_state, claim_first_attempt};
    use cccc_core::{GroupStore, ledger};
    use serde_json::json;

    #[test]
    fn a_gap_skips_existing_receipts_before_paging_and_rechecks_at_claim() {
        let temp = tempfile::tempdir().expect("isolated directory");
        let home = HomeLayout::from_path(temp.path()).expect("isolated home");
        let store = GroupStore::new(home.clone()).expect("group store");
        let mut group = store
            .create("missing first claim", "")
            .expect("create group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save membership");
        let path = store.ledger_path(&group.group_id).expect("group ledger");
        let source = || {
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data = json!({"to":[actor.id],"text":"test","message_mode":"send"})
                .as_object()
                .expect("message data")
                .clone();
            ledger::append(&path, &event).expect("append source message");
            event.id
        };
        let receipt = |source: &str, outcome| {
            append_state(
                &home,
                &group.group_id,
                &actor.id,
                &actor.created_at,
                source,
                "pty",
                outcome,
            )
            .expect("append delivery receipt");
        };
        source(); // Never dispatched, but outside this gap.
        let first = source();
        receipt(&first, DeliveryOutcome::Claimed);
        for outcome in [
            DeliveryOutcome::Accepted,
            DeliveryOutcome::Ambiguous("unknown"),
            DeliveryOutcome::Unsubmitted("not written"),
        ] {
            receipt(&source(), outcome);
        }
        let last = source();
        source(); // A later send has its own outcome.
        let page = || {
            inbox::unclaimed_deliveries(&home, &group, &actor.id, Some((&first, &last)), 1)
                .expect("read unclaimed page")
        };
        let events = page();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, last);
        // The original delivery path resolved the source since the maintenance read.
        receipt(&last, DeliveryOutcome::Unsubmitted("not written"));
        assert!(!claim_first_attempt(&home, &group, &actor, &last, "pty").expect("recheck claim"));
        assert!(page().is_empty());
    }

    #[test]
    fn a_routing_change_does_not_remove_other_sources_in_the_range() {
        let temp = tempfile::tempdir().expect("isolated directory");
        let home = HomeLayout::from_path(temp.path()).expect("isolated home");
        let store = GroupStore::new(home.clone()).expect("group store");
        let mut group = store.create("new foreman", "").expect("create group");
        group.actors = vec![Actor::new("lead"), Actor::new("peer")];
        store.save(&group).expect("save membership");
        let path = store.ledger_path(&group.group_id).expect("group ledger");
        let send = |to: &str| {
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data = json!({"to":[to], "message_mode":"send", "text":"hello"})
                .as_object()
                .expect("message data")
                .clone();
            ledger::append(&path, &event).expect("append source message");
            event.id
        };
        let first = send("@peers");
        let direct = send("peer");
        group.actors.remove(0);
        store.save(&group).expect("save membership");
        let events = inbox::unclaimed_deliveries(&home, &group, "peer", Some((&first, &direct)), 2)
            .expect("read with new roles");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, direct);
    }

    #[test]
    fn a_new_member_with_the_same_name_does_not_inherit_the_gap() {
        let temp = tempfile::tempdir().expect("isolated directory");
        let home = HomeLayout::from_path(temp.path()).expect("isolated home");
        let store = GroupStore::new(home.clone()).expect("group store");
        let mut group = store.create("replacement", "").expect("create group");
        let mut actor = Actor::new("peer1");
        let source = Event::new("chat.message", &group.group_id);
        record(&group, &actor, Some(&source));
        actor.created_at = "a different member".into();
        group.actors.push(actor);
        store.save(&group).expect("save membership");

        recover_group(&home, &group.group_id);
        let mut groups = HashSet::new();
        collect_groups(&mut groups);
        assert!(!groups.contains(&group.group_id));
    }
}
