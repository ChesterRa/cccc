//! Native interaction with the resident Secretary. The daemon owns its lifetime.
use super::*;
use cccc_contracts::voice_secretary::SecretaryTaskExecution;
use cccc_runtime::{LaunchSpec, TerminalAttachMode, TerminalAttachment};

const TERMINAL_SCOPE: &str = "voice-secretary-terminal";
const MAX_PROGRESS_BYTES: usize = 8192;

#[derive(Default)]
pub(super) struct Progress {
    visible: String,
    pending: String,
    completed: bool,
}

impl Progress {
    pub(super) fn snapshot(&self) -> String {
        self.visible.clone()
    }

    fn append(&mut self, text: &str, complete: bool, secrets: &[&str]) {
        // Completed items replace the preview; the next delta starts a new item.
        if complete || self.completed {
            self.visible.clear();
            self.pending.clear();
        }
        self.completed = complete;
        let mut input = std::mem::take(&mut self.pending);
        input.push_str(text);
        let mut remaining = input.as_str();
        while !remaining.is_empty() {
            // Do not publish a prefix that could become a secret in the next delta.
            if !complete
                && secrets
                    .iter()
                    .any(|secret| secret.len() > remaining.len() && secret.starts_with(remaining))
            {
                self.pending.push_str(remaining);
                break;
            }
            if let Some(length) = secrets
                .iter()
                .filter(|secret| remaining.starts_with(**secret))
                .map(|secret| secret.len())
                .max()
            {
                self.visible.push_str("[redacted]");
                remaining = &remaining[length..];
            } else {
                let length = remaining.chars().next().expect("nonempty text").len_utf8();
                self.visible.push_str(&remaining[..length]);
                remaining = &remaining[length..];
            }
        }
        if self.visible.len() > MAX_PROGRESS_BYTES {
            let mut start = self.visible.len() - MAX_PROGRESS_BYTES;
            while !self.visible.is_char_boundary(start) {
                start += 1;
            }
            self.visible.drain(..start);
        }
    }
}

struct TerminalTarget {
    owner: Arc<Manager>,
    resident: Arc<resident::Resident>,
    session: Arc<AnalystSession>,
}

fn execution(manager: &Manager, task_id: &str) -> Option<Arc<Execution>> {
    manager
        .grants
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .find(|execution| execution.task_id == task_id)
        .cloned()
}

pub(super) fn observation(
    home: &HomeLayout,
    task: &SecretaryTask,
) -> Option<SecretaryTaskExecution> {
    if !task.phase.executing() {
        return None;
    }
    let owner = lookup(home)?;
    let execution = execution(&owner, &task.task_id)?;
    if *execution.cancel.borrow() {
        return None;
    }
    let session = execution.session.lock().ok()?.clone()?;
    Some(SecretaryTaskExecution {
        generation: session.generation().into(),
        runtime: session.runtime(),
        native_terminal: session.tui_ready(),
        progress: execution.progress.lock().ok()?.snapshot(),
        activity: execution.activity.lock().ok()?.clone(),
    })
}

fn target(home: &HomeLayout, request: &DaemonRequest) -> Result<TerminalTarget, OpError> {
    configuration::require_user(request)?;
    let generation = required_arg(request, "generation")?;
    let owner = lookup(home).ok_or_else(unavailable)?;
    let resident = owner.resident().ok_or_else(unavailable)?;
    let session = resident.session.clone();
    if !resident.available.load(Ordering::Acquire)
        || session.generation() != generation
        || session.structured_only()
    {
        return Err(unavailable());
    }
    Ok(TerminalTarget {
        owner,
        resident,
        session,
    })
}

fn unavailable() -> OpError {
    OpError::new(
        "voice_secretary_terminal_unavailable",
        "This Secretary session no longer has an available native terminal.",
    )
}

fn size(request: &DaemonRequest) -> Result<Option<(u16, u16)>, OpError> {
    if !request.args.contains_key("cols") && !request.args.contains_key("rows") {
        return Ok(None);
    }
    let dimension = |name: &str, minimum: u64| {
        request
            .args
            .get(name)
            .and_then(Value::as_u64)
            .filter(|value| (minimum..=4096).contains(value))
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| OpError::new("invalid_args", format!("invalid terminal {name}")))
    };
    Ok(Some((dimension("cols", 10)?, dimension("rows", 2)?)))
}

fn require_live(resident: &resident::Resident, session: &AnalystSession) -> Result<(), OpError> {
    if !resident.available.load(Ordering::Acquire) || !session.process_running() {
        Err(unavailable())
    } else {
        Ok(())
    }
}

/// Attach to the current resident TUI without starting a model turn or task.
pub(crate) fn attach(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<TerminalAttachment, OpError> {
    let initial_size = size(request)?.unwrap_or((120, 32));
    let TerminalTarget {
        owner,
        resident,
        session,
    } = target(home, request)?;
    let _gate = resident
        .terminal_gate
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    require_live(&resident, &session)?;
    let generation = session.generation();
    if !cccc_runtime::status(TERMINAL_SCOPE, generation).is_ok_and(|status| status.running) {
        cccc_runtime::start(LaunchSpec {
            group_id: TERMINAL_SCOPE.into(),
            actor_id: generation.into(),
            runner: cccc_contracts::RunnerKind::Pty,
            command: session.tui_command(),
            cwd: owner.store.runtime_workspace(),
            env: session.tui_environment(),
            cols: initial_size.0,
            rows: initial_size.1,
        })
        .map_err(|_| {
            OpError::new(
                "voice_secretary_terminal_unavailable",
                "Could not open the native terminal for this Secretary task.",
            )
        })?;
    }
    require_live(&resident, &session)?;
    let since = request.args.get("since").and_then(Value::as_u64);
    let mode = if string_arg(request, "mode").as_deref() == Some("viewer") {
        TerminalAttachMode::Viewer
    } else {
        TerminalAttachMode::Control
    };
    let takeover = mode == TerminalAttachMode::Control
        && crate::dispatch::bool_arg(request, "takeover", false);
    let result = if string_arg(request, "bootstrap").as_deref() == Some("snapshot_v1") {
        cccc_runtime::attach_with_snapshot_and_size(
            TERMINAL_SCOPE,
            generation,
            mode,
            takeover,
            since,
            initial_size.0,
            initial_size.1,
        )
    } else {
        cccc_runtime::attach_with_size(
            TERMINAL_SCOPE,
            generation,
            mode,
            takeover,
            since,
            initial_size.0,
            initial_size.1,
        )
    };
    result.map_err(|_| unavailable())
}

pub(super) fn resize(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (cols, rows) =
        size(request)?.ok_or_else(|| OpError::new("invalid_args", "terminal size is required"))?;
    let TerminalTarget {
        owner: _owner,
        resident,
        session,
    } = target(home, request)?;
    let _gate = resident
        .terminal_gate
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    require_live(&resident, &session)?;
    let attachment_id = request
        .args
        .get("attachment_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| OpError::new("invalid_args", "attachment_id is required"))?;
    let resized = cccc_runtime::resize_from_attachment(
        TERMINAL_SCOPE,
        session.generation(),
        attachment_id,
        cols,
        rows,
    )
    .map_err(|_| unavailable())?;
    object(json!({"resized":resized}))
}

pub(super) fn stop_generation(generation: &str) -> io::Result<()> {
    match cccc_runtime::stop(TERMINAL_SCOPE, generation) {
        Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(_, _)) => Ok(()),
        Err(error) => Err(io::Error::other(error)),
    }
}

impl Execution {
    pub(super) fn observe_progress(
        &self,
        event: &Value,
        turn_id: &str,
        config: &ResolvedAgentRuntime,
        token: &str,
    ) {
        let params = &event["params"];
        if params["turnId"] != turn_id {
            return;
        }
        if event["method"] == "cccc/toolActivity" {
            let title = params["title"].as_str().unwrap_or_default();
            *self.activity.lock().unwrap_or_else(|e| e.into_inner()) =
                safe_diagnostic(&io::Error::other(title), config, token);
            return;
        }
        let (text, replace) = match event["method"].as_str() {
            Some("item/agentMessage/delta") => (params["delta"].as_str(), false),
            Some("item/completed") if params["item"]["type"] == "agentMessage" => {
                (params["item"]["text"].as_str(), true)
            }
            _ => return,
        };
        let Some(text) = text else {
            return;
        };
        let secrets = std::iter::once(token)
            .chain(config.environment.values().map(String::as_str))
            .filter(|secret| !secret.is_empty())
            .collect::<Vec<_>>();
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(text, replace, &secrets);
    }
}
