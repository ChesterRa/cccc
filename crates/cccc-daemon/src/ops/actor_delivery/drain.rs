use super::*;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, ledger};
use serde_json::json;

pub(crate) fn pending_group_ids() -> Vec<String> {
    let mut groups = completions()
        .lock()
        .map(|queue| {
            queue
                .iter()
                .map(|completion| completion.group_id.clone())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    if let Ok(notices) = sender_notices().lock() {
        groups.extend(notices.iter().map(|notice| notice.group_id.clone()));
    }
    if let Ok(workers) = workers().lock() {
        groups.extend(
            workers
                .iter()
                .filter(|(_, worker)| worker.needs_refill)
                .map(|((group_id, _), _)| group_id.clone()),
        );
    }
    groups.into_iter().collect()
}

pub(crate) fn drain_group(home: &HomeLayout, group_id: &str) {
    let pending = take_group_completions(group_id);
    let mut deferred = VecDeque::new();
    for completion in pending {
        match crate::ops::runtime_delivery::append_state(
            home,
            &completion.group_id,
            &completion.actor_id,
            &completion.actor_created_at,
            &completion.event_id,
            &completion.transport,
            completion.outcome.ledger_outcome(),
        ) {
            Ok(_) => clear_in_flight(|item| {
                item.0 == completion.group_id
                    && item.1 == completion.actor_id
                    && item.2 == completion.event_id
            }),
            Err(error) => {
                tracing::warn!(
                    message = %error.message,
                    event_id = %completion.event_id,
                    "failed to persist runtime delivery result"
                );
                deferred.push_back(completion);
            }
        }
    }
    if let Ok(mut completions) = completions().lock() {
        completions.extend(deferred);
    }
    tell_senders(home, group_id);

    let actors = workers()
        .lock()
        .map(|workers| {
            workers
                .iter()
                .filter(|((id, _), worker)| id == group_id && worker.needs_refill)
                .map(|((_, actor_id), _)| actor_id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if actors.is_empty() {
        return;
    }
    let group =
        match cccc_core::GroupStore::new(home.clone()).and_then(|store| store.load(group_id)) {
            Ok(group) => group,
            Err(error) => {
                tracing::warn!(%error, %group_id, "failed to reload pending native deliveries");
                return;
            }
        };
    for actor_id in actors {
        // Offline workers keep their bounded slots. Wait for space rather than
        // repeatedly loading the same full page or probing the native provider.
        let has_capacity = in_flight()
            .lock()
            .map(|pending| {
                pending
                    .iter()
                    .filter(|(group, actor, _)| group == group_id && actor == &actor_id)
                    .count()
                    < QUEUE_CAPACITY
            })
            .unwrap_or(false);
        if has_capacity {
            dispatch_unread(home, &group, &actor_id);
        }
    }
}

/// Runs under the Group's write permit, so the member checked here is the one
/// the notice reaches. Best effort: what fails is logged and dropped.
fn tell_senders(home: &HomeLayout, group_id: &str) {
    let notices = sender_notices()
        .lock()
        .map(|mut queue| {
            let (selected, remaining): (Vec<_>, Vec<_>) = std::mem::take(&mut *queue)
                .into_iter()
                .partition(|notice| notice.group_id == group_id);
            *queue = remaining.into();
            selected
        })
        .unwrap_or_default();
    for notice in notices {
        if let Err(error) = notify_sender(home, &notice) {
            tracing::warn!(
                message = %error.message,
                event_id = %notice.event_id,
                "could not tell the sender how its delivery ended"
            );
        }
    }
}

fn notify_sender(home: &HomeLayout, notice: &SenderNotice) -> Result<(), crate::dispatch::OpError> {
    use crate::dispatch::OpError;
    use crate::ops::messaging_status::{Requester, requester_for_event};
    let (group_id, recipient) = (&notice.group_id, &notice.recipient);
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(group_id).map_err(OpError::io)?;
    let path = store.ledger_path(group_id).map_err(OpError::io)?;
    let Some(source) = ledger::find_event(&path, &notice.event_id).map_err(OpError::io)? else {
        return Ok(());
    };
    // A member added since under the same name did not send this.
    if !cccc_core::actors::visible(&group).any(|actor| actor.id == source.by)
        || !matches!(
            requester_for_event(home, group_id, &source)?,
            Requester::Available
        )
    {
        return Ok(());
    }
    // A notice can arrive late, and a daemon restart can pick failed work up
    // again, so this says what happened, not what holds now. The reason stays
    // out: it can hold another runtime's own text.
    let (title, state, advice) = if notice.uncertain {
        (
            format!("Delivery to {recipient} was uncertain"),
            "ambiguous",
            "It may have reached the agent. Check before sending it again.",
        )
    } else {
        (
            format!("Delivery to {recipient} failed"),
            "failed",
            "Check before you wait for a reply.",
        )
    };
    let client_id = format!("delivery:{state}:{}", notice.delivery_id);
    if ledger::find_idempotent(&path, "system.notify", "system", &client_id)
        .map_err(OpError::io)?
        .is_some()
    {
        return Ok(());
    }
    let request = DaemonRequest {
        v: 1,
        op: "system_notify".into(),
        args: json!({
            "group_id":group_id,"by":"system","client_id":client_id,"kind":"error",
            "title":title,
            "message":format!(
                "This attempt has stopped, and the result may have changed since. {advice} \
                 Source event: {}.",
                source.id
            ),
            "to":[source.by],"target_actor_id":source.by,"im_visibility":"internal",
            "related_event_id":source.id,
            "context":{"delivery_id":notice.delivery_id,"state":state,"actor_id":recipient},
        })
        .as_object()
        .cloned()
        .expect("sender notice"),
    };
    crate::ops::messaging::send(home, &request, "system.notify").map(|_| ())
}

fn take_group_completions(group_id: &str) -> Vec<DeliveryCompletion> {
    completions()
        .lock()
        .map(|mut queue| {
            let mut selected = Vec::new();
            let mut remaining = VecDeque::new();
            while let Some(completion) = queue.pop_front() {
                if completion.group_id == group_id {
                    selected.push(completion);
                } else {
                    remaining.push_back(completion);
                }
            }
            *queue = remaining;
            selected
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::{Actor, Event};
    use cccc_core::{GroupStore, ledger};
    use serde_json::json;

    #[test]
    fn completion_records_delivery_without_advancing_read_cursor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("delivery", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save actor");
        let mut source = Event::new("chat.message", &group.group_id);
        source.by = "user".into();
        source.data = json!({"to":["peer1"],"text":"hello","message_mode":"send"})
            .as_object()
            .cloned()
            .expect("source data");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger"),
            &source,
        )
        .expect("append source");
        record_completion(DeliveryCompletion {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            actor_created_at: actor.created_at.clone(),
            event_id: source.id.clone(),
            transport: "pty".into(),
            outcome: CompletedDelivery::Accepted,
        });

        drain_group(&home, &group.group_id);

        assert!(
            cccc_core::inbox::cursor(&home, &group.group_id, &actor.id)
                .expect("cursor")
                .is_none()
        );
        let events =
            ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger")).expect("events");
        assert!(events.iter().any(|event| {
            event.kind == "runtime.delivery"
                && event.data["source_event_id"] == source.id
                && event.data["state"] == "accepted"
        }));
    }

    #[cfg(unix)]
    #[test]
    fn a_withdrawal_that_cannot_be_recorded_is_retried_as_a_record_only() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("withdrawal retry", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save actor");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let mut notice = Event::new("system.notify", &group.group_id);
        notice.data = json!({"kind":"mail_notice","target_actor_id":"peer1"})
            .as_object()
            .cloned()
            .expect("notice");
        ledger::append(&ledger_path, &notice).expect("append notice");
        assert_eq!(
            crate::ops::runtime_delivery::claim(&home, &group, &actor, &notice.id, "pty", false)
                .expect("claim"),
            crate::ops::runtime_delivery::ClaimResult::Claimed
        );
        let job = DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: notice.clone(),
            claimed: true,
            transport: "pty",
        };
        if let Ok(mut pending) = in_flight().lock() {
            pending.insert((group.group_id.clone(), actor.id.clone(), notice.id.clone()));
        }
        let state = || {
            crate::ops::runtime_delivery::latest_state(&home, &group.group_id, "peer1", &notice.id)
                .expect("state")
                .map(|(state, _)| state)
        };
        let in_flight_now = || {
            in_flight().lock().expect("in flight").contains(&(
                group.group_id.clone(),
                actor.id.clone(),
                notice.id.clone(),
            ))
        };

        // The ledger rejects writes, so the withdrawal cannot be recorded yet.
        let writable = std::fs::metadata(&ledger_path)
            .expect("ledger")
            .permissions();
        std::fs::set_permissions(&ledger_path, std::fs::Permissions::from_mode(0o444))
            .expect("read-only ledger");
        withdraw_jobs(std::slice::from_ref(&job), "held");
        assert_eq!(state().as_deref(), Some("claimed"));
        assert!(in_flight_now(), "the notice keeps its slot until recorded");

        // Once the ledger recovers, only the record is retried.
        std::fs::set_permissions(&ledger_path, writable).expect("restore ledger");
        drain_group(&home, &group.group_id);
        assert_eq!(state().as_deref(), Some("withdrawn"));
        assert!(!in_flight_now());
    }
}
