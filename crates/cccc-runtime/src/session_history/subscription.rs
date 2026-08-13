use crate::RuntimeError;
use crate::output::RawHistoryPage;
use crate::terminal_snapshot::TerminalSnapshot;
use tokio::sync::watch;

use super::SessionHistory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputState {
    pub(crate) end_cursor: u64,
    pub(crate) closed: bool,
}

pub(crate) struct HistorySubscription {
    pub(crate) initial: InitialHistory,
    pub(crate) changes: watch::Receiver<OutputState>,
}

pub(crate) enum InitialHistory {
    Replay(RawHistoryPage),
    Snapshot(TerminalSnapshot),
}

impl SessionHistory {
    pub(crate) fn subscribe(
        &self,
        after: Option<u64>,
        prefer_snapshot: bool,
    ) -> Result<HistorySubscription, RuntimeError> {
        let state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        let changes = self.changes.subscribe();
        let requested = state.output.raw_retained_since(after);
        let should_snapshot = prefer_snapshot && (after.is_none() || requested.cursor_expired);
        let initial = if should_snapshot {
            state
                .terminal
                .snapshot(state.output.end_cursor())
                .map(InitialHistory::Snapshot)
                .unwrap_or_else(|| InitialHistory::Replay(state.output.raw_retained_since(None)))
        } else {
            InitialHistory::Replay(requested)
        };
        Ok(HistorySubscription { initial, changes })
    }

    pub(crate) fn resize_terminal_with(
        &self,
        cols: u16,
        rows: u16,
        resize_pty: impl FnOnce() -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        if state.terminal.size() == (cols, rows) {
            return Ok(());
        }
        resize_pty()?;
        state.terminal.resize(cols, rows);
        Ok(())
    }

    pub(crate) fn active_raw_page_since(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<RawHistoryPage, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.raw_page_since(after, limit))
    }
}
