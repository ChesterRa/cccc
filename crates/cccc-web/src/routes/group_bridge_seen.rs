use std::collections::{HashSet, VecDeque};

const MAX_SEEN_EVENTS: usize = 256;

#[derive(Default)]
pub(super) struct SeenEvents {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl SeenEvents {
    pub(super) fn insert(&mut self, event_id: String) -> bool {
        if !self.ids.insert(event_id.clone()) {
            return false;
        }
        self.order.push_back(event_id);
        while self.order.len() > MAX_SEEN_EVENTS {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::SeenEvents;

    #[test]
    fn event_deduplication_memory_is_bounded() {
        let mut seen = SeenEvents::default();
        for index in 0..1_000 {
            assert!(seen.insert(format!("event-{index}")));
        }
        assert!(seen.ids.len() <= 256, "seen ids grew to {}", seen.ids.len());
        assert!(!seen.insert("event-999".into()));
    }
}
