use crate::terminal_modes::TerminalModes;

const MAX_PENDING_SEQUENCE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceState {
    Ground,
    Escape,
    Csi,
    Osc { escaped: bool },
    UnsupportedString { escaped: bool },
    Utf8 { remaining: u8 },
}

#[derive(Debug)]
pub(crate) struct TerminalSequenceTracker {
    state: SequenceState,
    pending: Vec<u8>,
    snapshot_safe: bool,
    modes: TerminalModes,
}

impl Default for TerminalSequenceTracker {
    fn default() -> Self {
        Self {
            state: SequenceState::Ground,
            pending: Vec::new(),
            snapshot_safe: true,
            modes: TerminalModes::default(),
        }
    }
}

impl TerminalSequenceTracker {
    pub(crate) fn process(&mut self, data: &[u8]) {
        for &byte in data {
            self.advance(byte);
        }
    }

    pub(crate) const fn snapshot_safe(&self) -> bool {
        self.snapshot_safe
    }

    pub(crate) fn pending(&self) -> &[u8] {
        &self.pending
    }

    pub(crate) const fn alternate_screen(&self) -> bool {
        self.modes.alternate_screen()
    }

    pub(crate) fn main_keyboard_restore(&self) -> Vec<u8> {
        self.modes.main_keyboard_restore()
    }

    pub(crate) fn active_keyboard_restore(&self) -> Vec<u8> {
        self.modes.active_keyboard_restore()
    }

    fn advance(&mut self, byte: u8) {
        match self.state {
            SequenceState::Ground => self.advance_ground(byte),
            SequenceState::Escape => self.advance_escape(byte),
            SequenceState::Csi => self.advance_csi(byte),
            SequenceState::Osc { escaped } => self.advance_string(byte, escaped, false),
            SequenceState::UnsupportedString { escaped } => {
                self.advance_string(byte, escaped, true);
            }
            SequenceState::Utf8 { remaining } => self.advance_utf8(byte, remaining),
        }
    }

    fn advance_ground(&mut self, byte: u8) {
        match byte {
            0x1b => self.begin(SequenceState::Escape, byte),
            0x9b => self.begin(SequenceState::Csi, byte),
            0x9d => self.begin(SequenceState::Osc { escaped: false }, byte),
            0x90 | 0x98 | 0x9e | 0x9f => self.begin_unsupported(byte),
            0xc2..=0xdf => self.begin(SequenceState::Utf8 { remaining: 1 }, byte),
            0xe0..=0xef => self.begin(SequenceState::Utf8 { remaining: 2 }, byte),
            0xf0..=0xf4 => self.begin(SequenceState::Utf8 { remaining: 3 }, byte),
            _ => {}
        }
    }

    fn advance_escape(&mut self, byte: u8) {
        self.push_pending(byte);
        match byte {
            b'[' => self.state = SequenceState::Csi,
            b']' => self.state = SequenceState::Osc { escaped: false },
            b'P' | b'X' | b'^' | b'_' => {
                self.snapshot_safe = false;
                self.state = SequenceState::UnsupportedString { escaped: false };
            }
            0x1b => self.restart_escape(),
            0x18 | 0x1a | 0x30..=0x7e => self.complete(),
            _ => {}
        }
    }

    fn advance_csi(&mut self, byte: u8) {
        self.push_pending(byte);
        match byte {
            0x40..=0x7e => {
                self.apply_csi();
                self.complete();
            }
            0x1b => self.restart_escape(),
            0x18 | 0x1a => self.complete(),
            _ => {}
        }
    }

    fn advance_string(&mut self, byte: u8, escaped: bool, unsupported: bool) {
        self.push_pending(byte);
        if (byte == 0x07 && !unsupported) || (escaped && byte == b'\\') {
            self.complete();
        } else {
            let next = byte == 0x1b;
            self.state = if unsupported {
                SequenceState::UnsupportedString { escaped: next }
            } else {
                SequenceState::Osc { escaped: next }
            };
        }
    }

    fn advance_utf8(&mut self, byte: u8, remaining: u8) {
        if byte & 0xc0 != 0x80 {
            self.complete();
            self.advance_ground(byte);
            return;
        }
        self.push_pending(byte);
        if remaining == 1 {
            self.complete();
        } else {
            self.state = SequenceState::Utf8 {
                remaining: remaining - 1,
            };
        }
    }

    fn begin(&mut self, state: SequenceState, byte: u8) {
        self.pending.clear();
        self.pending.push(byte);
        self.state = state;
    }

    fn begin_unsupported(&mut self, byte: u8) {
        self.snapshot_safe = false;
        self.begin(SequenceState::UnsupportedString { escaped: false }, byte);
    }

    fn push_pending(&mut self, byte: u8) {
        if self.pending.len() >= MAX_PENDING_SEQUENCE_BYTES {
            self.snapshot_safe = false;
            return;
        }
        self.pending.push(byte);
    }

    fn restart_escape(&mut self) {
        self.pending.clear();
        self.pending.push(0x1b);
        self.state = SequenceState::Escape;
    }

    fn complete(&mut self) {
        self.pending.clear();
        self.state = SequenceState::Ground;
    }

    fn apply_csi(&mut self) {
        self.modes.apply_csi(&self.pending);
    }
}

#[cfg(test)]
#[path = "terminal_sequence_tracker_tests.rs"]
mod tests;
