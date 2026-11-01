//! Persist only lifecycle facts that can be associated with a Chet input.
use anyhow::{Context, Result, bail};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_daemon::experimental_codex_voice::{AnalystCompletionSource, AnalystLifecycleEvent};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LivePhase {
    Queued,
    Working,
}

pub(super) enum HistoryCommand {
    Lifecycle(AnalystLifecycleEvent),
    Voice {
        key: String,
        delegation: cccc_core::chet_history::VoiceDelegation,
    },
}

pub(super) fn voice_command(
    provider: cccc_contracts::codex_voice::VoiceProvider,
    call_id: &str,
    delegation: &cccc_daemon::experimental_codex_voice::ProviderDelegation,
) -> HistoryCommand {
    // Keep this key aligned with CodexVoiceCall::route_provider_event: Grok
    // namespaces its provider ID by call, while Codex currently uses it raw.
    let key = if provider == cccc_contracts::codex_voice::VoiceProvider::Grok {
        format!("grok:{call_id}:{}", delegation.id)
    } else {
        delegation.id.clone()
    };
    HistoryCommand::Voice {
        key,
        delegation: cccc_core::chet_history::VoiceDelegation {
            call_id: call_id.to_owned(),
            delegation_id: delegation.id.clone(),
            text: delegation.text.clone(),
        },
    }
}

pub(super) fn clear_live(runtime: &super::AnalystRuntime) -> bool {
    let mut voice = runtime
        .chet_voice
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut live = runtime
        .chet_live
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let had_live = !live.is_empty();
    live.clear();
    voice.clear();
    had_live
}

pub(super) fn update_live(runtime: &super::AnalystRuntime, event: &AnalystLifecycleEvent) {
    let mut voice = runtime
        .chet_voice
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut live = runtime
        .chet_live
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    project_live(&mut live, event, &voice);
    match event {
        AnalystLifecycleEvent::Completed { delegation_ids, .. } => {
            for id in delegation_ids {
                voice.remove(id);
            }
        }
        AnalystLifecycleEvent::Disconnected | AnalystLifecycleEvent::NeedsAttention { .. } => {
            voice.clear()
        }
        _ => {}
    }
}

fn project_live(
    live: &mut std::collections::HashMap<Uuid, LivePhase>,
    event: &AnalystLifecycleEvent,
    voice: &HashMap<String, Uuid>,
) {
    match event {
        AnalystLifecycleEvent::Queued { delegation_id, .. } => {
            if let Some(id) = resolve_input_id(delegation_id, voice) {
                live.insert(id, LivePhase::Queued);
            }
        }
        AnalystLifecycleEvent::Started { receipt, .. }
        | AnalystLifecycleEvent::Associated { receipt, .. } => {
            if let Some(id) = resolve_input_id(&receipt.delegation_id, voice) {
                live.insert(id, LivePhase::Working);
            }
        }
        AnalystLifecycleEvent::Completed { delegation_ids, .. } => {
            for id in delegation_ids
                .iter()
                .filter_map(|id| resolve_input_id(id, voice))
            {
                live.remove(&id);
            }
        }
        AnalystLifecycleEvent::Disconnected | AnalystLifecycleEvent::NeedsAttention { .. } => {
            live.clear();
        }
        AnalystLifecycleEvent::Progress { .. } => {}
    }
}

fn input_id(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value.strip_prefix("chet-input:")?).ok()
}

fn resolve_input_id(key: &str, voice: &HashMap<String, Uuid>) -> Option<Uuid> {
    match (input_id(key), voice.get(key).copied()) {
        // Ambiguous identities must not be attributed to either input.
        (Some(_), Some(_)) => None,
        (Some(id), None) | (None, Some(id)) => Some(id),
        (None, None) => None,
    }
}

pub(super) fn in_flight_status(
    live: &HashMap<Uuid, LivePhase>,
    voice: &HashMap<String, Uuid>,
    pending: &[String],
    active: &[String],
) -> (Vec<Uuid>, Vec<Uuid>) {
    let mut queued: Vec<_> = live
        .iter()
        .filter_map(|(id, phase)| (*phase == LivePhase::Queued).then_some(*id))
        .chain(
            pending
                .iter()
                .filter_map(|key| resolve_input_id(key, voice)),
        )
        .collect();
    let mut working: Vec<_> = live
        .iter()
        .filter_map(|(id, phase)| (*phase == LivePhase::Working).then_some(*id))
        .chain(active.iter().filter_map(|key| resolve_input_id(key, voice)))
        .collect();
    queued.sort_unstable();
    queued.dedup();
    working.sort_unstable();
    working.dedup();
    queued.retain(|id| working.binary_search(id).is_err());
    (queued, working)
}

fn event_id(kind: &str, generation: &str, turn_id: &str, input: Option<Uuid>) -> Uuid {
    let mut digest = Sha256::new();
    for part in [kind, generation, turn_id] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    if let Some(input) = input {
        digest.update(input.as_bytes());
    }
    let digest = digest.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

pub(super) fn voice_input_id(generation: &str, key: &str) -> Uuid {
    event_id("voice-input", generation, key, None)
}

fn operations(
    generation: &str,
    event: &AnalystLifecycleEvent,
    voice: &HashMap<String, Uuid>,
) -> Vec<(&'static str, Value)> {
    let resolve = |key: &str| resolve_input_id(key, voice);
    match event {
        AnalystLifecycleEvent::Started { receipt, .. }
        | AnalystLifecycleEvent::Associated { receipt, .. } => {
            let Some(input) = resolve(&receipt.delegation_id) else {
                return Vec::new();
            };
            vec![(
                "chet_history_receipt",
                json!({
                    "by":"user",
                    "id":event_id("receipt", generation, &receipt.turn_id, Some(input)),
                    "input_id":input,
                    "receipt":{"generation":generation,"turn_id":receipt.turn_id}
                }),
            )]
        }
        AnalystLifecycleEvent::Completed {
            source,
            turn_id,
            delegation_ids,
            status,
            error,
            result,
            ..
        } => {
            if *source == AnalystCompletionSource::HostUnconfirmed {
                return Vec::new();
            }
            let mut seen = HashSet::new();
            delegation_ids
                .iter()
                .filter_map(|key| resolve(key))
                .filter(|id| seen.insert(*id))
                .map(|input| {
                    if *source == AnalystCompletionSource::HostNotStarted {
                        (
                            "chet_history_not_started",
                            json!({"by":"user",
                            "id":event_id("cancelled", generation, turn_id, Some(input)),
                            "input_id":input,"reason":"cancelled"}),
                        )
                    } else {
                        (
                            "chet_history_result",
                            json!({
                                "by":"user",
                                "id":event_id("result", generation, turn_id, Some(input)),
                                "input_ids":[input],
                                "outcome":{"generation":generation,"turn_id":turn_id,
                                           "status":status,"text":result,"error":error}
                            }),
                        )
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

pub(super) fn is_history_event(event: &AnalystLifecycleEvent) -> bool {
    matches!(
        event,
        AnalystLifecycleEvent::Started { .. }
            | AnalystLifecycleEvent::Associated { .. }
            | AnalystLifecycleEvent::Completed { .. }
    )
}

pub(super) async fn persist_event(
    home: &HomeLayout,
    generation: &str,
    event: &AnalystLifecycleEvent,
    voice: &HashMap<String, Uuid>,
) -> Result<()> {
    let client = cccc_client::DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(5));
    let mut failed = false;
    for (op, args) in operations(generation, event, voice) {
        if !client
            .call(&DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("fixed object").clone(),
            })
            .await
            .is_ok_and(|response| response.ok)
        {
            failed = true;
        }
    }
    if failed {
        bail!("daemon did not confirm the Chet history write");
    }
    Ok(())
}

pub(super) async fn persist_voice(
    home: &HomeLayout,
    generation: &str,
    key: &str,
    delegation: &cccc_core::chet_history::VoiceDelegation,
    voice: &mut HashMap<String, Uuid>,
) -> Result<()> {
    let id = voice_input_id(generation, key);
    voice.insert(key.to_owned(), id);
    if input_id(key).is_some() {
        bail!("Voice delegation ID collides with a Chet text input ID");
    }
    let client = cccc_client::DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(5));
    let args = json!({"by":"user","id":id,"delegation":delegation});
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: "chet_history_voice_delegation".into(),
            args: args.as_object().expect("fixed object").clone(),
        })
        .await
        .context("persist observed Voice request")?;
    if !response.ok {
        bail!("daemon did not confirm the Voice request");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_daemon::experimental_codex_voice::AnalystTurnOrigin;

    #[test]
    fn lifecycle_status_merges_starting_and_native_work_by_correlated_input() {
        let text = Uuid::new_v4();
        let voice_id = Uuid::new_v4();
        let active = Uuid::new_v4();
        let voice = HashMap::from([("voice-key".to_owned(), voice_id)]);
        let live = HashMap::from([(active, LivePhase::Working), (text, LivePhase::Queued)]);
        let pending = vec![
            format!("chet-input:{text}"),
            "voice-key".into(),
            "unrelated".into(),
        ];
        let active_ids = vec![format!("chet-input:{active}"), format!("chet-input:{text}")];
        let (queued, working) = in_flight_status(&live, &voice, &pending, &active_ids);
        assert_eq!(queued, vec![voice_id]);
        assert_eq!(working.len(), 2);
        assert!(working.contains(&text) && working.contains(&active));
    }

    #[test]
    fn projects_only_correlated_inputs_and_keeps_receipt_distinct_from_result() {
        let id = Uuid::new_v4();
        let turn = cccc_daemon::experimental_codex_voice::TurnReceipt {
            delegation_id: format!("chet-input:{id}"),
            thread_id: "thread".into(),
            turn_id: "turn".into(),
        };
        let started = AnalystLifecycleEvent::Started {
            receipt: turn,
            origin: AnalystTurnOrigin::Terminal,
        };
        let voice = HashMap::new();
        let (op, receipt) = operations("generation", &started, &voice)
            .pop()
            .expect("receipt");
        assert_eq!(op, "chet_history_receipt");
        assert_eq!(receipt["input_id"], id.to_string());
        assert_eq!(receipt["receipt"]["turn_id"], "turn");
        let done = AnalystLifecycleEvent::Completed {
            source: AnalystCompletionSource::Runtime,
            turn_id: "turn".into(),
            delegation_id: format!("chet-input:{id}"),
            delegation_ids: vec!["not-a-chet-input".into(), format!("chet-input:{id}")],
            status: "completed".into(),
            error: String::new(),
            result: "answer".into(),
            speakable: false,
        };
        let (op, result) = operations("generation", &done, &voice)
            .pop()
            .expect("result");
        assert_eq!(op, "chet_history_result");
        assert_eq!(result["input_ids"], json!([id]));
        assert_eq!(result["outcome"]["text"], "answer");
        assert_eq!(
            operations("generation", &done, &voice),
            operations("generation", &done, &voice)
        );
    }

    #[test]
    fn mixed_text_and_voice_turn_has_one_result_per_correlated_input() {
        let text = Uuid::new_v4();
        let voice_id = Uuid::new_v4();
        let voice = HashMap::from([("voice-raw".to_owned(), voice_id)]);
        let done = AnalystLifecycleEvent::Completed {
            source: AnalystCompletionSource::Runtime,
            turn_id: "turn".into(),
            delegation_id: "voice-raw".into(),
            delegation_ids: vec!["voice-raw".into(), format!("chet-input:{text}")],
            status: "completed".into(),
            error: String::new(),
            result: "answer".into(),
            speakable: true,
        };
        let records = operations("generation", &done, &voice);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].1["input_ids"], json!([voice_id]));
        assert_eq!(records[1].1["input_ids"], json!([text]));
        assert_ne!(records[0].1["id"], records[1].1["id"]);
    }

    #[test]
    fn host_delivery_failure_does_not_become_a_runtime_result() {
        let input = Uuid::new_v4();
        let event = |source| AnalystLifecycleEvent::Completed {
            source,
            turn_id: "host".into(),
            delegation_id: format!("chet-input:{input}"),
            delegation_ids: vec![format!("chet-input:{input}")],
            status: "failed".into(),
            error: "no receipt".into(),
            result: String::new(),
            speakable: false,
        };
        let voice = HashMap::new();
        assert!(
            operations(
                "generation",
                &event(AnalystCompletionSource::HostUnconfirmed),
                &voice
            )
            .is_empty()
        );
        let records = operations(
            "generation",
            &event(AnalystCompletionSource::HostNotStarted),
            &voice,
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, "chet_history_not_started");
        assert_eq!(records[0].1["reason"], "cancelled");
    }

    #[test]
    fn live_status_tracks_this_input_not_an_unrelated_busy_turn() {
        let id = Uuid::new_v4();
        let delegation = format!("chet-input:{id}");
        let mut live = std::collections::HashMap::new();
        let voice = HashMap::new();
        project_live(
            &mut live,
            &AnalystLifecycleEvent::Queued {
                delegation_id: delegation.clone(),
                position: 1,
            },
            &voice,
        );
        assert_eq!(live.get(&id), Some(&LivePhase::Queued));
        project_live(
            &mut live,
            &AnalystLifecycleEvent::Started {
                receipt: cccc_daemon::experimental_codex_voice::TurnReceipt {
                    delegation_id: delegation.clone(),
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                },
                origin: AnalystTurnOrigin::Terminal,
            },
            &voice,
        );
        assert_eq!(live.get(&id), Some(&LivePhase::Working));
        project_live(
            &mut live,
            &AnalystLifecycleEvent::Completed {
                source: AnalystCompletionSource::Runtime,
                turn_id: "other".into(),
                delegation_id: "other".into(),
                delegation_ids: vec!["other".into()],
                status: "completed".into(),
                error: String::new(),
                result: String::new(),
                speakable: false,
            },
            &voice,
        );
        assert_eq!(live.get(&id), Some(&LivePhase::Working));
        project_live(&mut live, &AnalystLifecycleEvent::Disconnected, &voice);
        assert!(!live.contains_key(&id));
    }

    #[test]
    fn voice_status_uses_the_same_history_id() {
        let key = "grok:call:provider-id".to_owned();
        let id = voice_input_id("generation", &key);
        let voice = HashMap::from([(key.clone(), id)]);
        let mut live = HashMap::new();
        project_live(
            &mut live,
            &AnalystLifecycleEvent::Queued {
                delegation_id: key.clone(),
                position: 1,
            },
            &voice,
        );
        assert_eq!(live.get(&id), Some(&LivePhase::Queued));
        project_live(
            &mut live,
            &AnalystLifecycleEvent::Started {
                receipt: cccc_daemon::experimental_codex_voice::TurnReceipt {
                    delegation_id: key.clone(),
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                },
                origin: AnalystTurnOrigin::Voice,
            },
            &voice,
        );
        assert_eq!(live.get(&id), Some(&LivePhase::Working));
        project_live(
            &mut live,
            &AnalystLifecycleEvent::Completed {
                source: AnalystCompletionSource::Runtime,
                turn_id: "turn".into(),
                delegation_id: key.clone(),
                delegation_ids: vec![key],
                status: "completed".into(),
                error: String::new(),
                result: "answer".into(),
                speakable: true,
            },
            &voice,
        );
        assert!(!live.contains_key(&id));
    }
}
