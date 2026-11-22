//! Revisit a send that was recorded before its transport or first claim could be established.
//! The ledger holds the messages. Keep only the affected range for each member.
use super::*;
use crate::ops::runtime_delivery::{PreparedGapClaim, apply_gap_claim, prepare_gap_claim};
use crate::session_owner::OwnerObservations;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Gap {
    created_at: String,
    // None means an explicit lifecycle catch-up covered the full pending history.
    range: Option<(String, String)>,
    paused: bool,
    origin: u64,
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
        use std::sync::atomic::AtomicU64;
        static NEXT_ORIGIN: AtomicU64 = AtomicU64::new(0);
        let key = (group.group_id.clone(), actor.id.clone());
        pending.insert(
            key,
            Gap {
                created_at: actor.created_at.clone(),
                range: event.map(|e| (e.id.clone(), e.id.clone())),
                paused: false,
                origin: NEXT_ORIGIN.fetch_add(1, Ordering::Relaxed),
                revision: 0,
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

pub(super) struct PreparedGap {
    actor_id: String,
    gap: Gap,
    action: GapAction,
}

enum GapAction {
    Complete,
    Offer {
        transport: &'static str,
        sources: Vec<(Event, PreparedGapClaim)>,
        last_page: bool,
    },
}

pub(super) fn prepare_group(
    home: &HomeLayout,
    group: &GroupDoc,
    observed: &OwnerObservations,
) -> Vec<PreparedGap> {
    let pending = gaps()
        .lock()
        .map(|gaps| {
            gaps.iter()
                .filter(|((group_id, _), gap)| group_id == &group.group_id && !gap.paused)
                .map(|((_, actor_id), gap)| (actor_id.clone(), gap.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    pending
        .into_iter()
        .filter_map(|(actor_id, gap)| {
            let action = prepare_member(home, group, &actor_id, &gap, observed)?;
            Some(PreparedGap {
                actor_id,
                gap,
                action,
            })
        })
        .collect()
}

fn prepare_member(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    gap: &Gap,
    observed: &OwnerObservations,
) -> Option<GapAction> {
    let Some(actor) = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id && actor.created_at == gap.created_at)
    else {
        return Some(GapAction::Complete);
    };
    if !actor.enabled
        || !actor.external_session_id.is_empty()
        || matches!(group.state, GroupState::Paused | GroupState::Stopped)
        || held_for_switch(group, actor)
    {
        return Some(GapAction::Complete);
    }

    let transport = observed
        .with_scope(|| {
            use crate::dispatch::OpError;
            use crate::ops::actor_runtime::{
                OwnerState, confirmation_pause_applies, delivery_owner,
            };
            let owner = delivery_owner(&group.group_id, actor_id).map_err(OpError::io)?;
            if (!configured_for_push(actor)
                && !owner
                    .as_ref()
                    .is_some_and(|owner| owner.state == OwnerState::Running))
                || confirmation_pause_applies(home, &group.group_id, actor)?
            {
                return Ok::<_, OpError>(None);
            }
            Ok(Some(owner_transport(home, group, actor, owner.as_ref())))
        })
        .ok()?;
    let Some(transport) = transport else {
        return Some(GapAction::Complete);
    };
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
        return None;
    }
    let range = gap
        .range
        .as_ref()
        .map(|(first, last)| (first.as_str(), last.as_str()));
    let events = match inbox::unclaimed_or_unsubmitted_deliveries(
        home, group, actor_id, range, slots,
    ) {
        Ok(events) => events,
        Err(error) => {
            tracing::warn!(%error, %actor_id, group_id = %group.group_id, "could not read unclaimed or unsubmitted deliveries");
            return None;
        }
    };
    let mut last_page = events.len() < slots;
    let mut sources = Vec::new();
    for event in events {
        match prepare_gap_claim(home, group, actor, &event.id) {
            Ok(Some(claim)) => sources.push((event, claim)),
            Ok(None) => {}
            Err(_) => {
                last_page = false;
                break;
            }
        }
    }
    Some(GapAction::Offer {
        transport,
        sources,
        last_page,
    })
}

pub(super) fn apply_group(home: &HomeLayout, group: &GroupDoc, prepared: Vec<PreparedGap>) {
    for PreparedGap {
        actor_id,
        gap,
        action,
    } in prepared
    {
        let key = (group.group_id.clone(), actor_id.clone());
        if let GapAction::Offer {
            transport,
            sources,
            last_page,
        } = action
        {
            // A concurrent append only extends this range. The prepared
            // prefix keeps its order, and the exact comparison in complete()
            // below leaves the newly appended tail pending.
            if !gaps().lock().is_ok_and(|pending| {
                pending
                    .get(&key)
                    .is_some_and(|current| !current.paused && current.origin == gap.origin)
            }) {
                continue;
            }
            let Some(actor) = group.actors.iter().find(|actor| {
                actor.id == actor_id && actor.created_at == gap.created_at && actor.enabled
            }) else {
                continue;
            };
            if held_for_switch(group, actor) {
                continue;
            }
            let settled = sources.into_iter().all(|(event, claim)| {
                match apply_gap_claim(home, group, actor, &event.id, transport, claim) {
                    Ok(true) => matches!(
                        offer_delivery(home, group, actor, &event, transport, true),
                        Offer::Queued | Offer::AlreadyInFlight
                    ),
                    Ok(false) => true,
                    Err(_) => false,
                }
            });
            if !settled || !last_page {
                continue;
            }
        }
        complete(&group.group_id, &actor_id, Some(&gap));
    }
}

#[cfg(test)]
fn recover_group(home: &HomeLayout, group_id: &str) {
    match cccc_core::GroupStore::new(home.clone()).and_then(|store| store.load(group_id)) {
        Ok(group) => apply_group(
            home,
            &group,
            prepare_group(home, &group, &OwnerObservations::default()),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            clear(|(group, _)| group == group_id)
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::runtime_delivery::{
        DeliveryOutcome, append_state, claim_unclaimed_or_unsubmitted,
    };
    use cccc_core::{GroupStore, ledger};
    use serde_json::json;

    struct GroupGuard(String);
    impl Drop for GroupGuard {
        fn drop(&mut self) {
            shutdown_group(&self.0);
        }
    }

    #[test]
    fn a_gap_skips_non_retryable_receipts_and_rechecks_at_claim() {
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
            DeliveryOutcome::Accepted(None),
            DeliveryOutcome::Ambiguous("unknown"),
            DeliveryOutcome::Withdrawn("withheld"),
            DeliveryOutcome::Failed("ordinary failure"),
            DeliveryOutcome::RetriesExhausted("retries used up"),
        ] {
            receipt(&source(), outcome);
        }
        let unsubmitted = source();
        receipt(&unsubmitted, DeliveryOutcome::Claimed);
        receipt(&unsubmitted, DeliveryOutcome::Unsubmitted("not written"));
        let last = source();
        source(); // A later send has its own outcome.
        let page = |limit| {
            inbox::unclaimed_or_unsubmitted_deliveries(
                &home,
                &group,
                &actor.id,
                Some((&first, &last)),
                limit,
            )
            .expect("read eligible gap page")
        };
        let events = page(4);
        assert_eq!(
            events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec![unsubmitted.as_str(), last.as_str()]
        );
        assert!(
            claim_unclaimed_or_unsubmitted(&home, &group, &actor, &unsubmitted, "pty")
                .expect("claim unsubmitted")
        );
        // Once another path records a non-retryable outcome, gap claim refuses it.
        for outcome in [
            DeliveryOutcome::Failed("ordinary failure"),
            DeliveryOutcome::RetriesExhausted("retries used up"),
            DeliveryOutcome::Ambiguous("unknown"),
            DeliveryOutcome::Withdrawn("withheld"),
            DeliveryOutcome::Accepted(None),
        ] {
            let control = source();
            receipt(&control, outcome);
            assert!(
                !claim_unclaimed_or_unsubmitted(&home, &group, &actor, &control, "pty")
                    .expect("recheck non-retryable claim"),
                "must refuse {outcome:?}"
            );
        }
        // Finalizing `last` before claim also prevents duplicate claiming.
        receipt(&last, DeliveryOutcome::Failed("ordinary failure"));
        assert!(
            !claim_unclaimed_or_unsubmitted(&home, &group, &actor, &last, "pty")
                .expect("recheck claim")
        );
        assert!(page(4).is_empty());
    }

    #[test]
    fn whole_history_gap_recovers_unsubmitted_and_unclaimed_without_touching_other_states() {
        let temp = tempfile::tempdir().expect("isolated directory");
        let home = HomeLayout::from_path(temp.path()).expect("isolated home");
        let store = GroupStore::new(home.clone()).expect("group store");
        let mut group = store
            .create("n1 whole history gap", "")
            .expect("create group");
        let _guard = GroupGuard(group.group_id.clone());
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save membership");
        let path = store.ledger_path(&group.group_id).expect("group ledger");
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        workers().lock().expect("workers").insert(
            (group.group_id.clone(), actor.id.clone()),
            DeliveryWorker {
                sender: Some(tx),
                cancelled: Arc::new(AtomicBool::new(false)),
                replaced: Arc::new(AtomicBool::new(false)),
                thread: None,
            },
        );
        let send = |text: &str| {
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data = json!({"to":[actor.id],"text":text,"message_mode":"send"})
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

        let a_unsubmitted = send("A unsubmitted");
        receipt(&a_unsubmitted, DeliveryOutcome::Claimed);
        receipt(
            &a_unsubmitted,
            DeliveryOutcome::Unsubmitted("stopped before write"),
        );
        let c_accepted = send("C accepted");
        receipt(&c_accepted, DeliveryOutcome::Accepted(None));
        let d_claimed = send("D claimed");
        receipt(&d_claimed, DeliveryOutcome::Claimed);
        let e_ambiguous = send("E ambiguous");
        receipt(&e_ambiguous, DeliveryOutcome::Ambiguous("uncertain"));
        let f_withdrawn = send("F withdrawn");
        receipt(&f_withdrawn, DeliveryOutcome::Withdrawn("withheld"));
        let g_exhausted = send("G exhausted");
        receipt(
            &g_exhausted,
            DeliveryOutcome::RetriesExhausted("retries used up"),
        );
        let h_legacy_failed = send("H legacy failed");
        receipt(&h_legacy_failed, DeliveryOutcome::Failed("legacy failed"));
        let b_unclaimed = send("B unclaimed");

        record(&group, &actor, None);
        let prepared = prepare_group(&home, &group, &OwnerObservations::default());
        let trailing = send("appended during preparation");
        let tail = ledger::read_all(&path)
            .expect("read ledger")
            .into_iter()
            .find(|event| event.id == trailing)
            .expect("appended message");
        assert!(extend(&group, &actor, &tail));
        apply_group(&home, &group, prepared);
        let delivered: Vec<String> = rx.try_iter().map(|job| job.event.id).collect();
        assert_eq!(delivered, vec![a_unsubmitted.clone(), b_unclaimed.clone()]);
        assert!(
            gaps()
                .lock()
                .expect("gaps")
                .contains_key(&(group.group_id.clone(), actor.id.clone()))
        );
        recover_group(&home, &group.group_id);

        let delivered: Vec<String> = rx.try_iter().map(|job| job.event.id).collect();
        assert_eq!(delivered, vec![trailing]);

        let events = ledger::read_all(&path).expect("read ledger");
        let claim_count = |source: &str| {
            events
                .iter()
                .filter(|event| {
                    event.kind == "runtime.delivery"
                        && event.data["source_event_id"] == source
                        && event.data["state"] == "claimed"
                })
                .count()
        };
        assert_eq!(claim_count(&a_unsubmitted), 2, "A gets one recovery claim");
        assert_eq!(claim_count(&b_unclaimed), 1, "B gets its first claim");
        assert_eq!(claim_count(&c_accepted), 0);
        assert_eq!(claim_count(&d_claimed), 1);
        assert_eq!(claim_count(&e_ambiguous), 0);
        assert_eq!(claim_count(&f_withdrawn), 0);
        assert_eq!(claim_count(&g_exhausted), 0);
        assert_eq!(claim_count(&h_legacy_failed), 0);

        let mut groups = HashSet::new();
        collect_groups(&mut groups);
        assert!(!groups.contains(&group.group_id));
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
        let events = inbox::unclaimed_or_unsubmitted_deliveries(
            &home,
            &group,
            "peer",
            Some((&first, &direct)),
            2,
        )
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
