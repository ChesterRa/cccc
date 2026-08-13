use crate::terminal_sequence_tracker::TerminalSequenceTracker;
use retach::screen::{AnsiRenderer, Screen};
use std::panic::{AssertUnwindSafe, catch_unwind};

const SNAPSHOT_SCROLLBACK_LINES: usize = 512;
const MAX_SNAPSHOT_BYTES: usize = 192 * 1024;
const MAX_MIRROR_COLS: u16 = 1_000;
const MAX_MIRROR_ROWS: u16 = 500;
const MAX_MIRROR_CELLS: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalSnapshot {
    pub(crate) data: Vec<u8>,
    pub(crate) cursor: u64,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

pub(crate) struct TerminalStateMirror {
    screen: Screen,
    sequences: TerminalSequenceTracker,
    enabled: bool,
    cols: u16,
    rows: u16,
}

impl TerminalStateMirror {
    pub(crate) fn new(cols: u16, rows: u16) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let enabled = supported_size(cols, rows);
        Self {
            screen: Screen::new(
                if enabled { cols } else { 1 },
                if enabled { rows } else { 1 },
                SNAPSHOT_SCROLLBACK_LINES,
            ),
            sequences: TerminalSequenceTracker::default(),
            enabled,
            cols,
            rows,
        }
    }

    pub(crate) fn process(&mut self, data: &[u8]) {
        if data.is_empty() || !self.enabled {
            return;
        }
        self.sequences.process(data);
        if !self.sequences.snapshot_safe() {
            self.enabled = false;
            return;
        }
        if catch_unwind(AssertUnwindSafe(|| self.screen.process(data))).is_err() {
            self.enabled = false;
            return;
        }
        self.discard_side_effects();
    }

    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        self.cols = cols;
        self.rows = rows;
        if !self.enabled {
            return;
        }
        if !supported_size(cols, rows) {
            self.enabled = false;
            return;
        }
        if catch_unwind(AssertUnwindSafe(|| {
            self.screen.resize(cols, rows);
        }))
        .is_err()
        {
            self.enabled = false;
        }
    }

    pub(crate) fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::new(self.cols, self.rows);
    }

    pub(crate) fn snapshot(&self, cursor: u64) -> Option<TerminalSnapshot> {
        if !self.enabled || !self.sequences.snapshot_safe() {
            return None;
        }
        let history = self.screen.get_history();
        let mut renderer = AnsiRenderer::new();
        let rendered = catch_unwind(AssertUnwindSafe(|| {
            renderer.render_with_scrollback(&self.screen, &history)
        }))
        .ok()?;
        let main_restore = self.sequences.main_keyboard_restore();
        let active_restore = self.sequences.active_keyboard_restore();
        let pending = self.sequences.pending();
        let extra = main_restore
            .len()
            .saturating_add(active_restore.len())
            .saturating_add(pending.len());
        if rendered.len().saturating_add(extra) > MAX_SNAPSHOT_BYTES {
            return None;
        }

        let mut data = Vec::with_capacity(rendered.len() + extra);
        if self.sequences.alternate_screen() {
            data.extend_from_slice(&main_restore);
        }
        data.extend_from_slice(&rendered);
        data.extend_from_slice(&active_restore);
        data.extend_from_slice(pending);
        if data.is_empty() {
            return None;
        }
        Some(TerminalSnapshot {
            data,
            cursor,
            cols: self.screen.cols(),
            rows: self.screen.rows(),
        })
    }

    fn discard_side_effects(&mut self) {
        self.screen.take_responses();
        self.screen.take_passthrough();
        self.screen.take_queued_notifications();
        self.screen.discard_pending_scrollback();
    }
}

fn supported_size(cols: u16, rows: u16) -> bool {
    cols <= MAX_MIRROR_COLS
        && rows <= MAX_MIRROR_ROWS
        && usize::from(cols).saturating_mul(usize::from(rows) + SNAPSHOT_SCROLLBACK_LINES)
            <= MAX_MIRROR_CELLS
}

#[cfg(test)]
#[path = "terminal_snapshot_tests.rs"]
mod tests;
