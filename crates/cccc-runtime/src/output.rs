use serde::Serialize;
use std::collections::VecDeque;

pub(crate) const DEFAULT_CAPACITY: usize = 2_000_000;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPage {
    pub data: String,
    pub start_cursor: u64,
    pub end_cursor: u64,
    pub has_more: bool,
    pub cursor_expired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawHistoryPage {
    pub(crate) data: Vec<u8>,
    pub(crate) start_cursor: u64,
    pub(crate) end_cursor: u64,
    pub(crate) cursor_expired: bool,
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
    pub(crate) fn with_capacity_at(capacity: usize, cursor: u64) -> Self {
        Self {
            start: cursor,
            end: cursor,
            capacity: capacity.max(1),
            ..Self::default()
        }
    }

    pub(crate) const fn end_cursor(&self) -> u64 {
        self.end
    }

    pub fn push(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.update_terminal_modes(data);
        self.chunks.push_back(data.to_vec());
        self.bytes += data.len();
        self.end = self.end.saturating_add(data.len() as u64);
        while self.bytes > self.capacity {
            let Some(front_len) = self.chunks.front().map(Vec::len) else {
                break;
            };
            let excess = self.bytes - self.capacity;
            if front_len <= excess {
                self.chunks.pop_front();
                self.bytes -= front_len;
                self.start = self.start.saturating_add(front_len as u64);
                continue;
            }
            if let Some(front) = self.chunks.front_mut() {
                front.drain(..excess);
            }
            self.bytes -= excess;
            self.start = self.start.saturating_add(excess as u64);
        }
    }

    pub fn page(&self, before: Option<u64>, limit: usize) -> HistoryPage {
        let page_end = before.unwrap_or(self.end).clamp(self.start, self.end);
        let page_start = page_end.saturating_sub(limit.max(1) as u64).max(self.start);
        let bytes = self.bytes_between(page_start, page_end);
        HistoryPage {
            data: cursor_preserving_text(&bytes),
            start_cursor: page_start,
            end_cursor: page_end,
            has_more: page_start > self.start,
            cursor_expired: before.is_some_and(|cursor| cursor < self.start),
        }
    }

    pub fn retained_page(&self) -> HistoryPage {
        let bytes = self.bytes_between(self.start, self.end);
        let complete_len = complete_utf8_prefix_len(&bytes);
        let page_end = self.start.saturating_add(complete_len as u64);
        HistoryPage {
            data: cursor_preserving_text(&bytes[..complete_len]),
            start_cursor: self.start,
            end_cursor: page_end,
            has_more: page_end < self.end,
            cursor_expired: false,
        }
    }

    pub fn retained_tail_page(&self, limit: usize) -> HistoryPage {
        let page_start = self.end.saturating_sub(limit.max(1) as u64).max(self.start);
        let bytes = self.bytes_between(page_start, self.end);
        let complete_len = complete_utf8_prefix_len(&bytes);
        let page_end = page_start.saturating_add(complete_len as u64);
        HistoryPage {
            data: cursor_preserving_text(&bytes[..complete_len]),
            start_cursor: page_start,
            end_cursor: page_end,
            has_more: page_start > self.start || page_end < self.end,
            cursor_expired: false,
        }
    }

    pub fn page_since(&self, after: u64, limit: usize) -> HistoryPage {
        self.page_since_until(after, self.end, limit)
    }

    pub(crate) fn raw_retained_since(&self, after: Option<u64>) -> RawHistoryPage {
        let requested = after.unwrap_or(self.start);
        let page_start = requested.clamp(self.start, self.end);
        RawHistoryPage {
            data: self.bytes_between(page_start, self.end),
            start_cursor: page_start,
            end_cursor: self.end,
            cursor_expired: after.is_some_and(|cursor| cursor < self.start),
        }
    }

    pub(crate) fn raw_page_since(&self, after: u64, limit: usize) -> RawHistoryPage {
        let page_start = after.clamp(self.start, self.end);
        let page_end = page_start.saturating_add(limit.max(1) as u64).min(self.end);
        RawHistoryPage {
            data: self.bytes_between(page_start, page_end),
            start_cursor: page_start,
            end_cursor: page_end,
            cursor_expired: after < self.start,
        }
    }

    pub(crate) fn page_since_until(
        &self,
        after: u64,
        end_cursor: u64,
        limit: usize,
    ) -> HistoryPage {
        let replay_end = end_cursor.clamp(self.start, self.end);
        let page_start = after.clamp(self.start, replay_end);
        let candidate_end = page_start
            .saturating_add(limit.max(1) as u64)
            .min(replay_end);
        let lookahead_end = candidate_end.saturating_add(3).min(replay_end);
        let lookahead = self.bytes_between(page_start, lookahead_end);
        let candidate_len = candidate_end.saturating_sub(page_start) as usize;
        let mut requested_len = candidate_len;
        while requested_len < lookahead.len() && is_utf8_continuation(lookahead[requested_len]) {
            requested_len += 1;
        }
        let complete_len = complete_utf8_prefix_len(&lookahead[..requested_len]);
        let page_end = page_start.saturating_add(complete_len as u64);
        HistoryPage {
            data: cursor_preserving_text(&lookahead[..complete_len]),
            start_cursor: page_start,
            end_cursor: page_end,
            has_more: page_end < replay_end,
            cursor_expired: after < self.start,
        }
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.bytes = 0;
        self.start = self.end;
    }

    pub(crate) fn trim_to(&mut self, limit: usize) {
        let retained_start = self.end.saturating_sub(limit as u64).max(self.start);
        let retained = self.bytes_between(retained_start, self.end);
        self.chunks.clear();
        if !retained.is_empty() {
            self.chunks.push_back(retained);
        }
        self.bytes = self.end.saturating_sub(retained_start) as usize;
        self.start = retained_start;
    }

    pub(crate) const fn retained_bytes(&self) -> usize {
        self.bytes
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

const fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

pub(crate) fn complete_utf8_prefix_len(bytes: &[u8]) -> usize {
    let mut offset = 0;
    while offset < bytes.len() {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(_) => return bytes.len(),
            Err(error) => {
                offset += error.valid_up_to();
                match error.error_len() {
                    Some(invalid) => offset += invalid,
                    None => return offset,
                }
            }
        }
    }
    offset
}

pub(crate) fn cursor_preserving_text(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                output.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                output.push_str(std::str::from_utf8(&remaining[..valid]).unwrap_or_default());
                let invalid = error.error_len().unwrap_or(remaining.len() - valid);
                output.extend(std::iter::repeat_n('?', invalid));
                remaining = &remaining[valid + invalid..];
            }
        }
    }
    output
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
