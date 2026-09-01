use super::super::*;
use super::live_helper::LiveVoiceHelper;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub(super) struct LiveReport {
    pub(super) delegated: HashSet<String>,
    pub(super) analyst_turn_ids: HashSet<String>,
    pub(super) provider_delegations: Vec<ProviderDelegation>,
    pub(super) analyst_result: String,
    pub(super) projection_sent: bool,
    pub(super) post_projection_audio_frames: u64,
    pub(super) provider_ack_events: Vec<Value>,
}

pub(super) async fn run_integrated_loop(
    call: &CodexVoiceCall,
    generation: &str,
    helper: &mut LiveVoiceHelper,
) -> LiveReport {
    let mut analyst_events = call.subscribe_analyst();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut report = LiveReport {
        delegated: HashSet::new(),
        analyst_turn_ids: HashSet::new(),
        provider_delegations: Vec::new(),
        analyst_result: String::new(),
        projection_sent: false,
        post_projection_audio_frames: 0,
        provider_ack_events: Vec::new(),
    };
    let mut active_turn: Option<TurnReceipt> = None;
    let mut agent_deltas = String::new();
    let mut completed_text = String::new();
    let mut helper_states = vec!["ready".to_owned()];
    let mut provider_event_types = Vec::new();
    let mut provider_trace = Vec::new();
    let mut analyst_event_types = Vec::new();
    let mut analyst_trace = Vec::new();
    let mut projection_trace = Vec::new();
    let mut total_audio_frames = 0_u64;
    let live_started_at = Instant::now();
    let deadline = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => panic!(
                "integrated Codex Voice loop timed out: states={helper_states:?} provider_events={provider_event_types:?} provider_trace={provider_trace:?} provider_acks={:?} analyst_events={analyst_event_types:?} analyst_trace={analyst_trace:?} projections={projection_trace:?} delegations={} analyst_turns={} active_turn={} analyst_result={} projection_sent={} audio_frames={total_audio_frames} post_projection_audio_frames={}",
                report.provider_ack_events,
                report.delegated.len(), report.analyst_turn_ids.len(), active_turn.is_some(),
                !report.analyst_result.is_empty(), report.projection_sent,
                report.post_projection_audio_frames,
            ),
            _ = heartbeat.tick() => call.heartbeat(generation).expect("call heartbeat"),
            event = helper.next_event() => {
                let event = event.expect("voice helper event");
                match event["type"].as_str().unwrap_or_default() {
                    "error" => panic!("voice helper failed: {}", event["message"]),
                    "state" => {
                        if let Some(state) = event["state"].as_str()
                            && helper_states.last().map(String::as_str) != Some(state)
                        {
                            helper_states.push(state.to_owned());
                        }
                    }
                    "pcm" => {
                        total_audio_frames += 1;
                        if report.projection_sent {
                            report.post_projection_audio_frames += 1;
                        }
                    }
                    "data" => {
                        let message = &event["message"];
                        if let Some(kind) = message["type"].as_str()
                            && !provider_event_types.iter().any(|known| known == kind)
                        {
                            provider_event_types.push(kind.to_owned());
                        }
                        if matches!(
                            message["type"].as_str(),
                            Some("input_transcript.added" | "turn.created" | "delegation.created" | "turn.done")
                        ) {
                            provider_trace.push(format!(
                                "{}ms {}", live_started_at.elapsed().as_millis(),
                                serde_json::to_string(message)
                                    .expect("provider trace JSON")
                                    .chars().take(1_500).collect::<String>()
                            ));
                        }
                        if matches!(
                            message["type"].as_str(),
                            Some("delegation.context.appended" | "session.context.appended")
                        ) {
                            report.provider_ack_events.push(message.clone());
                        }
                        if let Some(provider) = parse_provider_delegation(message)
                            .expect("provider event")
                        {
                            if !report.provider_delegations
                                .iter().any(|known| known.id == provider.id)
                            {
                                report.provider_delegations.push(provider.clone());
                            }
                            let receipt = call
                                .begin_delegation(generation, &provider)
                                .await
                                .expect("begin provider delegation");
                            report.delegated.insert(receipt.delegation_id.clone());
                            report.analyst_turn_ids.insert(receipt.turn_id.clone());
                            if let Some(active) = &active_turn {
                                assert_eq!(
                                    receipt.turn_id, active.turn_id,
                                    "later provider delegation started parallel Analyst work"
                                );
                            } else {
                                active_turn = Some(receipt);
                            }
                        }
                    }
                    _ => {}
                }
            }
            event = analyst_events.recv(), if active_turn.is_some() => {
                let event = event.expect("Analyst event");
                let turn = active_turn.as_ref().expect("active turn");
                let method = event.message["method"].as_str().unwrap_or_default();
                if !analyst_event_types.iter().any(|known| known == method) {
                    analyst_event_types.push(method.to_owned());
                }
                if matches!(method, "item/agentMessage/delta" | "item/completed" | "turn/completed") {
                    analyst_trace.push(format!(
                        "{}ms {}", live_started_at.elapsed().as_millis(),
                        serde_json::to_string(&event.message)
                            .expect("Analyst trace JSON")
                            .chars().take(1_500).collect::<String>()
                    ));
                }
                if method == "mcpServer/elicitation/request" {
                    panic!(
                        "YOLO Voice Analyst unexpectedly requested MCP approval: {}",
                        event.message
                    );
                }
                let params = &event.message["params"];
                if method == "item/agentMessage/delta"
                    && params["turnId"] == turn.turn_id
                    && let Some(delta) = params["delta"].as_str()
                {
                    agent_deltas.push_str(delta);
                    for command in call
                        .project_analyst_delta(generation, &turn.turn_id, delta)
                        .await
                        .expect("project Analyst progress")
                    {
                        projection_trace.push(format!(
                            "{}ms {}", live_started_at.elapsed().as_millis(), command
                        ));
                        helper
                            .send(json!({"type":"send_data","message":command}))
                            .await
                            .expect("send progress context");
                        report.projection_sent = true;
                    }
                }
                if method == "item/completed"
                    && params["turnId"] == turn.turn_id
                    && params["item"]["type"] == "agentMessage"
                    && let Some(text) = params["item"]["text"].as_str()
                {
                    completed_text = text.to_owned();
                }
                if method == "turn/completed" && params["turn"]["id"] == turn.turn_id {
                    assert_eq!(params["turn"]["status"], "completed");
                    report.analyst_result = if completed_text.is_empty() {
                        agent_deltas.trim().to_owned()
                    } else {
                        completed_text.trim().to_owned()
                    };
                    let projection = call
                        .take_final_projection(
                            generation, &turn.delegation_id, &turn.turn_id, &report.analyst_result,
                        )
                        .await
                        .expect("final projection")
                        .expect("first final projection");
                    for command in projection.commands {
                        projection_trace.push(format!(
                            "{}ms {}", live_started_at.elapsed().as_millis(), command
                        ));
                        helper
                            .send(json!({"type":"send_data","message":command}))
                            .await
                            .expect("send final context");
                        report.projection_sent = true;
                    }
                    active_turn = None;
                }
            }
        }
        if !report.analyst_result.is_empty() && report.post_projection_audio_frames > 0 {
            return report;
        }
    }
}
