use super::super::AnalystEvent;
use cccc_contracts::{Actor, Event};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

const BUSY_DELIVERY_LIMIT: Duration = Duration::from_secs(6);
const LIVE_ACTOR_TIMEOUT: Duration = Duration::from_secs(180);

pub(super) async fn wait_for_daemon(client: &cccc_client::DaemonClient) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if client
            .call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: serde_json::Map::new(),
            })
            .await
            .is_ok_and(|response| response.ok)
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "daemon did not become ready"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) fn assert_managed_actor_starts_idle(
    store: &GroupStore,
    group: &GroupDoc,
    actor: &Actor,
) -> io::Result<()> {
    let event_path = store
        .state_dir(&group.group_id)?
        .join("headless/events.jsonl");
    wait_for(LIVE_ACTOR_TIMEOUT, "managed Actor idle startup", || {
        fail_if_managed_actor_stopped(&event_path)?;
        let events = headless_events(&event_path);
        if events.iter().any(|event| {
            event["actor_id"] == actor.id
                && matches!(
                    event["type"].as_str(),
                    Some("headless.turn.started" | "headless.control.started")
                )
        }) {
            return Err(io::Error::other(
                "managed Actor startup created model work before any real input",
            ));
        }
        Ok(
            super::super::super::local_headless::status(&group.group_id, &actor.id)
                .is_some_and(|status| status.status == "idle"),
        )
    })
}

pub(super) async fn wait_for_turn_text(
    events: &mut broadcast::Receiver<AnalystEvent>,
    turn_id: &str,
) -> String {
    let timeout = std::env::var("CCCC_VOICE_ANALYST_TURN_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(120));
    let trace = std::env::var("CCCC_VOICE_ANALYST_TRACE").as_deref() == Ok("1");
    tokio::time::timeout(timeout, async {
        let mut completed_text = String::new();
        let mut deltas = String::new();
        loop {
            let event = events.recv().await.expect("Analyst event");
            if trace {
                eprintln!("VOICE_ANALYST_EVENT {}", event.message);
            }
            let method = event.message["method"].as_str().unwrap_or_default();
            let params = &event.message["params"];
            assert_ne!(
                method, "mcpServer/elicitation/request",
                "YOLO Voice Analyst unexpectedly requested MCP approval: {}",
                event.message
            );
            if method == "item/agentMessage/delta"
                && params["turnId"] == turn_id
                && let Some(delta) = params["delta"].as_str()
            {
                deltas.push_str(delta);
            }
            if method == "item/completed"
                && params["turnId"] == turn_id
                && params["item"]["type"] == "agentMessage"
                && let Some(text) = params["item"]["text"].as_str()
            {
                completed_text = text.to_owned();
            }
            if method == "turn/completed" && params["turn"]["id"] == turn_id {
                return if completed_text.is_empty() {
                    deltas
                } else {
                    completed_text
                };
            }
        }
    })
    .await
    .expect("Analyst turn timeout")
}

pub(super) async fn wait_for_turn_status(
    events: &mut broadcast::Receiver<AnalystEvent>,
    turn_id: &str,
) -> String {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = events.recv().await.expect("Analyst event");
            if event.message["method"] == "turn/completed"
                && event.message["params"]["turn"]["id"] == turn_id
            {
                return event.message["params"]["turn"]["status"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
            }
        }
    })
    .await
    .expect("Analyst terminal status timeout")
}

pub(super) async fn wait_for_interruptible_activity(
    events: &mut broadcast::Receiver<AnalystEvent>,
    turn_id: &str,
) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = events.recv().await.expect("Analyst event");
            let method = event.message["method"].as_str().unwrap_or_default();
            let params = &event.message["params"];
            if method == "turn/completed" && params["turn"]["id"] == turn_id {
                panic!("cancel fixture completed before it became interruptible");
            }
            let matching_item =
                params["turnId"] == turn_id && params["item"]["type"] == "commandExecution";
            let matching_approval = event.message.get("id").is_some()
                && params["turnId"] == turn_id
                && matches!(
                    method,
                    "item/commandExecution/requestApproval" | "item/permissions/requestApproval"
                );
            if matching_item || matching_approval {
                return;
            }
        }
    })
    .await
    .expect("interruptible activity timeout")
}

pub(super) fn task_title_is(event: &Map<String, Value>, title: &str) -> bool {
    event.get("title").and_then(Value::as_str) == Some(title)
}

/// Exercise the user-visible Actor contract against a real managed Runtime:
/// CCCC hands a second message to the native TUI while the first turn is still
/// active, and the Runtime remains responsible for deciding queue versus steer.
pub(super) fn run_busy_actor_delivery_canary(
    home: &HomeLayout,
    store: &GroupStore,
    group: &GroupDoc,
    actor: &Actor,
    reply_text: &str,
) -> io::Result<Duration> {
    let event_path = store
        .state_dir(&group.group_id)?
        .join("headless/events.jsonl");
    wait_for(LIVE_ACTOR_TIMEOUT, "managed Actor idle state", || {
        fail_if_managed_actor_stopped(&event_path)?;
        Ok(
            super::super::super::local_headless::status(&group.group_id, &actor.id)
                .is_some_and(|status| status.status == "idle"),
        )
    })?;

    let hold_marker = format!("{reply_text}_BUSY_HOLD_DONE");
    let mut hold = Event::new("chat.message", &group.group_id);
    hold.by = "user".into();
    hold.data = serde_json::json!({
        "text":format!(
            "Run the shell command `sleep 12`, then finish this turn with exactly {hold_marker}. Do not use a CCCC message tool for this first turn."
        ),
        "to":[actor.id],
        "message_mode":"send",
    })
    .as_object()
    .cloned()
    .expect("hold event data");
    ledger::append(&store.ledger_path(&group.group_id)?, &hold)?;
    if !super::super::super::local_headless::submit(home, group, actor, &hold) {
        return Err(io::Error::other(
            "managed Actor rejected the busy-state setup delivery",
        ));
    }
    wait_for(LIVE_ACTOR_TIMEOUT, "managed Actor busy state", || {
        fail_if_managed_actor_stopped(&event_path)?;
        Ok(
            super::super::super::local_headless::status(&group.group_id, &actor.id)
                .is_some_and(|status| status.status == "working"),
        )
    })?;

    let mut source = Event::new("chat.message", &group.group_id);
    source.by = "user".into();
    source.data = serde_json::json!({
        "text":format!(
            "Reply with exactly {reply_text} using the required CCCC reply tool."
        ),
        "to":[actor.id],
        "message_mode":"request_reply",
    })
    .as_object()
    .cloned()
    .expect("busy delivery event data");
    ledger::append(&store.ledger_path(&group.group_id)?, &source)?;
    let started = Instant::now();
    if !super::super::super::local_headless::submit(home, group, actor, &source) {
        return Err(io::Error::other(
            "managed Actor rejected a delivery while its provider was busy",
        ));
    }
    let elapsed = started.elapsed();
    if elapsed > BUSY_DELIVERY_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "CCCC held a managed Actor delivery for {elapsed:?} while the provider was busy"
            ),
        ));
    }

    wait_for(LIVE_ACTOR_TIMEOUT, "busy-state Actor MCP reply", || {
        fail_if_managed_actor_stopped(&event_path)?;
        Ok(
            ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
                .unwrap_or_default()
                .iter()
                .any(|event| {
                    event.kind == "chat.message"
                        && event.by == actor.id
                        && event.data.get("reply_to").and_then(Value::as_str)
                            == Some(source.id.as_str())
                        && event.data.get("text").and_then(Value::as_str) == Some(reply_text)
                }),
        )
    })?;
    Ok(elapsed)
}

fn fail_if_managed_actor_stopped(path: &Path) -> io::Result<()> {
    let events = headless_events(path);
    if let Some(event) = events
        .iter()
        .rev()
        .find(|event| event["type"] == "headless.session.disconnected")
    {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            format!(
                "managed Actor disconnected: {}",
                event["data"]["reason"].as_str().unwrap_or("unknown reason")
            ),
        ));
    }
    if events
        .iter()
        .any(|event| event["type"] == "headless.session.stopped")
    {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "managed Actor stopped before the busy-state canary completed",
        ));
    }
    Ok(())
}

fn headless_events(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn wait_for(
    timeout: Duration,
    stage: &str,
    ready: impl Fn() -> io::Result<bool>,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready()? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("live managed Actor canary timed out waiting for {stage}"),
    ))
}
