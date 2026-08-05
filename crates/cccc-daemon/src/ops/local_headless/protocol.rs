use super::{Session, Turn};
use serde_json::{Value, json};
use std::io;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn initialize_codex(
    session: &Arc<Session>,
    cwd: &std::path::Path,
    model: &str,
) -> io::Result<()> {
    session.request(
        "initialize",
        json!({
            "clientInfo":{"name":"cccc","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"experimentalApi":true}
        }),
        Duration::from_secs(10),
    )?;
    let mut params = json!({
        "cwd":cwd,
        "approvalPolicy":"never",
        "sandbox":"danger-full-access",
        "personality":"pragmatic"
    });
    if !model.is_empty() {
        params["model"] = json!(model);
    }
    let result = session.request("thread/start", params, Duration::from_secs(20))?;
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
    *session.thread_id.lock().map_err(|_| poisoned())? = thread_id.to_owned();
    Ok(())
}

pub(super) fn submit_codex(session: &Arc<Session>, turn: &Turn) -> io::Result<String> {
    let thread_id = session.thread_id.lock().map_err(|_| poisoned())?.clone();
    let result = session.request(
        "turn/start",
        json!({"threadId":thread_id,"input":[{"type":"text","text":turn.text}]}),
        Duration::from_secs(30),
    )?;
    Ok(result
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(&turn.event_id)
        .to_owned())
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
