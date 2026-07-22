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
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    let mut grouped = HashMap::<Key, Vec<DeliveryCompletion>>::new();
    for completion in pending {
        grouped
            .entry((completion.group_id.clone(), completion.actor_id.clone()))
            .or_default()
            .push(completion);
    }
    let mut deferred = VecDeque::new();
    for ((group_id, actor_id), batch) in grouped {
        let Ok(group) = store.load(&group_id) else {
            clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
            continue;
        };
        if !auto_mark_on_delivery(&group) {
            clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
            continue;
        }
        let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
            clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
            continue;
        };
        let unread = inbox::list_unread(home, &group, &actor_id, 1000)
            .unwrap_or_default()
            .into_iter()
            .filter(|event| actor.created_at.is_empty() || event.ts >= actor.created_at)
            .collect::<Vec<_>>();
        let unread_ids = unread
            .iter()
            .map(|event| event.id.clone())
            .collect::<HashSet<_>>();
        let completed_ids = batch
            .iter()
            .map(|completion| completion.event_id.clone())
            .collect::<HashSet<_>>();
        let delivered = unread
            .iter()
            .take_while(|event| completed_ids.contains(&event.id))
            .map(|event| event.id.clone())
            .collect::<Vec<_>>();
        let delivered_ids = delivered.iter().cloned().collect::<HashSet<_>>();
        let resolved_ids = batch
            .iter()
            .filter(|completion| {
                delivered_ids.contains(&completion.event_id)
                    || !unread_ids.contains(&completion.event_id)
            })
            .map(|completion| completion.event_id.clone())
            .collect::<HashSet<_>>();
        for completion in batch {
            if !resolved_ids.contains(&completion.event_id) {
                deferred.push_back(completion);
            }
        }
        clear_in_flight(|item| {
            item.0 == group_id && item.1 == actor_id && resolved_ids.contains(&item.2)
        });
        let advanced = delivered.last().is_some_and(|event_id| {
            inbox::advance(home, &group_id, &actor_id, event_id).unwrap_or(false)
        });
        if advanced {
            record_read_event(&store, &group_id, &actor_id, &delivered);
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

fn record_read_event(store: &GroupStore, group_id: &str, actor_id: &str, delivered: &[String]) {
    let event_id = delivered.last().cloned().unwrap_or_default();
    let mut event = Event::new("chat.read", group_id);
    event.by = actor_id.to_owned();
    event.data = json!({
        "actor_id": actor_id,
        "event_id": event_id,
        "delivered_count": delivered.len(),
        "source": "runtime_delivery",
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    if let Ok(path) = store.ledger_path(group_id) {
        let _ = ledger::append(&path, &event);
    }
}

fn auto_mark_on_delivery(group: &GroupDoc) -> bool {
    group
        .extra
        .get("settings")
        .and_then(|value| value.get("auto_mark_on_delivery"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}
