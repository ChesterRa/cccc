use crate::RuntimeError;
use crate::output::{HistoryPage, OutputBuffer};
use crate::transcript_archive::{HistoryConfig, TranscriptArchive};
use std::sync::{Arc, Mutex};

struct SessionHistoryState {
    output: OutputBuffer,
    archive: Option<TranscriptArchive>,
    archive_writable: bool,
}

#[derive(Clone)]
pub(crate) struct SessionHistory {
    state: Arc<Mutex<SessionHistoryState>>,
}

impl SessionHistory {
    pub(crate) fn new(config: Option<HistoryConfig>) -> Result<Self, RuntimeError> {
        let capacity = config
            .as_ref()
            .map_or(crate::output::DEFAULT_CAPACITY, |value| value.hot_bytes);
        let archive = config.map(TranscriptArchive::create).transpose()?;
        let cursor = archive.as_ref().map_or(0, TranscriptArchive::end_cursor);
        Ok(Self {
            state: Arc::new(Mutex::new(SessionHistoryState {
                output: OutputBuffer::with_capacity_at(capacity, cursor),
                archive,
                archive_writable: true,
            })),
        })
    }

    pub(crate) fn push(&self, data: &[u8]) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
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

    pub(crate) fn clear(&self) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        state.output.clear();
        state.archive_writable = false;
        let result = match state.archive.as_mut() {
            Some(archive) => archive.clear(),
            None => Ok(()),
        };
        if result.is_ok() {
            state.archive_writable = true;
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
}
