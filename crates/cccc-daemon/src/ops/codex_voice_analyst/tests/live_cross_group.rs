use super::super::*;
use super::live_support::{task_title_is, wait_for_daemon, wait_for_turn_text};
use cccc_core::{GroupStore, HomeLayout, group_scope, scope};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

#[tokio::test]
async fn live_codex_uses_an_explicit_cross_group_target_when_enabled() {
    if std::env::var("CCCC_VOICE_ANALYST_MCP_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("root");
    let store = GroupStore::new(home.clone()).expect("store");
    let focus_group = store.create("live voice focus", "").expect("focus group");
    let focus_group = group_scope::attach(
        &store,
        &focus_group.group_id,
        scope::detect(&root).expect("scope"),
    )
    .expect("attach");
    let target_group = store
        .create("live voice handoff target", "")
        .expect("target group");
    store
        .mutate(&target_group.group_id, |group| {
            let mut worker = cccc_contracts::Actor::new("worker");
            worker.runtime = cccc_contracts::ActorRuntime::WebModel;
            cccc_core::actors::add(group, worker)?;
            Ok(())
        })
        .expect("worker actor");

    let daemon = tokio::spawn(crate::server::run(home.clone()));
    let client = cccc_client::DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;

    let mut config = LaunchConfig::new(&focus_group.group_id, &root);
    config.model =
        Some(std::env::var("CCCC_VOICE_ANALYST_MODEL").unwrap_or_else(|_| "gpt-5.6-sol".into()));
    config.codex_executable = std::env::var_os("CCCC_CODEX_EXECUTABLE").map(PathBuf::from);
    let session = AnalystSession::launch(&home, config)
        .await
        .expect("launch MCP Analyst");
    let generation = session.generation().to_owned();
    let mut events = session.subscribe();
    let delegation_prompt = format!(
        "Use the CCCC MCP tool cccc_capability_use exactly once to invoke cccc_tracked_send. Pass scope=session and tool_name=cccc_tracked_send. In tool_arguments explicitly pass group_id={} and deliberately pass by=intruder together with title=VOICE-HANDOFF-TOPAZ, text=VOICE_HANDOFF_TOPAZ, insight=Durable delegation is justified here because this fixture verifies explicit cross-Group routing and user attribution, outcome=Return ACTOR_RESULT_TOPAZ, to=worker, assignee=worker, and idempotency_key=voice-live-handoff-topaz. Use the explicit target Group rather than the Focus Group, while the host must preserve user authority. After the tool succeeds, report the returned task_id and event_id concisely. Do not modify files.",
        target_group.group_id
    );
    let delegated = session
        .start_turn(&generation, "live-tracked-send", &delegation_prompt)
        .await
        .expect("tracked send turn");
    let delegated_text = wait_for_turn_text(&mut events, &delegated.turn_id).await;
    assert!(
        delegated_text.contains("T00") || delegated_text.contains("task"),
        "Analyst did not report task evidence: {delegated_text}"
    );

    let context_store = cccc_core::context::ContextStore::new(home.clone()).expect("context store");
    let focus_context = context_store
        .load(&focus_group.group_id)
        .expect("focus context");
    assert!(
        focus_context
            .tasks
            .iter()
            .all(|task| !task_title_is(task, "VOICE-HANDOFF-TOPAZ")),
        "explicit target must not write into the Focus Group"
    );
    let context = context_store
        .load(&target_group.group_id)
        .expect("target context");
    let tasks = context
        .tasks
        .iter()
        .filter(|task| task_title_is(task, "VOICE-HANDOFF-TOPAZ"))
        .collect::<Vec<_>>();
    assert_eq!(tasks.len(), 1, "tracked handoff must create one task");
    assert_eq!(tasks[0]["assignee"], "worker");
    let ledger_path = store
        .ledger_path(&target_group.group_id)
        .expect("ledger path");
    let events_in_ledger = cccc_core::ledger::read_all(&ledger_path).expect("ledger");
    let source = events_in_ledger
        .iter()
        .find(|event| {
            event.kind == "chat.message"
                && event.by == "user"
                && event.data.get("text").and_then(Value::as_str) == Some("VOICE_HANDOFF_TOPAZ")
        })
        .expect("user-attributed cross-group tracked message");
    assert_eq!(
        events_in_ledger
            .iter()
            .filter(|event| {
                event.kind == "chat.message"
                    && event.data.get("text").and_then(Value::as_str) == Some("VOICE_HANDOFF_TOPAZ")
            })
            .count(),
        1,
        "idempotent handoff must append one source message"
    );

    let reply = client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: "reply".into(),
            args: json!({
                "group_id":target_group.group_id,
                "by":"worker",
                "reply_to":source.id,
                "text":"ACTOR_RESULT_TOPAZ",
                "to":["user"],
                "message_mode":"send"
            })
            .as_object()
            .cloned()
            .expect("reply args"),
        })
        .await
        .expect("fixture actor reply");
    assert!(reply.ok, "fixture actor reply failed: {:?}", reply.error);

    let result_prompt = format!(
        "Use cccc_message_history with explicit group_id={} to read the fixture worker's reply to the tracked handoff. Reply with only the exact result text from that actor.",
        target_group.group_id
    );
    let returned = session
        .start_turn(&generation, "live-actor-result", &result_prompt)
        .await
        .expect("result query turn");
    let returned_text = wait_for_turn_text(&mut events, &returned.turn_id).await;
    assert_eq!(returned_text.trim(), "ACTOR_RESULT_TOPAZ");

    session.stop(&generation).await.expect("stop MCP Analyst");
    let shutdown = client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: serde_json::Map::new(),
        })
        .await
        .expect("shutdown daemon");
    assert!(shutdown.ok, "daemon shutdown failed: {:?}", shutdown.error);
    tokio::time::timeout(Duration::from_secs(10), daemon)
        .await
        .expect("daemon exit timeout")
        .expect("daemon task")
        .expect("daemon exit");
}
