use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn reme_write_preserves_metadata_shadow_and_dedup_semantics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = create_group(&home, "reme parity");
    let payload = json!({
        "group_id":group_id,
        "target":"memory",
        "content":"Keep the memory-lane coverage bridge deterministic.",
        "actor_id":"peer1",
        "source_refs":["message:m1"],
        "tags":["memory-lane","contract"],
        "supersedes":["MEMORY.md#L2"],
        "idempotency_key":"reme-parity-1",
        "dedup_intent":"new"
    });
    let first = call(&home, "memory_reme_write", payload.clone());
    assert_eq!(first.result["status"], "written");
    assert!(first.result["shadow_daily"].is_object());

    let layout = cccc_core::memory::MemoryStore::new(home.clone())
        .layout(&group_id, None)
        .expect("layout");
    let memory = std::fs::read_to_string(&layout.memory_file).expect("memory");
    let daily = std::fs::read_to_string(&layout.today_file).expect("daily");
    for value in [
        "Keep the memory-lane coverage bridge deterministic.",
        "message:m1",
        "memory-lane",
        "MEMORY.md#L2",
    ] {
        assert!(memory.contains(value), "missing {value} in memory");
    }
    assert_eq!(memory.matches("Keep the memory-lane").count(), 1);
    assert_eq!(daily.matches("Keep the memory-lane").count(), 1);

    let mut changed = payload;
    changed["content"] = Value::String("Changed payload with the same key.".into());
    let idempotent = call(&home, "memory_reme_write", changed);
    assert_eq!(idempotent.result["status"], "silent");
    assert_eq!(idempotent.result["reason"], "persistence_idempotency_key");
    assert_eq!(idempotent.result["dedup"]["precheck_decision"], "new");
    assert_eq!(idempotent.result["dedup"]["final_decision"], "silent");
}

#[test]
fn reme_context_daily_signal_and_search_controls_match_python_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = create_group(&home, "reme flow");
    let messages = vec![
        json!({"role":"user","name":"alice","content":"old ".repeat(800)}),
        json!({"role":"assistant","content":"answer ".repeat(500)}),
        json!({"role":"user","name":"bob","content":"new question ".repeat(300)}),
        json!({"role":"assistant","content":"partial ".repeat(100)}),
        json!({"role":"tool","content":"tool output ".repeat(100)}),
    ];
    let checked = call(
        &home,
        "memory_reme_context_check",
        json!({
            "group_id":group_id,
            "messages":messages,
            "context_window_tokens":3000,
            "reserve_tokens":200,
            "keep_recent_tokens":500
        }),
    );
    assert_eq!(checked.result["needs_compaction"], true);
    assert_eq!(checked.result["is_split_turn"], true);
    assert_eq!(checked.result["turn_prefix_messages"][0]["name"], "bob");

    let compact = call(
        &home,
        "memory_reme_compact",
        json!({
            "group_id":group_id,
            "messages_to_summarize":checked.result["messages_to_summarize"],
            "turn_prefix_messages":checked.result["turn_prefix_messages"],
            "language":"zh",
            "return_prompt":true
        }),
    );
    assert!(
        compact.result["prompt"]["system"]
            .as_str()
            .is_some_and(|value| value.contains("Output language: zh"))
    );
    assert!(
        compact.result["prompt"]["turn_prefix_user"]
            .as_str()
            .is_some_and(|value| value.contains("USER(bob)"))
    );
    let invalid_prefix = raw_call(
        &home,
        "memory_reme_compact",
        json!({
            "group_id":group_id,
            "messages_to_summarize":[],
            "turn_prefix_messages":{}
        }),
    );
    assert!(!invalid_prefix.ok);
    let invalid_window = raw_call(
        &home,
        "memory_reme_context_check",
        json!({"group_id":group_id,"messages":[],"context_window_tokens":100}),
    );
    assert!(!invalid_window.ok);
    let invalid_prompt_budget = raw_call(
        &home,
        "memory_reme_daily_flush",
        json!({
            "group_id":group_id,
            "messages":[],
            "return_prompt":true,
            "signal_pack_token_budget":1
        }),
    );
    assert!(!invalid_prompt_budget.ok);

    let active = (0..20)
        .map(|index| format!("active-{index} {}", "D".repeat(120)))
        .collect::<Vec<_>>();
    let flushed = call(
        &home,
        "memory_reme_daily_flush",
        json!({
            "group_id":group_id,
            "messages":[{"role":"user","content":"Need memory compaction summary."}],
            "dedup_intent":"new",
            "signal_pack_token_budget":64,
            "signal_pack":{
                "coordination_brief":{"objective":"A".repeat(2000),"current_focus":"B".repeat(1200)},
                "tasks":{"active":active},
                "agent_states":[{"id":"peer1","hot":{"focus":"G".repeat(400)}}]
            }
        }),
    );
    assert_eq!(flushed.result["status"], "written");
    assert_eq!(flushed.result["signal_pack"]["schema"], "v1");
    assert!(
        flushed.result["signal_pack"]["token_estimate"]
            .as_u64()
            .unwrap_or(u64::MAX)
            <= 64
    );
    let duplicate = call(
        &home,
        "memory_reme_daily_flush",
        json!({
            "group_id":group_id,
            "messages":[{"role":"user","content":"Need memory compaction summary."}],
            "dedup_intent":"silent",
            "dedup_query":"Need memory compaction summary."
        }),
    );
    assert_eq!(duplicate.result["status"], "silent");
    assert_eq!(duplicate.result["reason"], "precheck_silent");
    assert_eq!(duplicate.result["signal_pack"]["token_budget"], 320);

    let no_memory_source = call(
        &home,
        "memory_reme_search",
        json!({"group_id":group_id,"query":"memory","sources":["sessions"]}),
    );
    assert_eq!(no_memory_source.result["count"], 0);
    let unknown_source_fallback = call(
        &home,
        "memory_reme_search",
        json!({"group_id":group_id,"query":"memory","sources":["unknown"]}),
    );
    assert!(
        unknown_source_fallback.result["count"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    let invalid = raw_call(
        &home,
        "memory_reme_search",
        json!({"group_id":group_id,"query":"memory","vector_weight":1.5}),
    );
    assert!(!invalid.ok);
    assert_eq!(invalid.error.expect("error").code, "invalid_args");
}

fn create_group(home: &HomeLayout, title: &str) -> String {
    call(home, "group_create", json!({"title":title})).result["group_id"]
        .as_str()
        .expect("group id")
        .to_owned()
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn raw_call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
