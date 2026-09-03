use super::super::runtime_session;
use super::{Session, SessionTransport, Turn, managed_runtime};
use cccc_contracts::ActorRuntime;
use serde_json::{Value, json};
use std::io;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn initialize_codex(
    session: &Arc<Session>,
    cwd: &std::path::Path,
    model: &str,
    command: &[String],
) -> io::Result<()> {
    session.request(
        "initialize",
        json!({
            "clientInfo":{"name":"cccc","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"experimentalApi":true}
        }),
        Duration::from_secs(10),
    )?;
    let mut start_params = json!({
        "cwd":cwd,
        "approvalPolicy":"never",
        "sandbox":"danger-full-access",
        "personality":"pragmatic"
    });
    if !model.is_empty() {
        start_params["model"] = json!(model);
    }
    let resume_thread_id = runtime_session::prepare_codex_app_thread(
        &session.home,
        &session.group_id,
        &session.actor_id,
        cwd,
        command,
        model,
    )?;
    let (result, resumed) = if let Some(thread_id) = resume_thread_id {
        let mut resume_params = json!({
            "threadId":thread_id,
            "approvalPolicy":"never",
            "sandbox":"danger-full-access",
            "personality":"pragmatic"
        });
        if !model.is_empty() {
            resume_params["model"] = json!(model);
        }
        match session.request("thread/resume", resume_params, Duration::from_secs(20)) {
            Ok(result) => (result, true),
            Err(error) => {
                runtime_session::mark_resume_failed(
                    &session.home,
                    &session.group_id,
                    &session.actor_id,
                    &error.to_string(),
                )?;
                (
                    session.request("thread/start", start_params, Duration::from_secs(20))?,
                    false,
                )
            }
        }
    } else {
        (
            session.request("thread/start", start_params, Duration::from_secs(20))?,
            false,
        )
    };
    let thread_id = result
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if thread_id.is_empty() {
        return Err(io::Error::other(
            "codex app-server returned an empty thread id",
        ));
    }
    if let Err(error) = runtime_session::record_codex_app_thread(
        &session.home,
        &session.group_id,
        &session.actor_id,
        cwd,
        command,
        runtime_session::CodexAppThread {
            id: thread_id,
            resumed,
            runner: cccc_contracts::RunnerKind::Headless,
        },
    ) {
        tracing::warn!(
            %error,
            group_id = %session.group_id,
            actor_id = %session.actor_id,
            "failed to persist Codex app-server thread"
        );
    }
    *session.thread_id.lock().map_err(|_| poisoned())? = thread_id.to_owned();
    Ok(())
}

pub(super) fn submit_managed(session: &Arc<Session>, turn: &Turn) -> io::Result<String> {
    if matches!(session.runtime, ActorRuntime::Grok | ActorRuntime::Opencode) {
        let SessionTransport::ManagedAgent {
            session: managed, ..
        } = &session.transport
        else {
            return Err(io::Error::other(
                "ACP Actor is not connected to its managed session",
            ));
        };
        let request_id = if turn.event_id.trim().is_empty() {
            uuid::Uuid::new_v4().simple().to_string()
        } else {
            turn.event_id.clone()
        };
        return managed_runtime()
            .block_on(managed.start_turn(managed.generation(), &request_id, &turn.text))
            .map(|receipt| receipt.turn_id);
    }
    if session.runtime != ActorRuntime::Codex {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "managed Actor protocol is unavailable for this Runtime",
        ));
    }
    let thread_id = session.thread_id.lock().map_err(|_| poisoned())?.clone();
    let request_id = if turn.event_id.trim().is_empty() {
        uuid::Uuid::new_v4().simple().to_string()
    } else {
        turn.event_id.clone()
    };
    let result = session.request(
        "turn/start",
        json!({
            "threadId":thread_id,
            "input":[{"type":"text","text":turn.text}],
            "clientUserMessageId":format!("cccc-actor:{}:{}:{request_id}", session.group_id, session.actor_id),
            "responsesapiClientMetadata":{
                "cccc_actor_generation":format!("actor:{}:{}", session.group_id, session.actor_id),
                "cccc_turn_correlation_id":request_id,
            }
        }),
        Duration::from_secs(30),
    )?;
    let turn_id = result
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("Codex app-server returned an empty turn id"))?
        .to_owned();
    session.mark_managed_session_materialized();
    Ok(turn_id)
}

pub(super) fn submit_claude(session: &Arc<Session>, turn: &Turn) -> io::Result<String> {
    session.write_json(&json!({
        "type":"user",
        "message":{"role":"user","content":turn.text}
    }))?;
    Ok(uuid::Uuid::new_v4().simple().to_string()[..12].to_owned())
}

fn poisoned() -> io::Error {
    io::Error::other("headless session lock poisoned")
}
