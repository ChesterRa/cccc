use super::{Session, managed_runtime, output};
use serde_json::Value;
use std::io;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

pub(super) fn spawn(
    session: Arc<Session>,
    mut events: tokio::sync::broadcast::Receiver<super::super::codex_voice_analyst::AnalystEvent>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!(
            "cccc-managed-agent-out:{}:{}",
            session.group_id, session.actor_id
        ))
        .spawn(move || loop {
            match managed_runtime().block_on(events.recv()) {
                Ok(event) => {
                    let disconnected = event.message.get("method").and_then(Value::as_str)
                        == Some(
                            super::super::codex_voice_analyst::MANAGED_AGENT_DISCONNECTED_METHOD,
                        );
                    if disconnected {
                        let reason = event
                            .message
                            .pointer("/params/reason")
                            .and_then(Value::as_str)
                            .unwrap_or("managed Agent disconnected");
                        output::emit(
                            &session,
                            "headless.session.disconnected",
                            serde_json::Map::from_iter([(
                                "reason".into(),
                                Value::String(reason.to_owned()),
                            )]),
                        );
                    }
                    output::handle_message(&session, event.message);
                    if disconnected {
                        stop_after_provider_exit(&session);
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        group_id = %session.group_id,
                        actor_id = %session.actor_id,
                        "managed Actor event reader fell behind; stopping the unreplayable session"
                    );
                    stop_after_provider_exit(&session);
                    break;
                }
                Err(RecvError::Closed) => {
                    stop_after_provider_exit(&session);
                    break;
                }
            }
        })?;
    Ok(())
}

fn stop_after_provider_exit(session: &Session) {
    if !session.stop_after_process_exit() {
        return;
    }
    if let Err(error) = super::super::actor_runtime::record_process_exit(
        &session.home,
        &session.group_id,
        &session.actor_id,
        None,
    ) {
        tracing::warn!(
            ?error,
            group_id = %session.group_id,
            actor_id = %session.actor_id,
            "failed to record managed Actor provider exit"
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::{HeadlessStatus, Session, SessionTransport, turn_channel};
    use super::spawn;
    use crate::ops::codex_voice_analyst::{AnalystEvent, MANAGED_AGENT_DISCONNECTED_METHOD};
    use cccc_contracts::{Actor, ActorRuntime, utc_now};
    use cccc_core::{GroupStore, HomeLayout, ledger};
    use serde_json::json;
    use std::collections::HashMap;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};
    use tokio::sync::broadcast;

    #[test]
    fn provider_disconnect_records_one_system_actor_stop() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("managed exit", "").expect("group");
        store
            .mutate(&group.group_id, |document| {
                let mut actor = Actor::new("opencode-1");
                actor.runtime = ActorRuntime::Opencode;
                document.actors.push(actor);
                Ok(())
            })
            .expect("actor");

        let mut child = Command::new("sh")
            .args(["-c", "sleep 30"])
            .stdin(Stdio::piped())
            .spawn()
            .expect("child");
        let stdin = child.stdin.take().expect("stdin");
        let (turns, _turn_receiver) = turn_channel();
        let session = Arc::new(Session {
            home: home.clone(),
            group_id: group.group_id.clone(),
            actor_id: "opencode-1".into(),
            runtime: ActorRuntime::Opencode,
            transport: SessionTransport::Process {
                child: Mutex::new(child),
                stdin: Mutex::new(stdin),
            },
            status: Mutex::new(HeadlessStatus {
                status: "idle".into(),
                task_id: None,
                updated_at: utc_now(),
                pid: None,
            }),
            stopped: AtomicBool::new(false),
            next_request_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            thread_id: Mutex::new(String::new()),
            resumed_provider_session_id: Mutex::new(String::new()),
            startup_prompt: Mutex::new(None),
            active_turn: Mutex::new(None),
            completion: (Mutex::new(0), Condvar::new()),
            turns,
        });
        let (events, receiver) = broadcast::channel(4);
        spawn(Arc::clone(&session), receiver).expect("reader");
        events
            .send(AnalystEvent {
                generation: "generation-1".into(),
                message: json!({
                    "method":MANAGED_AGENT_DISCONNECTED_METHOD,
                    "params":{"reason":"ACP stdout reader closed"}
                }),
                requested_delegation_id: None,
            })
            .expect("disconnect");

        let path = store.ledger_path(&group.group_id).expect("ledger path");
        let deadline = Instant::now() + Duration::from_secs(2);
        let events = loop {
            let current = ledger::read_all(&path).expect("ledger");
            if current.iter().any(|event| event.kind == "actor.stop") {
                break current;
            }
            assert!(Instant::now() < deadline, "actor.stop was not recorded");
            std::thread::sleep(Duration::from_millis(10));
        };
        let stopped = events
            .iter()
            .filter(|event| event.kind == "actor.stop")
            .collect::<Vec<_>>();
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].by, "system");
        assert_eq!(stopped[0].data["actor_id"], "opencode-1");
        assert_eq!(stopped[0].data["reason"], "process_exit");
    }
}
