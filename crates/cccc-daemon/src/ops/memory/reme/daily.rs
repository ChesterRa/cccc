use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::memory::MemoryStore;
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

use super::common::{
    EntryInput, append_entry, build_entry, dedup_intent, dedup_precheck, digest,
    normalize_messages, truncate,
};
use super::context::compact_payload;

const DEFAULT_SIGNAL_PACK_TOKEN_BUDGET: u64 = 320;

pub(super) fn daily_flush(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let messages = request
        .args
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "messages must be an array"))?;
    let messages = messages
        .iter()
        .filter(|message| message.is_object())
        .cloned()
        .collect::<Vec<_>>();
    let language = string_arg(request, "language").unwrap_or_else(|| "en".into());
    let budget = match request.args.get("signal_pack_token_budget") {
        None => DEFAULT_SIGNAL_PACK_TOKEN_BUDGET,
        Some(value) => value.as_u64().ok_or_else(|| {
            OpError::new("invalid_args", "signal_pack_token_budget must be integer")
        })?,
    };
    if !(64..=4_096).contains(&budget) {
        return Err(OpError::new(
            "invalid_args",
            "signal_pack_token_budget must be in [64, 4096]",
        ));
    }
    if request
        .args
        .get("return_prompt")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return object(compact_payload(&messages, &[], "", &language, true));
    }
    let (signal_pack, signal_meta) = normalize_signal_pack(
        request.args.get("signal_pack").and_then(Value::as_object),
        budget as usize,
    );
    let date = string_arg(request, "date")
        .filter(|date| !date.trim().is_empty())
        .unwrap_or_else(today);
    let layout = MemoryStore::new(home.clone())
        .layout(&group_id, Some(&date))
        .map_err(OpError::io)?;
    let summary = summarize_daily(&messages, signal_pack.as_ref());
    let intent = dedup_intent(request.args.get("dedup_intent"));
    if summary.is_empty() {
        return object(json!({
            "status":"silent",
            "reason":"empty_summary",
            "target_file":layout.today_file,
            "content_hash":"",
            "bytes_written":0,
            "signal_pack":signal_meta,
            "dedup":{
                "intent":intent,"query":"","candidate_count":0,"top_score":0.0,"hits":[],
                "precheck_decision":"silent","final_decision":"silent",
                "final_reason":"empty_summary","decision":"silent"
            },
        }));
    }
    let query = string_arg(request, "dedup_query")
        .filter(|query| !query.trim().is_empty())
        .unwrap_or_else(|| summary.clone());
    let dedup = dedup_precheck(home, &group_id, &query, intent);
    if dedup.precheck_is_silent() {
        return object(json!({
            "status":"silent",
            "reason":"precheck_silent",
            "target_file":layout.today_file,
            "content_hash":"",
            "bytes_written":0,
            "signal_pack":signal_meta,
            "dedup":dedup.finalize("silent", "precheck_silent"),
        }));
    }
    let actor_id = string_arg(request, "actor_id").unwrap_or_default();
    let entry = build_entry(
        home,
        &group_id,
        EntryInput {
            kind: "conversation",
            summary: &summary,
            actor_id: &actor_id,
            source_refs: (0..messages.len().min(20))
                .map(|index| format!("chat:{index}"))
                .collect(),
            tags: vec!["daily_flush".into()],
            supersedes: Vec::new(),
            date: &date,
        },
    )?;
    let key = format!("daily_flush:{group_id}:{date}:{}", digest(&summary));
    let outcome = append_entry(&layout.today_file, &entry, &key).map_err(OpError::io)?;
    object(json!({
        "status":outcome.status,
        "reason":if outcome.status == "silent" { outcome.reason.as_str() } else { "" },
        "target_file":outcome.path,
        "content_hash":outcome.content_hash,
        "bytes_written":outcome.bytes_written,
        "signal_pack":signal_meta,
        "dedup":dedup.finalize(&outcome.status, &outcome.reason),
    }))
}

fn normalize_signal_pack(
    raw: Option<&Map<String, Value>>,
    budget: usize,
) -> (Option<Value>, Value) {
    let Some(raw) = raw else {
        return (
            None,
            json!({"schema":"v1","token_budget":budget,"token_estimate":0,"truncated":false}),
        );
    };
    let brief = raw
        .get("coordination_brief")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let tasks = raw
        .get("tasks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let constraints = brief
        .get("constraints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .take(6)
        .map(|value| Value::String(truncate(value, 64)))
        .collect::<Vec<_>>();
    let mut pack = json!({
        "schema":"v1",
        "coordination_brief":{
            "objective":text(&brief, "objective", 220),
            "current_focus":text(&brief, "current_focus", 180),
            "constraints":constraints,
            "project_brief":text(&brief, "project_brief", 280),
        },
        "tasks":{
            "active":task_items(tasks.get("active"), 8, 96),
            "planned":task_items(tasks.get("planned"), 8, 96),
            "done_recent":task_items(tasks.get("done_recent"), 6, 96),
            "blocked":task_items(tasks.get("blocked"), 6, 96),
            "waiting_user":task_items(tasks.get("waiting_user"), 4, 96),
        },
        "agent_states":agent_items(raw.get("agent_states"), 8),
    });
    if !has_signal_payload(&pack) {
        return (
            None,
            json!({"schema":"v1","token_budget":budget,"token_estimate":0,"truncated":false}),
        );
    }
    let mut truncated = false;
    for (parent, field) in [
        ("tasks", "done_recent"),
        ("tasks", "planned"),
        ("tasks", "blocked"),
        ("tasks", "waiting_user"),
    ] {
        while token_estimate(&pack) > budget && pop_array(&mut pack, parent, field) {
            truncated = true;
        }
    }
    if token_estimate(&pack) > budget {
        for field in ["persona_notes", "user_model", "environment_summary"] {
            if drop_agent_field(&mut pack, field) {
                truncated = true;
            }
            if token_estimate(&pack) <= budget {
                break;
            }
        }
    }
    while token_estimate(&pack) > budget && pop_root_array(&mut pack, "agent_states") {
        truncated = true;
    }
    while token_estimate(&pack) > budget && pop_array(&mut pack, "tasks", "active") {
        truncated = true;
    }
    while token_estimate(&pack) > budget
        && pop_array(&mut pack, "coordination_brief", "constraints")
    {
        truncated = true;
    }
    for (field, floor, step) in [
        ("project_brief", 0, 40),
        ("current_focus", 0, 20),
        ("objective", 0, 20),
    ] {
        while token_estimate(&pack) > budget && shrink_brief_field(&mut pack, field, floor, step) {
            truncated = true;
        }
    }
    if token_estimate(&pack) > budget {
        pack = json!({"schema":"v1"});
        truncated = true;
    }
    let estimate = token_estimate(&pack);
    (
        Some(pack),
        json!({
            "schema":"v1",
            "token_budget":budget,
            "token_estimate":estimate,
            "truncated":truncated,
        }),
    )
}

fn summarize_daily(messages: &[Value], signal_pack: Option<&Value>) -> String {
    let normalized = normalize_messages(messages);
    let conversation = normalized
        .iter()
        .filter_map(|message| {
            let content = message.get("content")?.as_str()?.trim();
            (!content.is_empty()).then(|| {
                format!(
                    "- [{}] {}",
                    message["role"].as_str().unwrap_or("assistant"),
                    truncate(content, 500)
                )
            })
        })
        .take(16)
        .collect::<Vec<_>>();
    let mut sections = Vec::new();
    if !conversation.is_empty() {
        sections.push(format!(
            "## Conversation Summary\n{}",
            conversation.join("\n")
        ));
    }
    let Some(pack) = signal_pack else {
        return sections.join("\n\n");
    };
    let brief = pack.get("coordination_brief").and_then(Value::as_object);
    let mut brief_lines = Vec::new();
    if let Some(brief) = brief {
        push_field(&mut brief_lines, brief, "objective", "Objective", 240);
        push_field(
            &mut brief_lines,
            brief,
            "current_focus",
            "Current Focus",
            240,
        );
        for constraint in brief
            .get("constraints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .take(4)
        {
            brief_lines.push(format!("- Constraint: {}", truncate(constraint, 160)));
        }
        push_field(
            &mut brief_lines,
            brief,
            "project_brief",
            "Project Brief",
            240,
        );
    }
    if !brief_lines.is_empty() {
        sections.push(format!(
            "## Coordination Snapshot\n{}",
            brief_lines.join("\n")
        ));
    }
    let mut task_lines = Vec::new();
    if let Some(tasks) = pack.get("tasks").and_then(Value::as_object) {
        for (field, label) in [
            ("active", "Active"),
            ("done_recent", "Done Recently"),
            ("blocked", "Blocked"),
            ("waiting_user", "Waiting User"),
            ("planned", "Planned"),
        ] {
            for item in tasks
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .take(4)
            {
                task_lines.push(format!("- {label}: {}", truncate(item, 200)));
            }
        }
    }
    if !task_lines.is_empty() {
        sections.push(format!("## Task Snapshot\n{}", task_lines.join("\n")));
    }
    let mut agent_lines = Vec::new();
    for agent in pack
        .get("agent_states")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .take(4)
    {
        let mut parts = Vec::new();
        for (field, label) in [
            ("id", ""),
            ("focus", "focus="),
            ("next_action", "next="),
            ("what_changed", "changed="),
        ] {
            if let Some(value) = agent
                .get(field)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                parts.push(format!("{label}{}", truncate(value, 120)));
            }
        }
        for (field, label) in [
            ("blockers", "blockers="),
            ("open_loops", "open="),
            ("commitments", "commit="),
        ] {
            let values = agent
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .take(2)
                .map(|value| truncate(value, 80))
                .collect::<Vec<_>>();
            if !values.is_empty() {
                parts.push(format!("{label}{}", values.join("; ")));
            }
        }
        if !parts.is_empty() {
            agent_lines.push(format!("- {}", parts.join(" | ")));
        }
    }
    if !agent_lines.is_empty() {
        sections.push(format!("## Agent Resume Cues\n{}", agent_lines.join("\n")));
    }
    sections.join("\n\n")
}

fn text(object: &Map<String, Value>, field: &str, max_chars: usize) -> String {
    truncate(
        object
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_default(),
        max_chars,
    )
}

fn task_items(value: Option<&Value>, max_items: usize, max_chars: usize) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(max_items)
        .filter_map(|item| {
            let label = if let Some(item) = item.as_object() {
                let id = text(item, "id", 24);
                let title = item
                    .get("title")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                format!("{id}: {}", truncate(title, max_chars.saturating_sub(28)))
                    .trim_matches([':', ' '])
                    .to_owned()
            } else {
                truncate(item.as_str().unwrap_or_default(), max_chars)
            };
            (!label.is_empty()).then_some(label)
        })
        .collect()
}

fn agent_items(value: Option<&Value>, max_items: usize) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .take(max_items)
        .filter_map(|agent| {
            let id = text(agent, "id", 32);
            if id.is_empty() {
                return None;
            }
            let hot = agent.get("hot").and_then(Value::as_object).unwrap_or(agent);
            let warm = agent
                .get("warm")
                .and_then(Value::as_object)
                .unwrap_or(agent);
            let mut row = Map::from_iter([("id".into(), Value::String(id))]);
            for (source, field, max_chars) in [
                (hot, "active_task_id", 24),
                (hot, "focus", 120),
                (hot, "next_action", 120),
                (warm, "what_changed", 140),
                (warm, "environment_summary", 120),
                (warm, "user_model", 120),
                (warm, "persona_notes", 120),
            ] {
                let value = text(source, field, max_chars);
                if !value.is_empty() {
                    row.insert(field.into(), Value::String(value));
                }
            }
            for (source, field, max_items, max_chars) in [
                (hot, "blockers", 3, 80),
                (warm, "open_loops", 3, 100),
                (warm, "commitments", 3, 100),
            ] {
                let values = source
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .take(max_items)
                    .map(|value| Value::String(truncate(value, max_chars)))
                    .collect::<Vec<_>>();
                if !values.is_empty() {
                    row.insert(field.into(), Value::Array(values));
                }
            }
            (row.len() > 1).then_some(Value::Object(row))
        })
        .collect()
}

fn has_signal_payload(pack: &Value) -> bool {
    token_estimate(pack)
        > token_estimate(&json!({
            "schema":"v1",
            "coordination_brief":{"objective":"","current_focus":"","constraints":[],"project_brief":""},
            "tasks":{"active":[],"planned":[],"done_recent":[],"blocked":[],"waiting_user":[]},
            "agent_states":[]
        }))
}

fn token_estimate(value: &Value) -> usize {
    serde_json::to_string(value).map_or(1, |text| (text.chars().count() / 4).max(1))
}

fn pop_array(pack: &mut Value, parent: &str, field: &str) -> bool {
    pack.get_mut(parent)
        .and_then(Value::as_object_mut)
        .and_then(|object| object.get_mut(field))
        .and_then(Value::as_array_mut)
        .is_some_and(|items| items.pop().is_some())
}

fn pop_root_array(pack: &mut Value, field: &str) -> bool {
    pack.get_mut(field)
        .and_then(Value::as_array_mut)
        .is_some_and(|items| items.pop().is_some())
}

fn drop_agent_field(pack: &mut Value, field: &str) -> bool {
    let mut changed = false;
    if let Some(agents) = pack.get_mut("agent_states").and_then(Value::as_array_mut) {
        for agent in agents.iter_mut().filter_map(Value::as_object_mut) {
            changed |= agent.remove(field).is_some();
        }
    }
    changed
}

fn shrink_brief_field(pack: &mut Value, field: &str, floor: usize, step: usize) -> bool {
    let Some(value) = pack
        .get_mut("coordination_brief")
        .and_then(Value::as_object_mut)
        .and_then(|brief| brief.get_mut(field))
        .and_then(|value| value.as_str())
    else {
        return false;
    };
    let length = value.chars().count();
    if length <= floor {
        return false;
    }
    let shortened = truncate(value, length.saturating_sub(step).max(floor));
    pack["coordination_brief"][field] = Value::String(shortened);
    true
}

fn push_field(
    output: &mut Vec<String>,
    object: &Map<String, Value>,
    field: &str,
    label: &str,
    max_chars: usize,
) {
    if let Some(value) = object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        output.push(format!("- {label}: {}", truncate(value, max_chars)));
    }
}

fn today() -> String {
    cccc_contracts::utc_now()[..10].to_owned()
}

#[cfg(test)]
mod tests {
    use super::{normalize_signal_pack, summarize_daily};
    use serde_json::json;

    #[test]
    fn signal_pack_never_exceeds_requested_budget() {
        let constraints = vec!["x"; 20];
        let active = (0..20).map(|_| "D".repeat(200)).collect::<Vec<_>>();
        let planned = (0..20).map(|_| "E".repeat(200)).collect::<Vec<_>>();
        let raw = json!({
            "coordination_brief":{"objective":"A".repeat(2000),"current_focus":"B".repeat(1200),"project_brief":"C".repeat(1200),"constraints":constraints},
            "tasks":{"active":active,"planned":planned},
            "agent_states":[{"id":"peer1","hot":{"focus":"F".repeat(400),"next_action":"G".repeat(400)}}]
        });
        let (_, meta) = normalize_signal_pack(raw.as_object(), 64);
        assert_eq!(meta["schema"], "v1");
        assert!(meta["token_estimate"].as_u64().unwrap_or(u64::MAX) <= 64);
        assert_eq!(meta["truncated"], true);
    }

    #[test]
    fn daily_summary_keeps_python_agent_resume_cues() {
        let pack = json!({
            "agent_states":[{
                "id":"peer1",
                "focus":"finish parity",
                "blockers":["waiting for CI"],
                "open_loops":["verify release"],
                "commitments":["preserve compatibility"]
            }]
        });
        let summary = summarize_daily(&[], Some(&pack));
        assert!(summary.contains("blockers=waiting for CI"));
        assert!(summary.contains("open=verify release"));
        assert!(summary.contains("commit=preserve compatibility"));
    }
}
