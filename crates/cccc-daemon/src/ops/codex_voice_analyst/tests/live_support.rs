use super::super::AnalystEvent;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::broadcast;

pub(super) struct ProbeDaemon {
    launcher: PathBuf,
    environment: BTreeMap<String, String>,
    home: PathBuf,
}

impl ProbeDaemon {
    pub(super) fn start(
        home: &cccc_core::HomeLayout,
        workdir: &Path,
        environment: &BTreeMap<String, String>,
    ) -> Self {
        let launcher = crate::ops::codex_mcp::resolve_cccc_executable().expect("CCCC launcher");
        let daemon = std::process::Command::new(&launcher)
            .args(["daemon", "start"])
            .envs(environment)
            .env("CCCC_HOME", home.root())
            .current_dir(workdir)
            .output()
            .expect("start isolated daemon");
        assert!(daemon.status.success(), "isolated daemon startup failed");
        Self {
            launcher,
            environment: environment.clone(),
            home: home.root().to_owned(),
        }
    }
}

impl Drop for ProbeDaemon {
    fn drop(&mut self) {
        let _ = std::process::Command::new(&self.launcher)
            .args(["daemon", "stop"])
            .envs(&self.environment)
            .env("CCCC_HOME", &self.home)
            .output();
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
    let mut recent = VecDeque::new();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = events.recv().await.expect("Analyst event");
            let params = &event.message["params"];
            let observed_turn = params["turn"]["id"]
                .as_str()
                .or_else(|| params["turnId"].as_str())
                .unwrap_or_default();
            if recent.len() == 12 {
                recent.pop_front();
            }
            // Keep identifiers and phases, never text, tool input or credentials.
            recent.push_back(format!(
                "{} turn={} status={}",
                event.message["method"].as_str().unwrap_or_default(),
                observed_turn,
                params["turn"]["status"].as_str().unwrap_or_default()
            ));
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
    .unwrap_or_else(|_| {
        panic!("Analyst terminal status timeout for {turn_id}; recent events: {recent:?}")
    })
}
