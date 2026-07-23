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
        let completed_ids = batch
            .iter()
            .map(|completion| completion.event_id.clone())
            .collect::<HashSet<_>>();
        let Ok((unread_ids, delivered)) = completion_resolution(
            home,
            &store,
            &group,
            &actor_id,
            &actor.created_at,
            &completed_ids,
        ) else {
            deferred.extend(batch);
            continue;
        };
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

fn completion_resolution(
    home: &HomeLayout,
    store: &GroupStore,
    group: &GroupDoc,
    actor_id: &str,
    actor_created_at: &str,
    completed_ids: &HashSet<String>,
) -> std::io::Result<(HashSet<String>, Vec<String>)> {
    let cursor = inbox::cursor(home, &group.group_id, actor_id)?;
    let path = store.ledger_path(&group.group_id)?;
    ledger::inspect(&path, |events, positions| {
        resolve_completion_prefix(
            events,
            positions,
            cursor.as_deref(),
            group,
            actor_id,
            actor_created_at,
            completed_ids,
        )
    })
}

fn resolve_completion_prefix(
    events: &[Event],
    positions: &HashMap<String, usize>,
    cursor: Option<&str>,
    group: &GroupDoc,
    actor_id: &str,
    actor_created_at: &str,
    completed_ids: &HashSet<String>,
) -> (HashSet<String>, Vec<String>) {
    let start = cursor
        .and_then(|event_id| positions.get(event_id))
        .map_or(0, |index| index + 1);
    let mut completed_unread = HashSet::new();
    let mut delivered = Vec::new();
    let mut prefix_complete = true;
    for event in events[start..].iter().filter(|event| {
        inbox::is_for_actor(group, event, actor_id)
            && (actor_created_at.is_empty() || event.ts.as_str() >= actor_created_at)
    }) {
        let completed = completed_ids.contains(&event.id);
        if completed {
            completed_unread.insert(event.id.clone());
        }
        if prefix_complete && completed {
            delivered.push(event.id.clone());
        } else {
            prefix_complete = false;
        }
    }
    (completed_unread, delivered)
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
    delivery_setting(group, "auto_mark_on_delivery")
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;

    #[test]
    fn resolves_completed_prefix_beyond_legacy_thousand_event_window() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("delivery", "").expect("group");
        group.actors.push(Actor::new("peer1"));
        let mut events = Vec::new();
        let mut positions = HashMap::new();
        let mut completed = HashSet::new();
        for index in 0..1_005 {
            let mut event = Event::new("chat.message", &group.group_id);
            event.id = format!("event-{index}");
            event.by = "user".into();
            event.data = json!({"to":["peer1"],"text":index})
                .as_object()
                .cloned()
                .expect("data");
            positions.insert(event.id.clone(), index);
            completed.insert(event.id.clone());
            events.push(event);
        }

        let (unread, delivered) =
            resolve_completion_prefix(&events, &positions, None, &group, "peer1", "", &completed);
        assert_eq!(unread.len(), 1_005);
        assert_eq!(delivered.len(), 1_005);
        assert_eq!(delivered.last().map(String::as_str), Some("event-1004"));
    }
}
