use crate::RuntimeError;
use crate::output::{HistoryPage, OutputBuffer};
use crate::transcript_archive::{HistoryConfig, TranscriptArchive};
use std::sync::{Arc, Mutex};

struct SessionHistoryState {
    output: OutputBuffer,
    archive: Option<TranscriptArchive>,
    archive_writable: bool,
    accepting_output: bool,
}

#[derive(Clone)]
pub(crate) struct SessionHistory {
    state: Arc<Mutex<SessionHistoryState>>,
}

impl SessionHistory {
    #[cfg(test)]
    pub(crate) fn new(config: Option<HistoryConfig>) -> Result<Self, RuntimeError> {
        Self::new_at(config, 0)
    }

    pub(crate) fn new_at(
        config: Option<HistoryConfig>,
        cursor_floor: u64,
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
        Ok(Self {
            state: Arc::new(Mutex::new(SessionHistoryState {
                output: OutputBuffer::with_capacity_at(capacity, cursor),
                archive,
                archive_writable,
                accepting_output: true,
            })),
        })
    }

    pub(crate) fn end_cursor(&self) -> Result<u64, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.end_cursor())
    }

    pub(crate) fn push(&self, data: &[u8]) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        if !state.accepting_output {
            return Ok(());
        }
        state.output.push(data);
        if !state.archive_writable {
            return Ok(());
        }
        let result = match state.archive.as_mut() {
            Some(archive) => archive.append(data),
            None => Ok(()),
        };
        if result.is_err() {
            state.archive_writable = false;
        }
        result
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
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|mut state| state.accepting_output = false)
    }

    pub(crate) fn bracketed_paste_enabled(&self) -> Result<bool, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.output.bracketed_paste_enabled())
    }
}

#[cfg(test)]
mod tests {
    use super::SessionHistory;
    use crate::transcript_archive::HistoryConfig;

    fn config(root: &std::path::Path) -> HistoryConfig {
        HistoryConfig {
            path: root.join("session.pty"),
            max_bytes: 1024,
            hot_bytes: 1024,
            persist: true,
        }
    }

    #[test]
    fn clear_keeps_archive_and_hot_buffer_aligned() {
        let temp = tempfile::tempdir().expect("tempdir");
        let history = SessionHistory::new(Some(config(temp.path()))).expect("history");
        history.push(b"old").expect("old");
        history.clear().expect("clear");
        history.push(b"new").expect("new");

        assert_eq!(history.page(None, 1024).expect("archive").data, "new");
        assert_eq!(history.retained_page().expect("hot").data, "new");
    }

    #[cfg(unix)]
    #[test]
    fn archive_failure_falls_back_to_the_hot_buffer() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("history");
        let history = SessionHistory::new(Some(config(&root))).expect("history");
        std::fs::remove_dir_all(&root).expect("remove archive directory");

        assert!(history.push(b"first").is_err());
        history.push(b" second").expect("hot-buffer fallback");

        assert_eq!(
            history.page(None, 1024).expect("fallback page").data,
            "first second"
        );
    }

    #[test]
    fn persistence_can_be_disabled_without_losing_hot_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("memory-only");
        let mut history_config = config(&root);
        history_config.persist = false;
        let history = SessionHistory::new(Some(history_config)).expect("history");

        history.push(b"memory only").expect("push");

        assert_eq!(
            history.page(None, 1024).expect("history page").data,
            "memory only"
        );
        assert!(!root.exists());
    }

    #[test]
    fn archive_creation_failure_does_not_block_in_memory_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let blocker = temp.path().join("not-a-directory");
        std::fs::write(&blocker, b"file").expect("blocker");
        let history = SessionHistory::new(Some(config(&blocker))).expect("memory fallback");

        history.push(b"still available").expect("push");

        assert_eq!(
            history.retained_page().expect("hot history").data,
            "still available"
        );
    }

    #[test]
    fn archive_creation_fallback_keeps_the_replacement_cursor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let blocker = temp.path().join("not-a-directory");
        std::fs::write(&blocker, b"file").expect("blocker");
        let history = SessionHistory::new_at(Some(config(&blocker)), 42)
            .expect("memory fallback with cursor");

        history.push(b"replacement").expect("push");

        let page = history.page_since(42, 1024).expect("replacement page");
        assert_eq!(page.data, "replacement");
        assert_eq!(page.start_cursor, 42);
        assert!(!page.cursor_expired);
    }
}
