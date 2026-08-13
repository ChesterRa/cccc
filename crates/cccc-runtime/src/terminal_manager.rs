use crate::RuntimeError;
use crate::registry::{lookup, with_session};
use crate::terminal_attach::{AttachmentRegistry, TerminalAttachMode, TerminalAttachment};

pub fn attach(
    group_id: &str,
    actor_id: &str,
    mode: TerminalAttachMode,
    takeover: bool,
    since: Option<u64>,
) -> Result<TerminalAttachment, RuntimeError> {
    attach_inner(group_id, actor_id, mode, takeover, since, false, None)
}

pub fn attach_with_snapshot(
    group_id: &str,
    actor_id: &str,
    mode: TerminalAttachMode,
    takeover: bool,
    since: Option<u64>,
) -> Result<TerminalAttachment, RuntimeError> {
    attach_inner(group_id, actor_id, mode, takeover, since, true, None)
}

pub fn attach_with_size(
    group_id: &str,
    actor_id: &str,
    mode: TerminalAttachMode,
    takeover: bool,
    since: Option<u64>,
    cols: u16,
    rows: u16,
) -> Result<TerminalAttachment, RuntimeError> {
    attach_inner(
        group_id,
        actor_id,
        mode,
        takeover,
        since,
        false,
        Some((cols, rows)),
    )
}

pub fn attach_with_snapshot_and_size(
    group_id: &str,
    actor_id: &str,
    mode: TerminalAttachMode,
    takeover: bool,
    since: Option<u64>,
    cols: u16,
    rows: u16,
) -> Result<TerminalAttachment, RuntimeError> {
    attach_inner(
        group_id,
        actor_id,
        mode,
        takeover,
        since,
        true,
        Some((cols, rows)),
    )
}

fn attach_inner(
    group_id: &str,
    actor_id: &str,
    mode: TerminalAttachMode,
    takeover: bool,
    since: Option<u64>,
    prefer_snapshot: bool,
    initial_size: Option<(u16, u16)>,
) -> Result<TerminalAttachment, RuntimeError> {
    with_session(group_id, actor_id, |session| {
        session.attach(mode, takeover, since, prefer_snapshot, initial_size)
    })
}

pub fn attachment_writable(
    group_id: &str,
    actor_id: &str,
    attachment_id: u64,
) -> Result<bool, RuntimeError> {
    with_session(group_id, actor_id, |session| {
        session.attachment_writable(attachment_id)
    })
}

pub fn resize_from_attachment(
    group_id: &str,
    actor_id: &str,
    attachment_id: u64,
    cols: u16,
    rows: u16,
) -> Result<bool, RuntimeError> {
    with_session(group_id, actor_id, |session| {
        session.resize_from_attachment(attachment_id, cols, rows)
    })
}

pub(crate) fn write_from_attachment(
    group_id: &str,
    actor_id: &str,
    registry: &AttachmentRegistry,
    attachment_id: u64,
    data: &[u8],
) -> Result<bool, RuntimeError> {
    if data.is_empty() {
        return Ok(true);
    }
    let session = lookup(group_id, actor_id)?;
    let gate = session
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .input_gate();
    let _guard = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
    session
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .write_from_attachment(registry, attachment_id, data)
}
