use crate::RuntimeError;
use crate::output::RawHistoryPage;
use crate::session_history::{HistorySubscription, OutputState, SessionHistory};
pub(crate) use crate::terminal_attachment_registry::AttachmentRegistry;
use crate::terminal_initial_output::{TerminalInitialOutput, TerminalInitialOutputKind};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalAttachMode {
    Control,
    Viewer,
}

pub(crate) struct TerminalAttachOptions {
    pub(crate) mode: TerminalAttachMode,
    pub(crate) takeover: bool,
    pub(crate) since: Option<u64>,
    pub(crate) prefer_snapshot: bool,
}

impl TerminalAttachMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Viewer => "viewer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOutput {
    pub data: Vec<u8>,
    pub start_cursor: u64,
    pub end_cursor: u64,
}

#[derive(Debug, Clone)]
pub struct TerminalInput {
    group_id: String,
    actor_id: String,
    attachment_id: u64,
    registry: AttachmentRegistry,
}

impl TerminalInput {
    #[must_use]
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    #[must_use]
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// Returns `false` when this attachment is not the current terminal writer.
    pub fn write(&self, data: &[u8]) -> Result<bool, RuntimeError> {
        crate::terminal_manager::write_from_attachment(
            &self.group_id,
            &self.actor_id,
            &self.registry,
            self.attachment_id,
            data,
        )
    }
}

pub struct TerminalAttachment {
    group_id: String,
    actor_id: String,
    attachment_id: u64,
    mode: TerminalAttachMode,
    initially_writable: bool,
    writer_replaced: bool,
    registry: AttachmentRegistry,
    history: SessionHistory,
    changes: watch::Receiver<OutputState>,
    initial: Option<TerminalInitialOutput>,
    cursor: u64,
}

impl TerminalAttachment {
    pub(crate) fn new(
        group_id: String,
        actor_id: String,
        options: TerminalAttachOptions,
        registry: AttachmentRegistry,
        history: SessionHistory,
    ) -> Result<Self, RuntimeError> {
        let HistorySubscription { initial, changes } =
            history.subscribe(options.since, options.prefer_snapshot)?;
        let registration = registry.register(options.mode, options.takeover)?;
        let initial = TerminalInitialOutput::from_history(initial);
        let cursor = initial.end_cursor;
        Ok(Self {
            group_id,
            actor_id,
            attachment_id: registration.id,
            mode: options.mode,
            initially_writable: registration.writable,
            writer_replaced: registration.writer_replaced,
            registry,
            history,
            changes,
            initial: Some(initial),
            cursor,
        })
    }

    #[must_use]
    pub const fn mode(&self) -> TerminalAttachMode {
        self.mode
    }

    #[must_use]
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    #[must_use]
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    #[must_use]
    pub const fn attachment_id(&self) -> u64 {
        self.attachment_id
    }

    #[must_use]
    pub const fn terminal_writable(&self) -> bool {
        self.initially_writable
    }

    #[must_use]
    pub const fn writer_replaced(&self) -> bool {
        self.writer_replaced
    }

    #[must_use]
    pub fn replay_cursor(&self) -> u64 {
        self.initial
            .as_ref()
            .map_or(self.cursor, |initial| initial.start_cursor)
    }

    #[must_use]
    pub fn replay_end_cursor(&self) -> u64 {
        self.cursor
    }

    #[must_use]
    pub fn initial_output_kind(&self) -> TerminalInitialOutputKind {
        self.initial
            .as_ref()
            .map_or(TerminalInitialOutputKind::Replay, |initial| initial.kind)
    }

    #[must_use]
    pub fn initial_output_bytes(&self) -> usize {
        self.initial
            .as_ref()
            .map_or(0, |initial| initial.data.len())
    }

    #[must_use]
    pub fn snapshot_size(&self) -> Option<(u16, u16)> {
        let initial = self.initial.as_ref()?;
        Some((initial.cols?, initial.rows?))
    }

    #[must_use]
    pub fn input(&self) -> TerminalInput {
        TerminalInput {
            group_id: self.group_id.clone(),
            actor_id: self.actor_id.clone(),
            attachment_id: self.attachment_id,
            registry: self.registry.clone(),
        }
    }

    pub fn take_initial_output(&mut self) -> TerminalInitialOutput {
        self.initial
            .take()
            .unwrap_or_else(|| TerminalInitialOutput::empty(self.cursor))
    }

    pub fn take_replay(&mut self) -> TerminalOutput {
        let initial = self.take_initial_output();
        TerminalOutput {
            data: initial.data,
            start_cursor: initial.start_cursor,
            end_cursor: initial.end_cursor,
        }
    }

    pub async fn next_output(
        &mut self,
        limit: usize,
    ) -> Result<Option<TerminalOutput>, RuntimeError> {
        loop {
            let page = self.history.active_raw_page_since(self.cursor, limit)?;
            if page.cursor_expired {
                return Err(RuntimeError::OutputLagged {
                    requested: self.cursor,
                    retained_start: page.start_cursor,
                });
            }
            if !page.data.is_empty() {
                self.cursor = page.end_cursor;
                return Ok(Some(output_from_page(page)));
            }

            let state = *self.changes.borrow_and_update();
            if state.closed && self.cursor >= state.end_cursor {
                return Ok(None);
            }
            if state.end_cursor > self.cursor {
                continue;
            }
            if self.changes.changed().await.is_err() {
                return Ok(None);
            }
        }
    }
}

impl Drop for TerminalAttachment {
    fn drop(&mut self) {
        self.registry.unregister(self.attachment_id);
    }
}

fn output_from_page(page: RawHistoryPage) -> TerminalOutput {
    TerminalOutput {
        data: page.data,
        start_cursor: page.start_cursor,
        end_cursor: page.end_cursor,
    }
}

#[cfg(test)]
#[path = "terminal_attach_tests.rs"]
mod tests;
