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
    bracketed_paste: bool,
    mode_probe: Vec<u8>,
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
            start: 0,
            end: 0,
            capacity: DEFAULT_CAPACITY,
            bracketed_paste: false,
            mode_probe: Vec::new(),
        }
    }
}

impl OutputBuffer {
    pub fn push(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.update_terminal_modes(data);
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
        let page_end = before.unwrap_or(self.end).clamp(self.start, self.end);
        let page_start = page_end.saturating_sub(limit.max(1) as u64).max(self.start);
        HistoryPage {
            data: String::from_utf8_lossy(&self.bytes_between(page_start, page_end)).into_owned(),
            start_cursor: page_start,
            end_cursor: page_end,
            has_more: page_start > self.start,
            cursor_expired: before.is_some_and(|cursor| cursor < self.start),
        }
    }

    pub fn page_since(&self, after: u64, limit: usize) -> HistoryPage {
        let page_start = after.clamp(self.start, self.end);
        let page_end = page_start.saturating_add(limit.max(1) as u64).min(self.end);
        HistoryPage {
            data: String::from_utf8_lossy(&self.bytes_between(page_start, page_end)).into_owned(),
            start_cursor: page_start,
            end_cursor: page_end,
            has_more: page_end < self.end,
            cursor_expired: after < self.start,
        }
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.bytes = 0;
        self.start = self.end;
    }

    pub const fn bracketed_paste_enabled(&self) -> bool {
        self.bracketed_paste
    }

    fn update_terminal_modes(&mut self, data: &[u8]) {
        self.mode_probe.extend_from_slice(data);
        for window in self.mode_probe.windows(8) {
            if window == b"\x1b[?2004h" {
                self.bracketed_paste = true;
            } else if window == b"\x1b[?2004l" {
                self.bracketed_paste = false;
            }
        }
        if self.mode_probe.len() > 16 {
            self.mode_probe.drain(..self.mode_probe.len() - 16);
        }
    }

    fn bytes_between(&self, start: u64, end: u64) -> Vec<u8> {
        let relative_start = start.saturating_sub(self.start) as usize;
        let relative_end = end.saturating_sub(self.start) as usize;
        let mut result = Vec::with_capacity(relative_end.saturating_sub(relative_start));
        let mut chunk_start = 0;
        for chunk in &self.chunks {
            let chunk_end = chunk_start + chunk.len();
            let from = relative_start.saturating_sub(chunk_start).min(chunk.len());
            let to = relative_end.saturating_sub(chunk_start).min(chunk.len());
            if from < to {
                result.extend_from_slice(&chunk[from..to]);
            }
            if chunk_end >= relative_end {
                break;
            }
            chunk_start = chunk_end;
        }
        result
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

    #[test]
    fn history_since_only_returns_new_output() {
        let mut output = OutputBuffer::default();
        output.push(b"hello");
        let first = output.page_since(u64::MAX, 1024);
        assert!(first.data.is_empty());
        assert_eq!(first.end_cursor, 5);

        output.push(b" world");
        let next = output.page_since(first.end_cursor, 1024);
        assert_eq!(next.data, " world");
        assert_eq!(next.start_cursor, 5);
        assert_eq!(next.end_cursor, 11);
        assert!(!next.has_more);
    }

    #[test]
    fn tracks_bracketed_paste_across_output_chunks() {
        let mut output = OutputBuffer::default();
        output.push(b"\x1b[?20");
        output.push(b"04hready");
        assert!(output.bracketed_paste_enabled());
        output.push(b"\x1b[?2004l");
        assert!(!output.bracketed_paste_enabled());
    }
}
