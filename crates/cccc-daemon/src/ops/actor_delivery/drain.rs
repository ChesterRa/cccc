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
        let advance_result = delivered
            .last()
            .map(|event_id| inbox::advance(home, &group_id, &actor_id, event_id));
        let cursor_write_failed = advance_result.as_ref().is_some_and(Result::is_err);
        for completion in batch {
            let resolved = resolved_ids.contains(&completion.event_id);
            let delivered_item = delivered.iter().any(|id| id == &completion.event_id);
            let unread_completed_beyond_prefix =
                unread_ids.contains(&completion.event_id) && !delivered_item;
            if !resolved
                || (cursor_write_failed && delivered_item)
                || unread_completed_beyond_prefix
            {
                deferred.push_back(completion);
            }
        }
        clear_in_flight(|item| {
            item.0 == group_id
                && item.1 == actor_id
                && resolved_ids.contains(&item.2)
                && !(cursor_write_failed && delivered.iter().any(|id| id == &item.2))
                && (!unread_ids.contains(&item.2) || delivered.iter().any(|id| id == &item.2))
        });
        if advance_result
            .as_ref()
            .is_some_and(|result| result.as_ref().is_ok_and(|advanced| *advanced))
        {
            record_read_event(&store, &group_id, &actor_id, &delivered);
            dispatch_unread(home, &group, &actor_id);
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
    let generations = inbox::actor_generation_positions(events);
    for (_, event) in events[start..]
        .iter()
        .enumerate()
        .filter(|(offset, event)| {
            let exists = generations
                .get(actor_id)
                .map(|generation| start + offset >= *generation)
                .unwrap_or_else(|| {
                    actor_created_at.is_empty() || event.ts.as_str() >= actor_created_at
                });
            inbox::is_for_actor(group, event, actor_id) && exists
        })
    {
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
    use cccc_contracts::{Actor, ActorRuntime};

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

    #[test]
    fn draining_a_full_recovery_window_refills_from_the_canonical_inbox() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("delivery refill", "").expect("group");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Custom;
        actor.command = vec!["sh".into(), "-c".into(), "sleep 30".into()];
        group.actors.push(actor);
        store.save(&group).expect("save actor");

        let mut event_ids = Vec::new();
        for index in 0..=QUEUE_CAPACITY {
            let mut event = Event::new("chat.message", &group.group_id);
            event.id = format!("event-{index}");
            event.by = "user".into();
            event.data = json!({"to":["peer1"],"text":index})
                .as_object()
                .cloned()
                .expect("data");
            ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
                .expect("append event");
            event_ids.push(event.id);
        }
        for event_id in event_ids.iter().take(QUEUE_CAPACITY) {
            in_flight().lock().expect("in flight").insert((
                group.group_id.clone(),
                "peer1".into(),
                event_id.clone(),
            ));
            record_completion(DeliveryCompletion {
                group_id: group.group_id.clone(),
                actor_id: "peer1".into(),
                event_id: event_id.clone(),
            });
        }

        drain_group(&home, &group.group_id);

        assert_eq!(
            inbox::cursor(&home, &group.group_id, "peer1").expect("cursor"),
            Some(event_ids[QUEUE_CAPACITY - 1].clone())
        );
        assert!(in_flight().lock().expect("in flight").contains(&(
            group.group_id.clone(),
            "peer1".into(),
            event_ids[QUEUE_CAPACITY].clone(),
        )));

        shutdown_actor(&group.group_id, "peer1");
        let _ = cccc_runtime::stop(&group.group_id, "peer1");
    }

    #[test]
    fn shared_durability_vector_does_not_skip_failed_prefix() {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/deepseek_durability_vectors.json"
        ))
        .expect("durability vector");
        assert!(
            vector["expected"]["delivered_prefix"]
                .as_array()
                .is_some_and(|value| value.is_empty())
        );
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("deepseek prefix", "").expect("group");
        group.actors.push(Actor::new("deepseek"));
        let mut first = Event::new("chat.message", &group.group_id);
        first.id = "event-1".into();
        first.by = "user".into();
        first.data = json!({"to":["deepseek"],"text":"first"})
            .as_object()
            .expect("first event data")
            .clone();
        let mut second = Event::new("chat.message", &group.group_id);
        second.id = "event-2".into();
        second.by = "user".into();
        second.data = json!({"to":["deepseek"],"text":"second"})
            .as_object()
            .expect("second event data")
            .clone();
        let events = vec![first, second];
        let positions = HashMap::from([(String::from("event-1"), 0), (String::from("event-2"), 1)]);
        let completed = HashSet::from([String::from("event-2")]);
        let (_, delivered) = resolve_completion_prefix(
            &events, &positions, None, &group, "deepseek", "", &completed,
        );
        assert!(delivered.is_empty());
    }
}
