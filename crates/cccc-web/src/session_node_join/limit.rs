use std::time::{Duration, Instant};

pub(super) struct Limit {
    start: Instant,
    count: usize,
}
impl Default for Limit {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            count: 0,
        }
    }
}
impl Limit {
    pub fn allow(&mut self, now: Instant, maximum: usize, window: Duration) -> bool {
        if now.saturating_duration_since(self.start) >= window {
            self.start = now;
            self.count = 0;
        }
        if self.count >= maximum {
            return false;
        }
        self.count += 1;
        true
    }
}
