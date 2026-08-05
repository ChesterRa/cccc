use serenity::all::MessageId;
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

const CAPACITY: usize = 4096;

#[derive(Default)]
pub(super) struct DiscordMessageDeduper {
    state: Mutex<DedupState>,
}

#[derive(Default)]
struct DedupState {
    ids: HashSet<(String, MessageId)>,
    order: VecDeque<(String, MessageId)>,
}

impl DiscordMessageDeduper {
    pub(super) fn accept(&self, group_id: &str, message_id: MessageId) -> bool {
        let key = (group_id.to_owned(), message_id);
        let mut state = self.state.lock().expect("Discord dedup registry poisoned");
        if !state.ids.insert(key.clone()) {
            return false;
        }
        state.order.push_back(key);
        while state.order.len() > CAPACITY {
            if let Some(expired) = state.order.pop_front() {
                state.ids.remove(&expired);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_message_ids_per_group() {
        let deduper = DiscordMessageDeduper::default();
        assert!(deduper.accept("g_one", MessageId::new(1)));
        assert!(!deduper.accept("g_one", MessageId::new(1)));
        assert!(deduper.accept("g_two", MessageId::new(1)));
    }

    #[test]
    fn evicts_oldest_message_ids_at_capacity() {
        let deduper = DiscordMessageDeduper::default();
        for id in 1..=(CAPACITY as u64 + 1) {
            assert!(deduper.accept("g_one", MessageId::new(id)));
        }
        assert!(deduper.accept("g_one", MessageId::new(1)));
    }
}
