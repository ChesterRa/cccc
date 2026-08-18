use crate::RuntimeError;
use crate::output::{HistoryPage, OutputBuffer};
use crate::terminal_snapshot::TerminalStateMirror;
use crate::transcript_archive::{HistoryConfig, TranscriptArchive};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

mod subscription;
pub(crate) use subscription::{HistorySubscription, InitialHistory, OutputState};

struct SessionHistoryState {
    output: OutputBuffer,
    archive: Option<TranscriptArchive>,
    archive_writable: bool,
    accepting_output: bool,
    terminal: TerminalStateMirror,
}

#[derive(Clone)]
pub(crate) struct SessionHistory {
    state: Arc<Mutex<SessionHistoryState>>,
    changes: watch::Sender<OutputState>,
}

pub(crate) struct HistoryPush {
    pub(crate) terminal_responses: Vec<Vec<u8>>,
    pub(crate) archive_error: Option<RuntimeError>,
}

impl SessionHistory {
    #[cfg(test)]
    pub(crate) fn new(config: Option<HistoryConfig>) -> Result<Self, RuntimeError> {
        Self::new_at(config, 0)
    }

    #[cfg(test)]
    pub(crate) fn new_at(
        config: Option<HistoryConfig>,
        cursor_floor: u64,
    ) -> Result<Self, RuntimeError> {
        Self::new_at_with_size(config, cursor_floor, 80, 24)
    }

    pub(crate) fn new_at_with_size(
        config: Option<HistoryConfig>,
        cursor_floor: u64,
        cols: u16,
        rows: u16,
    ) -> Result<Self, RuntimeError> {
        let capacity = config
            .as_ref()
            .map_or(crate::output::DEFAULT_CAPACITY, |value| value.hot_bytes);
        let archive = match config.filter(|value| value.persist) {
            Some(config) => match TranscriptArchive::create_at(config, cursor_floor) {
                Ok(archive) => Some(archive),
                Err(error) => {
                    eprintln!(
                        "CCCC terminal transcript archive unavailable; using memory only: {error}"
                    );
                    None
                }
            },
            None => None,
        };
        let cursor = archive
            .as_ref()
            .map_or(cursor_floor, TranscriptArchive::end_cursor);
        let archive_writable = archive.is_some();
        let (changes, _) = watch::channel(OutputState {
            end_cursor: cursor,
            closed: false,
        });
        Ok(Self {
            state: Arc::new(Mutex::new(SessionHistoryState {
                output: OutputBuffer::with_capacity_at(capacity, cursor),
                archive,
                archive_writable,
                accepting_output: true,
                terminal: TerminalStateMirror::new(cols, rows),
            })),
            changes,
        })
    }

    pub(crate) fn end_cursor(&self) -> Result<u64, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.end_cursor())
    }

    #[cfg(test)]
    pub(crate) fn push(&self, data: &[u8]) -> Result<(), RuntimeError> {
        let outcome = self.push_with_terminal_responses(data)?;
        match outcome.archive_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) fn push_with_terminal_responses(
        &self,
        data: &[u8],
    ) -> Result<HistoryPush, RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        if !state.accepting_output {
            return Ok(HistoryPush {
                terminal_responses: Vec::new(),
                archive_error: None,
            });
        }
        let responses = state.terminal.process(data);
        state.output.push(data);
        let end_cursor = state.output.end_cursor();
        let result = if state.archive_writable {
            match state.archive.as_mut() {
                Some(archive) => archive.append(data),
                None => Ok(()),
            }
        } else {
            Ok(())
        };
        if result.is_err() {
            state.archive_writable = false;
        }
        self.changes.send_replace(OutputState {
            end_cursor,
            closed: false,
        });
        Ok(HistoryPush {
            terminal_responses: responses,
            archive_error: result.err(),
        })
    }

    pub(crate) fn page(
        &self,
        before: Option<u64>,
        limit: usize,
    ) -> Result<HistoryPage, RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        if state.archive_writable
            && let Some(archive) = state.archive.as_mut()
        {
            return archive.page(before, limit);
        }
        Ok(state.output.page(before, limit))
    }

    pub(crate) fn page_since(&self, after: u64, limit: usize) -> Result<HistoryPage, RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        if state.archive_writable
            && let Some(archive) = state.archive.as_mut()
        {
            return archive.page_since(after, limit);
        }
        Ok(state.output.page_since(after, limit))
    }

    pub(crate) fn retained_page(&self) -> Result<HistoryPage, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.retained_page())
    }

    pub(crate) fn retained_tail_page(&self, limit: usize) -> Result<HistoryPage, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.retained_tail_page(limit))
    }

    pub(crate) fn active_page_since(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<HistoryPage, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.page_since(after, limit))
    }

    pub(crate) fn active_replay_page(
        &self,
        after: u64,
        end_cursor: Option<u64>,
        limit: usize,
    ) -> Result<(HistoryPage, u64), RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| {
                let complete_end = state.output.retained_page().end_cursor;
                let replay_end = end_cursor.map_or(complete_end, |cursor| cursor.min(complete_end));
                (
                    state.output.page_since_until(after, replay_end, limit),
                    replay_end,
                )
            })
    }

    pub(crate) fn trim_retained(&self, limit: usize) -> Result<(), RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .output
            .trim_to(limit);
        Ok(())
    }

    pub(crate) fn retained_bytes(&self) -> Result<usize, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.retained_bytes())
    }

    pub(crate) fn clear(&self) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        state.output.clear();
        state.terminal.clear();
        state.archive_writable = false;
        let result = match state.archive.as_mut() {
            Some(archive) => archive.clear(),
            None => Ok(()),
        };
        if result.is_ok() {
            state.archive_writable = state.archive.is_some();
        }
        result
    }

    pub(crate) fn flush(&self) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        if !state.archive_writable {
            return Ok(());
        }
        let result = match state.archive.as_mut() {
            Some(archive) => archive.flush(),
            None => Ok(()),
        };
        if result.is_err() {
            state.archive_writable = false;
        }
        result
    }

    pub(crate) fn seal_output(&self) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        state.accepting_output = false;
        self.changes.send_replace(OutputState {
            end_cursor: state.output.end_cursor(),
            closed: true,
        });
        Ok(())
    }

    pub(crate) fn bracketed_paste_enabled(&self) -> Result<bool, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.bracketed_paste_enabled())
    }
}
