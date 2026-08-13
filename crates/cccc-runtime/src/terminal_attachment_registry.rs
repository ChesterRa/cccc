use crate::RuntimeError;
use crate::terminal_attach::TerminalAttachMode;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub(crate) struct AttachmentRegistry {
    state: Arc<Mutex<AttachmentState>>,
}

#[derive(Debug, Default)]
struct AttachmentState {
    clients: BTreeMap<u64, TerminalAttachMode>,
    writer: Option<u64>,
}

pub(crate) struct AttachmentRegistration {
    pub(crate) id: u64,
    pub(crate) writable: bool,
    pub(crate) writer_replaced: bool,
}

impl AttachmentRegistry {
    pub(crate) fn register(
        &self,
        mode: TerminalAttachMode,
        takeover: bool,
    ) -> Result<AttachmentRegistration, RuntimeError> {
        static NEXT_ATTACHMENT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ATTACHMENT_ID.fetch_add(1, Ordering::Relaxed);
        let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        let previous_writer = state.writer;
        let writable =
            mode == TerminalAttachMode::Control && (previous_writer.is_none() || takeover);
        let writer_replaced = writable && previous_writer.is_some();
        state.clients.insert(id, mode);
        if writable {
            state.writer = Some(id);
        }
        Ok(AttachmentRegistration {
            id,
            writable,
            writer_replaced,
        })
    }

    pub(crate) fn unregister(&self, id: u64) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.clients.remove(&id);
        if state.writer != Some(id) {
            return;
        }
        state.writer = state.clients.iter().find_map(|(candidate, mode)| {
            (*mode == TerminalAttachMode::Control).then_some(*candidate)
        });
    }

    pub(crate) fn is_writer(&self, id: u64) -> Result<bool, RuntimeError> {
        self.state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|state| state.writer == Some(id))
    }

    pub(crate) fn run_if_writer(
        &self,
        id: u64,
        operation: impl FnOnce() -> Result<(), RuntimeError>,
    ) -> Result<bool, RuntimeError> {
        let state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
        if state.writer != Some(id) {
            return Ok(false);
        }
        operation()?;
        Ok(true)
    }

    pub(crate) fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}
