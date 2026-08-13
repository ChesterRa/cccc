use crate::session_history::InitialHistory;
use crate::terminal_snapshot::TerminalSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalInitialOutputKind {
    Replay,
    Snapshot,
}

impl TerminalInitialOutputKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Replay => "replay",
            Self::Snapshot => "snapshot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalInitialOutput {
    pub data: Vec<u8>,
    pub start_cursor: u64,
    pub end_cursor: u64,
    pub kind: TerminalInitialOutputKind,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

impl TerminalInitialOutput {
    pub(crate) fn from_history(initial: InitialHistory) -> Self {
        match initial {
            InitialHistory::Replay(page) => Self {
                data: page.data,
                start_cursor: page.start_cursor,
                end_cursor: page.end_cursor,
                kind: TerminalInitialOutputKind::Replay,
                cols: None,
                rows: None,
            },
            InitialHistory::Snapshot(TerminalSnapshot {
                data,
                cursor,
                cols,
                rows,
            }) => Self {
                data,
                start_cursor: cursor,
                end_cursor: cursor,
                kind: TerminalInitialOutputKind::Snapshot,
                cols: Some(cols),
                rows: Some(rows),
            },
        }
    }

    pub(crate) fn empty(cursor: u64) -> Self {
        Self {
            data: Vec::new(),
            start_cursor: cursor,
            end_cursor: cursor,
            kind: TerminalInitialOutputKind::Replay,
            cols: None,
            rows: None,
        }
    }
}
