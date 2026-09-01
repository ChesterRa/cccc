use super::super::AnalystEvent;
use serde_json::{Map, Value};
use std::time::Duration;
use tokio::sync::broadcast;

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
