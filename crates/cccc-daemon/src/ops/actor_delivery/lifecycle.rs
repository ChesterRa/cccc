use super::*;

pub fn shutdown_actor(group_id: &str, actor_id: &str) {
    let worker = workers()
        .lock()
        .ok()
        .and_then(|mut workers| workers.remove(&(group_id.to_owned(), actor_id.to_owned())));
    if let Some(worker) = worker {
        worker.shutdown();
    }
    clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
}

pub fn shutdown_group(group_id: &str) {
    let removed = workers()
        .lock()
        .map(|mut workers| {
            let keys = workers
                .keys()
                .filter(|(worker_group_id, _)| worker_group_id == group_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| workers.remove(&key))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for worker in removed {
        worker.shutdown();
    }
    clear_in_flight(|item| item.0 == group_id);
}

pub fn shutdown_all() {
    let removed = workers()
        .lock()
        .map(|mut workers| {
            std::mem::take(&mut *workers)
                .into_values()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for worker in removed {
        worker.shutdown();
    }
    if let Ok(mut completions) = completions().lock() {
        completions.clear();
    }
    if let Ok(mut pending) = in_flight().lock() {
        pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_and_group_shutdown_remove_workers() {
        let first = ("g_cleanup".to_owned(), "actor-1".to_owned());
        let second = ("g_cleanup".to_owned(), "actor-2".to_owned());
        workers().lock().expect("workers").extend([
            (first.clone(), spawn_worker(&first)),
            (second.clone(), spawn_worker(&second)),
        ]);

        shutdown_actor(&first.0, &first.1);
        assert!(!workers().lock().expect("workers").contains_key(&first));
        shutdown_group(&second.0);
        assert!(!workers().lock().expect("workers").contains_key(&second));
    }
}
