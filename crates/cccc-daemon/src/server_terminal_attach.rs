use anyhow::{Result, anyhow};
use cccc_contracts::{DaemonRequest, DaemonResponse, RunnerKind};
use cccc_core::{GroupStore, HomeLayout};
use cccc_runtime::{TerminalAttachMode, TerminalAttachment, TerminalInput};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::watch;

const OUTPUT_PAGE_BYTES: usize = 64 * 1024;

pub(crate) async fn handle<S>(
    mut stream: BufReader<S>,
    home: HomeLayout,
    request: DaemonRequest,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut attachment = match prepare(&home, &request) {
        Ok(attachment) => attachment,
        Err(response) => {
            write_response(stream.get_mut(), &response).await?;
            return Ok(());
        }
    };
    let response = attach_response(&attachment);
    write_response(stream.get_mut(), &response).await?;

    let input = attachment.input();
    let (read, write) = tokio::io::split(stream);
    tokio::select! {
        result = pump_output(write, &mut attachment) => result,
        result = pump_input(read, input, home) => result,
        changed = shutdown.changed() => {
            changed.ok();
            Ok(())
        }
    }
}

fn prepare(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<TerminalAttachment, DaemonResponse> {
    let group_id = non_blank(request, "group_id")
        .ok_or_else(|| DaemonResponse::failure("missing_group_id", "missing group_id"))?;
    let actor_id = non_blank(request, "actor_id")
        .ok_or_else(|| DaemonResponse::failure("missing_actor_id", "missing actor_id"))?;
    let store = GroupStore::new(home.clone())
        .map_err(|error| DaemonResponse::failure("daemon_error", error.to_string()))?;
    let group = store.load(&group_id).map_err(|_| {
        DaemonResponse::failure("group_not_found", format!("group not found: {group_id}"))
    })?;
    let actor = cccc_core::actors::find(&group, &actor_id).ok_or_else(|| {
        DaemonResponse::failure("actor_not_found", format!("actor not found: {actor_id}"))
    })?;
    if actor.runner == RunnerKind::Headless || crate::ops::actor_runtime::is_structured(actor) {
        let mut response = DaemonResponse::failure(
            "not_pty_actor",
            "terminal attach is only available for PTY actors",
        );
        if let Some(error) = response.error.as_mut() {
            error.details.insert("runner".into(), json!(actor.runner));
            error
                .details
                .insert("runner_effective".into(), json!("headless"));
        }
        return Err(response);
    }

    let mode = if non_blank(request, "mode").is_some_and(|mode| mode.eq_ignore_ascii_case("viewer"))
    {
        TerminalAttachMode::Viewer
    } else {
        TerminalAttachMode::Control
    };
    let takeover = mode == TerminalAttachMode::Control
        && request
            .args
            .get("takeover")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let since = request.args.get("since").and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    });
    // The runtime applies this size while holding the same session lock used
    // to register the writer and capture its initial output.
    let initial_size = (mode == TerminalAttachMode::Control && takeover)
        .then(|| requested_size(request))
        .flatten();
    let snapshot_requested = non_blank(request, "bootstrap").as_deref() == Some("snapshot_v1");
    let attached = match (snapshot_requested, initial_size) {
        (true, Some((cols, rows))) => cccc_runtime::attach_with_snapshot_and_size(
            &group_id, &actor_id, mode, takeover, since, cols, rows,
        ),
        (true, None) => {
            cccc_runtime::attach_with_snapshot(&group_id, &actor_id, mode, takeover, since)
        }
        (false, Some((cols, rows))) => {
            cccc_runtime::attach_with_size(&group_id, &actor_id, mode, takeover, since, cols, rows)
        }
        (false, None) => cccc_runtime::attach(&group_id, &actor_id, mode, takeover, since),
    };
    attached.map_err(runtime_error)
}

fn runtime_error(error: cccc_runtime::RuntimeError) -> DaemonResponse {
    match error {
        cccc_runtime::RuntimeError::NotFound(_, _)
        | cccc_runtime::RuntimeError::NotRunning(_, _) => {
            DaemonResponse::failure("actor_not_running", "actor is not running")
        }
        other => DaemonResponse::failure("runtime_error", other.to_string()),
    }
}

fn attach_response(attachment: &TerminalAttachment) -> DaemonResponse {
    let mut result = json!({
        "group_id": attachment.group_id(),
        "actor_id": attachment.actor_id(),
        "attachment_id": attachment.attachment_id(),
        "terminal_mode": attachment.mode().as_str(),
        "terminal_writable": attachment.terminal_writable(),
        "writer_replaced": attachment.writer_replaced(),
        "terminal_response_owner": "server_v1",
        "replay_cursor": attachment.replay_cursor(),
        "replay_end_cursor": attachment.replay_end_cursor(),
    });
    let mut initial = json!({
        "kind": attachment.initial_output_kind().as_str(),
        "bytes": attachment.initial_output_bytes(),
        "cursor": attachment.replay_end_cursor(),
    });
    if let Some((cols, rows)) = attachment.snapshot_size() {
        initial["cols"] = json!(cols);
        initial["rows"] = json!(rows);
    }
    result["initial_output"] = initial;
    DaemonResponse::success(result.as_object().cloned().unwrap_or_default())
}

async fn write_response<W>(write: &mut W, response: &DaemonResponse) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut payload = serde_json::to_vec(response)?;
    payload.push(b'\n');
    write.write_all(&payload).await?;
    write.flush().await?;
    Ok(())
}

async fn pump_output<W>(mut write: W, attachment: &mut TerminalAttachment) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let initial = attachment.take_initial_output();
    if !initial.data.is_empty() {
        write.write_all(&initial.data).await?;
        write.flush().await?;
    }
    while let Some(output) = attachment.next_output(OUTPUT_PAGE_BYTES).await? {
        write.write_all(&output.data).await?;
        write.flush().await?;
    }
    Ok(())
}

async fn pump_input<R>(mut read: R, input: TerminalInput, home: HomeLayout) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let group_id = input.group_id().to_owned();
    let actor_id = input.actor_id().to_owned();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = read.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        let data = buffer[..count].to_vec();
        let writer = input.clone();
        let written = tokio::task::spawn_blocking(move || writer.write(&data))
            .await
            .map_err(|error| anyhow!("terminal input task failed: {error}"))??;
        if written {
            crate::ops::runtime_hook_input::observe(&home, &group_id, &actor_id, &buffer[..count]);
        }
    }
}

fn non_blank(request: &DaemonRequest, name: &str) -> Option<String> {
    request
        .args
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn requested_size(request: &DaemonRequest) -> Option<(u16, u16)> {
    let dimension = |name: &str, minimum: u64| {
        request
            .args
            .get(name)
            .and_then(Value::as_u64)
            .filter(|value| (minimum..=4096).contains(value))
            .and_then(|value| u16::try_from(value).ok())
    };
    Some((dimension("cols", 10)?, dimension("rows", 2)?))
}
