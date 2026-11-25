//! The daemon's terminal ports address the actual owner. Provider-local code in
//! an owner process keeps using the original in-process runtime implementation.

pub(crate) use ::cccc_runtime::*;

pub(crate) fn status(group: &str, actor: &str) -> Result<SessionStatus, RuntimeError> {
    #[cfg(unix)]
    if let Some(snapshot) = super::snapshot(group, actor).map_err(RuntimeError::Io)? {
        return snapshot
            .terminal
            .ok_or_else(|| RuntimeError::NotFound(group.into(), actor.into()));
    }
    ::cccc_runtime::status(group, actor)
}

pub(crate) fn stop(group: &str, actor: &str) -> Result<SessionStatus, RuntimeError> {
    #[cfg(unix)]
    if let Some(status) = super::stop_actor(group, actor).map_err(RuntimeError::Io)? {
        return status.ok_or_else(|| RuntimeError::NotFound(group.into(), actor.into()));
    }
    ::cccc_runtime::stop(group, actor)
}

macro_rules! observe {
    ($name:ident ($($argument:ident : $kind:ty),*) -> $result:ty, $read:expr) => {
        pub(crate) fn $name(group: &str, actor: &str, $($argument: $kind),*) -> Result<$result, RuntimeError> {
            #[cfg(unix)]
            if let Some(value) = read(group, actor, $read)? {
                return serde_json::from_value(value).map_err(|error| RuntimeError::Io(std::io::Error::other(error)));
            }
            ::cccc_runtime::$name(group, actor, $($argument),*)
        }
    };
}

observe!(history(before: Option<u64>, limit: usize) -> HistoryPage, super::protocol::TerminalRead::History { before, limit });
observe!(retained_history() -> HistoryPage, super::protocol::TerminalRead::Retained);
observe!(retained_history_tail(limit: usize) -> HistoryPage, super::protocol::TerminalRead::RetainedTail { limit });
observe!(history_since(after: u64, limit: usize) -> HistoryPage, super::protocol::TerminalRead::Since { after, limit });
observe!(active_history_replay(after: u64, end: Option<u64>, limit: usize) -> (HistoryPage, u64), super::protocol::TerminalRead::Replay { after, end, limit });
observe!(last_output() -> Option<OutputObservation>, super::protocol::TerminalRead::LastOutput);
observe!(bracketed_paste_enabled() -> bool, super::protocol::TerminalRead::BracketedPaste);
observe!(attachment_writable(attachment_id: u64) -> bool, super::protocol::TerminalRead::Writable { attachment_id });
observe!(terminal_size() -> (u16, u16), super::protocol::TerminalRead::Size);

macro_rules! change {
    ($name:ident ($($argument:ident : $kind:ty),*) -> $result:ty, $action:expr) => {
        pub(crate) fn $name(group: &str, actor: &str, $($argument: $kind),*) -> Result<$result, RuntimeError> {
            #[cfg(unix)]
            if let Some(value) = change(group, actor, $action)? {
                return serde_json::from_value(value).map_err(|error| RuntimeError::Io(std::io::Error::other(error)));
            }
            ::cccc_runtime::$name(group, actor, $($argument),*)
        }
    };
}

change!(write(data: &[u8]) -> (), super::protocol::TerminalAction::Write { data: data.to_vec() });
change!(resize(cols: u16, rows: u16) -> (), super::protocol::TerminalAction::Resize { cols, rows });
change!(resize_from_attachment(attachment_id: u64, cols: u16, rows: u16) -> bool, super::protocol::TerminalAction::ResizeAttachment { attachment_id, cols, rows });
change!(clear() -> (), super::protocol::TerminalAction::Clear);
change!(release_startup_input(generation: &str) -> bool, super::protocol::TerminalAction::ReleaseStartupInput { generation: generation.to_owned() });

#[cfg(unix)]
fn read(
    group: &str,
    actor: &str,
    read: super::protocol::TerminalRead,
) -> Result<Option<serde_json::Value>, RuntimeError> {
    let Some(snapshot) = super::snapshot(group, actor).map_err(RuntimeError::Io)? else {
        return Ok(None);
    };
    super::controller::client_for(&snapshot.actor)
        .map_err(RuntimeError::Io)?
        .terminal_read(snapshot.actor, read)
        .map(Some)
        .map_err(|error| runtime_error(error, group, actor))
}

#[cfg(unix)]
fn change(
    group: &str,
    actor: &str,
    action: super::protocol::TerminalAction,
) -> Result<Option<serde_json::Value>, RuntimeError> {
    let Some(snapshot) = super::snapshot(group, actor).map_err(RuntimeError::Io)? else {
        return Ok(None);
    };
    let releasing_startup = matches!(
        &action,
        super::protocol::TerminalAction::ReleaseStartupInput { .. }
    );
    let result = super::controller::perform(
        &snapshot.actor,
        super::protocol::Mutation::Terminal {
            actor: snapshot.actor.clone(),
            action,
        },
    );
    match result {
        Err(error)
            if releasing_startup
                && matches!(
                    failure_code(&error),
                    Some("actor_not_owned" | "actor_replaced" | "session_replaced")
                ) =>
        {
            // The owner can reject the old target before executing the release.
            Ok(Some(serde_json::Value::Bool(false)))
        }
        result => result
            .map(Some)
            .map_err(|error| runtime_error(error, group, actor)),
    }
}

#[cfg(unix)]
fn runtime_error(error: std::io::Error, group: &str, actor: &str) -> RuntimeError {
    match failure_code(&error) {
        Some("not_found") => RuntimeError::NotFound(group.into(), actor.into()),
        Some("not_running") => RuntimeError::NotRunning(group.into(), actor.into()),
        _ => RuntimeError::Io(error),
    }
}

#[cfg(unix)]
fn failure_code(error: &std::io::Error) -> Option<&str> {
    error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<super::protocol::Failure>())
        .map(|error| error.code.as_str())
}

#[cfg(unix)]
pub(crate) async fn proxy_attachment<S>(
    stream: &mut tokio::io::BufReader<S>,
    request: &cccc_contracts::DaemonRequest,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<bool>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let Some(group) = request
        .args
        .get("group_id")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(false);
    };
    let Some(actor) = request
        .args
        .get("actor_id")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(false);
    };
    let group = group.to_owned();
    let actor = actor.to_owned();
    let target = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let Some(snapshot) = super::snapshot(&group, &actor)? else {
            return Ok(None);
        };
        let client = super::controller::client_for(&snapshot.actor)?;
        let epoch = client.control_epoch()?;
        Ok(Some((client.address.clone(), snapshot.actor, epoch)))
    })
    .await??;
    let Some((address, actor, epoch)) = target else {
        return Ok(false);
    };
    let mut owner = {
        let socket = super::transport::SocketPath::new(&address.socket)?;
        tokio::net::UnixStream::connect(&socket.path).await?
    };
    let mut frame = serde_json::to_vec(&super::protocol::Request::TerminalStream {
        actor,
        epoch,
        request: request.clone(),
    })?;
    frame.push(b'\n');
    owner.write_all(&frame).await?;
    owner.flush().await?;
    tokio::select! {
        copied = tokio::io::copy_bidirectional(stream, &mut owner) => { copied?; },
        _ = shutdown.changed() => {},
    }
    Ok(true)
}
