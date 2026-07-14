use serde::Serialize;
use std::collections::VecDeque;

const DEFAULT_CAPACITY: usize = 2_000_000;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPage {
    pub data: String,
    pub start_cursor: u64,
    pub end_cursor: u64,
    pub has_more: bool,
    pub cursor_expired: bool,
}

pub struct OutputBuffer {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    start: u64,
    end: u64,
    capacity: usize,
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
            start: 0,
            end: 0,
            capacity: DEFAULT_CAPACITY,
        }
    }
}

impl OutputBuffer {
    pub fn push(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.chunks.push_back(data.to_vec());
        self.bytes += data.len();
        self.end = self.end.saturating_add(data.len() as u64);
        while self.bytes > self.capacity {
            let Some(front) = self.chunks.pop_front() else {
                break;
            };
            self.bytes -= front.len();
            self.start = self.start.saturating_add(front.len() as u64);
        }
    }

    pub fn page(&self, before: Option<u64>, limit: usize) -> HistoryPage {
        let bytes = self.all();
        let page_end = before.unwrap_or(self.end).clamp(self.start, self.end);
        let page_start = page_end.saturating_sub(limit.max(1) as u64).max(self.start);
        let from = (page_start - self.start) as usize;
        let to = (page_end - self.start) as usize;
        HistoryPage {
            data: String::from_utf8_lossy(&bytes[from..to]).into_owned(),
            start_cursor: page_start,
            end_cursor: page_end,
            has_more: page_start > self.start,
            cursor_expired: before.is_some_and(|cursor| cursor < self.start),
        }
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.bytes = 0;
        self.start = self.end;
    }

    fn all(&self) -> Vec<u8> {
        self.chunks.iter().flatten().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::OutputBuffer;

    #[test]
    fn history_uses_absolute_cursors() {
        let mut output = OutputBuffer::default();
        output.push(b"hello");
        output.push(b" world");
        let page = output.page(None, 5);
        assert_eq!(page.data, "world");
        assert_eq!(page.start_cursor, 6);
        assert_eq!(page.end_cursor, 11);
    }
}
