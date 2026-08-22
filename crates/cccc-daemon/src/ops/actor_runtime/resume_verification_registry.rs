use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type Key = (String, String);

fn active() -> &'static Mutex<HashMap<Key, String>> {
    static ACTIVE: OnceLock<Mutex<HashMap<Key, String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) struct Registration {
    group_id: String,
    actor_id: String,
    started_at: String,
}

impl Registration {
    pub(super) fn new(group_id: &str, actor_id: &str, started_at: &str) -> Self {
        active()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                (group_id.to_owned(), actor_id.to_owned()),
                started_at.to_owned(),
            );
        Self {
            group_id: group_id.to_owned(),
            actor_id: actor_id.to_owned(),
            started_at: started_at.to_owned(),
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut active = active()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (self.group_id.clone(), self.actor_id.clone());
        if active
            .get(&key)
            .is_some_and(|current| current == &self.started_at)
        {
            active.remove(&key);
        }
    }
}

pub(super) fn cancel(group_id: &str, actor_id: &str) {
    active()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&(group_id.to_owned(), actor_id.to_owned()));
}

#[cfg(test)]
pub(super) fn cancel_if_current(group_id: &str, actor_id: &str, started_at: &str) {
    let mut active = active()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (group_id.to_owned(), actor_id.to_owned());
    if active
        .get(&key)
        .is_some_and(|current| current == started_at)
    {
        active.remove(&key);
    }
}

pub(super) fn cancel_all() {
    active()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

pub(super) fn is_current(group_id: &str, actor_id: &str, started_at: &str) -> bool {
    active()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(group_id.to_owned(), actor_id.to_owned()))
        .is_some_and(|current| current == started_at)
}
