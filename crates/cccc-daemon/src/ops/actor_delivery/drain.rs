use super::*;

pub(crate) fn pending_group_ids() -> Vec<String> {
    completions()
        .lock()
        .map(|queue| {
            queue
                .iter()
                .map(|completion| completion.group_id.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn drain_group(home: &HomeLayout, group_id: &str) {
    let pending = take_group_completions(group_id);
    if pending.is_empty() {
        return;
    }
    let mut deferred = VecDeque::new();
    for completion in pending {
        match crate::ops::runtime_delivery::append_state(
            home,
            &completion.group_id,
            &completion.actor_id,
            &completion.actor_created_at,
            &completion.event_id,
            &completion.transport,
            crate::ops::runtime_delivery::DeliveryOutcome::Accepted,
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
}
