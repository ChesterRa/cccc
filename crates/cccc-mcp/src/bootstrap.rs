use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::router::daemon;

const RECOVERY_TOKEN_BUDGET: usize = 1_100;
const STALE_AFTER_SECONDS: i64 = 20 * 60;
const MIND_HOT_ONLY_UPDATE_THRESHOLD: u64 = 3;
pub(crate) async fn build(
    home: &HomeLayout,
    client: &DaemonClient,
    args: Map<String, Value>,
) -> Result<Value, String> {
    let group_id = string_value(args.get("group_id"))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "group_id is required".to_owned())?;
    let actor_id = string_value(args.get("actor_id"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "user".into());
    let inbox_limit = args
        .get("inbox_limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 200) as usize;
    let group_result = daemon(client, "group_show", request_args(&group_id, &actor_id)).await?;
    let group = group_result
        .get("group")
        .and_then(Value::as_object)
        .ok_or_else(|| "group_show returned no group".to_owned())?;

    let actors_result = daemon(client, "actor_list", request_args(&group_id, &actor_id)).await?;
    let actors = actors_result
        .get("actors")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    let context = daemon(client, "context_get", request_args(&group_id, &actor_id)).await?;

    let mut inbox_args = request_args(&group_id, &actor_id);
    inbox_args.insert(
        "limit".into(),
        json!(inbox_limit.saturating_add(1).min(200)),
    );
    let inbox = daemon(client, "inbox_peek", inbox_args).await?;

    let runtime_meta = load_runtime_meta(home, &group_id, &actor_id);
    let recovery_pack = build_recovery_pack(&context, &actor_id, &runtime_meta, Utc::now());
    let context_hygiene = recovery_pack
        .get("context_hygiene")
        .cloned()
        .unwrap_or_else(|| missing_hygiene(&actor_id));
    let memory_recall_gate =
        build_memory_recall_gate(client, &group_id, &actor_id, &recovery_pack).await;

    let mut payload = assemble_payload(
        build_session(group, actors, &actor_id),
        build_recovery(&recovery_pack),
        build_inbox_preview(&inbox, inbox_limit),
        context_hygiene,
        memory_recall_gate,
    );
    if let Ok(store) = cccc_core::GroupStore::new(home.clone())
        && let Ok(group) = store.load(&group_id)
        && let Ok(Some(pending)) = cccc_core::inbox::mail_pending_summary(home, &group, &actor_id)
    {
        payload["mail_pending"] = pending;
    }
    Ok(payload)
}

fn assemble_payload(
    session: Value,
    recovery: Value,
    inbox_preview: Value,
    context_hygiene: Value,
    memory_recall_gate: Value,
) -> Value {
    json!({
        "session": session,
        "recovery": recovery,
        "inbox_preview": inbox_preview,
        "context_hygiene": context_hygiene,
        "memory_recall_gate": memory_recall_gate,
        "next_calls": {
            "help": "cccc_help()  # when a CCCC route or state boundary is unclear",
            "project_info": "cccc_capability_use(tool_name=\"cccc_project_info\", tool_arguments={})",
            "context_get": "cccc_context_get()",
            "inbox_read": "cccc_inbox_read()",
            "memory_search": "cccc_capability_use(tool_name=\"cccc_memory\", tool_arguments={\"action\":\"search\",\"query\":\"...\"})",
        }
    })
}

fn request_args(group_id: &str, actor_id: &str) -> Map<String, Value> {
    Map::from_iter([
        ("group_id".into(), Value::String(group_id.to_owned())),
        ("actor_id".into(), Value::String(actor_id.to_owned())),
        ("by".into(), Value::String(actor_id.to_owned())),
    ])
}

fn build_session(group: &Map<String, Value>, actors: &[Value], actor_id: &str) -> Value {
    let actor = actors
        .iter()
        .filter_map(Value::as_object)
        .find(|actor| actor.get("id").and_then(Value::as_str) == Some(actor_id));
    let scope = selected_scope(group);
    let project_path = scope
        .and_then(|scope| scope.get("url"))
        .and_then(Value::as_str)
        .map(Path::new)
        .map(|root| root.join("PROJECT.md"));
    let project_found = project_path.as_ref().is_some_and(|path| path.is_file());
    let active_scope = scope.map_or_else(
        || json!({}),
        |scope| {
            json!({
                "scope_key": string_value(scope.get("scope_key")).unwrap_or_default(),
                "path": string_value(scope.get("url")).unwrap_or_default(),
            })
        },
    );

    json!({
        "group_id": string_value(group.get("group_id")).unwrap_or_default(),
        "group_title": string_value(group.get("title"))
            .filter(|value| !value.is_empty())
            .or_else(|| string_value(group.get("group_id")))
            .unwrap_or_default(),
        "actor_id": actor_id,
        "role": actor.and_then(|item| string_value(item.get("role"))).unwrap_or_default(),
        "runner": actor.and_then(|item| string_value(item.get("runner"))).unwrap_or_default(),
        "active_scope": active_scope,
        "project_md": {
            "found": project_found,
            "path": project_path.filter(|_| project_found).map(|path| path.to_string_lossy().into_owned()),
        },
    })
}

fn selected_scope(group: &Map<String, Value>) -> Option<&Map<String, Value>> {
    let active = group
        .get("active_scope_key")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let scopes = group.get("scopes").and_then(Value::as_array)?;
    scopes
        .iter()
        .filter_map(Value::as_object)
        .find(|scope| {
            !active.is_empty() && scope.get("scope_key").and_then(Value::as_str) == Some(active)
        })
        .or_else(|| scopes.iter().find_map(Value::as_object))
}

fn build_recovery_pack(
    context: &Map<String, Value>,
    actor_id: &str,
    runtime_meta: &Map<String, Value>,
    now: DateTime<Utc>,
) -> Value {
    let actor_state = context
        .get("agent_states")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|state| {
            state
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.eq_ignore_ascii_case(actor_id))
        });
    let raw_hot = actor_state
        .and_then(|state| state.get("hot"))
        .and_then(Value::as_object);
    let raw_warm = actor_state
        .and_then(|state| state.get("warm"))
        .and_then(Value::as_object);
    let hot = slim_hot(raw_hot);
    let warm = slim_warm(raw_warm);
    let coordination = context.get("coordination").and_then(Value::as_object);
    let brief = coordination
        .and_then(|value| value.get("brief"))
        .and_then(Value::as_object);
    let tasks = coordination
        .and_then(|value| value.get("tasks"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let (assigned_active, attention) = task_slice(tasks, actor_id);
    let updated_at = actor_state
        .and_then(|state| state.get("updated_at"))
        .cloned()
        .unwrap_or(Value::Null);
    let hygiene = evaluate_hygiene(
        actor_id,
        raw_hot,
        raw_warm,
        actor_state.and_then(|state| state.get("updated_at")),
        runtime_meta,
        actor_state.is_some(),
        now,
    );

    let mut pack = json!({
        "agent_state": {
            "id": actor_state.and_then(|state| state.get("id")).cloned().unwrap_or_else(|| Value::String(actor_id.to_owned())),
            "hot": hot,
            "warm": warm,
            "mind_context_mini": mind_context_mini(raw_warm),
            "updated_at": updated_at,
        },
        "coordination_brief": clean_object(Map::from_iter([
            ("objective".into(), trimmed_value(brief.and_then(|value| value.get("objective")), 180)),
            ("current_focus".into(), trimmed_value(brief.and_then(|value| value.get("current_focus")), 180)),
            ("constraints".into(), Value::Array(trimmed_list(brief.and_then(|value| value.get("constraints")), 6, 140))),
            ("project_brief".into(), trimmed_value(brief.and_then(|value| value.get("project_brief")), 260)),
            ("project_brief_stale".into(), Value::Bool(brief.and_then(|value| value.get("project_brief_stale")).and_then(Value::as_bool).unwrap_or(false))),
        ])),
        "tasks": {
            "assigned_active": assigned_active,
            "attention": attention,
        },
        "recent_decisions": recent_notes(coordination, "recent_decisions"),
        "recent_handoffs": recent_notes(coordination, "recent_handoffs"),
        "context_hygiene": hygiene,
    });
    shrink_recovery_pack(&mut pack);
    pack
}

fn slim_hot(source: Option<&Map<String, Value>>) -> Value {
    let source = source.cloned().unwrap_or_default();
    Value::Object(clean_object(Map::from_iter([
        (
            "active_task_id".into(),
            trimmed_value(source.get("active_task_id"), 64),
        ),
        ("focus".into(), trimmed_value(source.get("focus"), 180)),
        (
            "next_action".into(),
            trimmed_value(source.get("next_action"), 180),
        ),
        (
            "blockers".into(),
            Value::Array(trimmed_list(source.get("blockers"), 3, 140)),
        ),
    ])))
}

fn slim_warm(source: Option<&Map<String, Value>>) -> Value {
    let source = source.cloned().unwrap_or_default();
    Value::Object(clean_object(Map::from_iter([
        (
            "what_changed".into(),
            trimmed_value(source.get("what_changed"), 180),
        ),
        (
            "open_loops".into(),
            Value::Array(trimmed_list(source.get("open_loops"), 3, 140)),
        ),
        (
            "commitments".into(),
            Value::Array(trimmed_list(source.get("commitments"), 3, 140)),
        ),
        (
            "environment_summary".into(),
            trimmed_value(source.get("environment_summary"), 160),
        ),
        (
            "user_model".into(),
            trimmed_value(source.get("user_model"), 160),
        ),
        (
            "persona_notes".into(),
            trimmed_value(source.get("persona_notes"), 160),
        ),
    ])))
}

fn mind_context_mini(source: Option<&Map<String, Value>>) -> Value {
    let source = source.cloned().unwrap_or_default();
    Value::Object(clean_object(Map::from_iter([
        (
            "environment_summary".into(),
            trimmed_value(source.get("environment_summary"), 84),
        ),
        (
            "user_model".into(),
            trimmed_value(source.get("user_model"), 84),
        ),
        (
            "persona_notes".into(),
            trimmed_value(source.get("persona_notes"), 84),
        ),
    ])))
}

fn task_slice(tasks: &[Value], actor_id: &str) -> (Vec<Value>, Vec<Value>) {
    let mut assigned = Vec::new();
    let mut actor_attention = Vec::new();
    let mut waiting_user = Vec::new();
    let mut globally_blocked = Vec::new();
    for task in tasks.iter().filter_map(Value::as_object) {
        let status = lower_string(task.get("status"));
        if matches!(status.as_str(), "done" | "archived") {
            continue;
        }
        let assignee = string_value(task.get("assignee")).unwrap_or_default();
        let handoff_to = string_value(task.get("handoff_to")).unwrap_or_default();
        let waiting_on = lower_string(task.get("waiting_on"));
        let slim = slim_task(task);
        if assignee == actor_id && status == "active" {
            assigned.push(slim);
        } else if assignee == actor_id || handoff_to == actor_id || waiting_on == "actor" {
            actor_attention.push(slim);
        } else if waiting_on == "user" {
            waiting_user.push(slim);
        } else if task
            .get("blocked_by")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
            || waiting_on == "external"
        {
            globally_blocked.push(slim);
        }
    }

    let mut seen = HashSet::new();
    let primary = take_unique(&assigned, 3, &mut seen);
    let mut attention = take_unique(&actor_attention, 2, &mut seen);
    attention.extend(take_unique(
        &waiting_user,
        3usize.saturating_sub(attention.len()),
        &mut seen,
    ));
    attention.extend(take_unique(
        &globally_blocked,
        3usize.saturating_sub(attention.len()),
        &mut seen,
    ));
    (primary, attention)
}

fn take_unique(items: &[Value], limit: usize, seen: &mut HashSet<String>) -> Vec<Value> {
    if limit == 0 {
        return Vec::new();
    }
    let mut output = Vec::new();
    for item in items {
        let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() || !seen.insert(id.to_owned()) {
            continue;
        }
        output.push(item.clone());
        if output.len() >= limit {
            break;
        }
    }
    output
}

fn slim_task(task: &Map<String, Value>) -> Value {
    let checklist = task
        .get("checklist")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .take(3)
        .map(|item| {
            Value::Object(clean_object(Map::from_iter([
                ("id".into(), trimmed_value(item.get("id"), 64)),
                ("text".into(), trimmed_value(item.get("text"), 120)),
                (
                    "status".into(),
                    Value::String(
                        string_value(item.get("status")).unwrap_or_else(|| "pending".into()),
                    ),
                ),
            ])))
        })
        .collect::<Vec<_>>();
    let blocked_by = trimmed_list(task.get("blocked_by"), 4, 64);
    Value::Object(clean_object(Map::from_iter([
        ("id".into(), trimmed_value(task.get("id"), 64)),
        ("title".into(), trimmed_value(task.get("title"), 120)),
        ("outcome".into(), trimmed_value(task.get("outcome"), 160)),
        ("parent_id".into(), trimmed_value(task.get("parent_id"), 64)),
        ("status".into(), trimmed_value(task.get("status"), 32)),
        ("assignee".into(), trimmed_value(task.get("assignee"), 64)),
        ("priority".into(), trimmed_value(task.get("priority"), 32)),
        (
            "waiting_on".into(),
            Value::String(string_value(task.get("waiting_on")).unwrap_or_else(|| "none".into())),
        ),
        (
            "handoff_to".into(),
            trimmed_value(task.get("handoff_to"), 64),
        ),
        ("task_type".into(), trimmed_value(task.get("task_type"), 48)),
        ("notes".into(), trimmed_value(task.get("notes"), 240)),
        ("checklist".into(), Value::Array(checklist)),
        (
            "updated_at".into(),
            task.get("updated_at").cloned().unwrap_or(Value::Null),
        ),
        ("blocked_by".into(), Value::Array(blocked_by)),
    ])))
}

fn recent_notes(coordination: Option<&Map<String, Value>>, key: &str) -> Value {
    let notes = coordination
        .and_then(|value| value.get(key))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .take(2)
        .map(|item| {
            Value::Object(clean_object(Map::from_iter([
                ("at".into(), item.get("at").cloned().unwrap_or(Value::Null)),
                ("by".into(), trimmed_value(item.get("by"), 64)),
                ("summary".into(), trimmed_value(item.get("summary"), 180)),
                ("task_id".into(), trimmed_value(item.get("task_id"), 64)),
            ])))
        })
        .collect();
    Value::Array(notes)
}

fn build_recovery(pack: &Value) -> Value {
    let agent_state = pack.get("agent_state").and_then(Value::as_object);
    let mut recovery = Map::from_iter([
        (
            "coordination_brief".into(),
            pack.get("coordination_brief")
                .cloned()
                .unwrap_or_else(|| json!({})),
        ),
        (
            "self_state".into(),
            json!({
                "hot": agent_state.and_then(|state| state.get("hot")).cloned().unwrap_or_else(|| json!({})),
                "recovery": agent_state.and_then(|state| state.get("warm")).cloned().unwrap_or_else(|| json!({})),
                "mind_context_mini": agent_state.and_then(|state| state.get("mind_context_mini")).cloned().unwrap_or_else(|| json!({})),
                "updated_at": agent_state.and_then(|state| state.get("updated_at")).cloned().unwrap_or(Value::Null),
            }),
        ),
        (
            "task_slice".into(),
            pack.get("tasks")
                .cloned()
                .unwrap_or_else(|| json!({"assigned_active": [], "attention": []})),
        ),
        (
            "recent_notes".into(),
            json!({
                "decisions": pack.get("recent_decisions").cloned().unwrap_or_else(|| json!([])),
                "handoffs": pack.get("recent_handoffs").cloned().unwrap_or_else(|| json!([])),
            }),
        ),
    ]);
    if has_recoverable_work(pack) {
        recovery.insert(
            "takeover_nudge".into(),
            Value::String(cccc_core::peer_insight::BOOTSTRAP_TAKEOVER_NUDGE.into()),
        );
    }
    Value::Object(recovery)
}

fn has_recoverable_work(pack: &Value) -> bool {
    let agent_state = pack.get("agent_state").and_then(Value::as_object);
    let hot = agent_state
        .and_then(|state| state.get("hot"))
        .and_then(Value::as_object);
    let warm = agent_state
        .and_then(|state| state.get("warm"))
        .and_then(Value::as_object);
    let brief = pack.get("coordination_brief").and_then(Value::as_object);
    ["active_task_id", "focus", "next_action"]
        .into_iter()
        .any(|field| non_blank(hot.and_then(|value| value.get(field))))
        || non_blank(brief.and_then(|value| value.get("current_focus")))
        || non_empty_array(hot.and_then(|value| value.get("blockers")))
        || ["open_loops", "commitments"]
            .into_iter()
            .any(|field| non_empty_array(warm.and_then(|value| value.get(field))))
        || ["assigned_active", "attention"].into_iter().any(|field| {
            pack.get("tasks")
                .and_then(|value| value.get(field))
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.as_object().is_some_and(|item| !item.is_empty()))
                })
        })
}

fn build_inbox_preview(inbox: &Map<String, Value>, limit: usize) -> Value {
    let messages = inbox
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let preview = messages
        .iter()
        .take(limit)
        .filter_map(Value::as_object)
        .map(|item| {
            let data = item.get("data").and_then(Value::as_object);
            let text = data
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut entry = clean_object(Map::from_iter([
                ("id".into(), trimmed_value(item.get("id"), 80)),
                ("ts".into(), trimmed_value(item.get("ts"), 48)),
                ("by".into(), trimmed_value(item.get("by"), 64)),
                ("kind".into(), Value::String("chat.message".into())),
                ("message_mode".into(), Value::String("mail".into())),
                ("text_preview".into(), Value::String(trim_text(text, 220))),
            ]));
            if let Some(insight) = data
                .and_then(|value| value.get("insight"))
                .and_then(Value::as_str)
                .map(|value| trim_text(value, 220))
                .filter(|value| !value.is_empty())
            {
                entry.insert("insight_preview".into(), Value::String(insight));
            }
            Value::Object(entry)
        })
        .collect::<Vec<_>>();
    json!({"messages": preview, "truncated": messages.len() > limit})
}

async fn build_memory_recall_gate(
    client: &DaemonClient,
    group_id: &str,
    actor_id: &str,
    recovery_pack: &Value,
) -> Value {
    let query = memory_recall_query(recovery_pack);
    let mut args = request_args(group_id, actor_id);
    args.insert("query".into(), Value::String(query.clone()));
    args.insert("max_results".into(), json!(3));
    args.insert("min_score".into(), json!(0.1));
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        daemon(client, "memory_reme_search", args),
    )
    .await;
    let mut gate = json!({
        "required": true,
        "status": "empty",
        "query": query,
        "hits": [],
        "note": "Recall gate: read this before planning or implementation. If it is empty, expand with local cccc_memory(search/get).",
    });
    match result {
        Ok(Ok(response)) => {
            let hits = response
                .get("hits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .take(3)
                .map(|item| {
                    json!({
                        "path": string_value(item.get("path")).unwrap_or_default(),
                        "start_line": item.get("start_line").and_then(Value::as_u64).unwrap_or(1),
                        "score": item.get("score").and_then(Value::as_f64).unwrap_or(0.0),
                        "snippet": trim_text(item.get("snippet").and_then(Value::as_str).unwrap_or_default(), 220),
                    })
                })
                .collect::<Vec<_>>();
            gate["status"] = Value::String(if hits.is_empty() { "empty" } else { "ready" }.into());
            gate["hits"] = Value::Array(hits);
        }
        Ok(Err(error)) => {
            gate["status"] = Value::String("error".into());
            gate["error"] = Value::String(error);
        }
        Err(_) => {
            gate["status"] = Value::String("error".into());
            gate["error"] = Value::String("memory recall timed out after 4 seconds".into());
        }
    }
    gate
}

fn memory_recall_query(pack: &Value) -> String {
    let agent_state = pack.get("agent_state").and_then(Value::as_object);
    let hot = agent_state
        .and_then(|value| value.get("hot"))
        .and_then(Value::as_object);
    let warm = agent_state
        .and_then(|value| value.get("warm"))
        .and_then(Value::as_object);
    let mini = agent_state
        .and_then(|value| value.get("mind_context_mini"))
        .and_then(Value::as_object);
    let brief = pack.get("coordination_brief").and_then(Value::as_object);
    let tasks = pack.get("tasks").and_then(Value::as_object);
    let mut ranked = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |priority: usize, value: Option<&Value>, max_chars: usize| {
        let text = trim_text(&string_value(value).unwrap_or_default(), max_chars);
        if !text.is_empty() && seen.insert(text.to_lowercase()) {
            ranked.push((priority, text));
        }
    };
    add(0, hot.and_then(|value| value.get("active_task_id")), 48);
    add(1, hot.and_then(|value| value.get("focus")), 120);
    add(2, hot.and_then(|value| value.get("next_action")), 120);
    add(3, brief.and_then(|value| value.get("current_focus")), 120);
    if let Some(task) = tasks
        .and_then(|value| value.get("assigned_active"))
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
    {
        add(4, task.get("title"), 100);
        add(5, task.get("outcome"), 120);
    }
    add(6, warm.and_then(|value| value.get("what_changed")), 120);
    for value in warm
        .and_then(|value| value.get("open_loops"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(2)
    {
        add(7, Some(value), 120);
    }
    if let Some(task) = tasks
        .and_then(|value| value.get("attention"))
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
    {
        add(8, task.get("title"), 100);
    }
    for value in warm
        .and_then(|value| value.get("commitments"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(2)
    {
        add(8, Some(value), 120);
    }
    add(9, brief.and_then(|value| value.get("objective")), 120);
    add(
        10,
        warm.and_then(|value| value.get("environment_summary"))
            .or_else(|| mini.and_then(|value| value.get("environment_summary"))),
        100,
    );
    add(
        11,
        warm.and_then(|value| value.get("user_model"))
            .or_else(|| mini.and_then(|value| value.get("user_model"))),
        100,
    );
    add(
        12,
        warm.and_then(|value| value.get("persona_notes"))
            .or_else(|| mini.and_then(|value| value.get("persona_notes"))),
        100,
    );
    if ranked.is_empty() {
        return "recent decisions constraints preferences".into();
    }
    ranked.sort_by_key(|item| item.0);
    trim_text(
        &ranked
            .into_iter()
            .take(6)
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join(" | "),
        240,
    )
}

fn evaluate_hygiene(
    actor_id: &str,
    hot: Option<&Map<String, Value>>,
    warm: Option<&Map<String, Value>>,
    updated_at: Option<&Value>,
    runtime_meta: &Map<String, Value>,
    present: bool,
    now: DateTime<Utc>,
) -> Value {
    let updated = string_value(updated_at).unwrap_or_default();
    let execution_age = age_seconds(&updated, now);
    let execution_is_stale = execution_age.is_none_or(|age| age > STALE_AFTER_SECONDS);
    let mut execution_fields = Vec::new();
    for field in ["active_task_id", "focus", "next_action"] {
        if non_blank(hot.and_then(|value| value.get(field))) {
            execution_fields.push(field);
        }
    }
    if non_blank(warm.and_then(|value| value.get("what_changed"))) {
        execution_fields.push("what_changed");
    }
    if non_empty_array(hot.and_then(|value| value.get("blockers"))) {
        execution_fields.push("blockers");
    }
    let mind_fields = ["environment_summary", "user_model", "persona_notes"];
    let mind_present = mind_fields
        .into_iter()
        .filter(|field| non_blank(warm.and_then(|value| value.get(*field))))
        .collect::<Vec<_>>();
    let mut mind_touched =
        string_value(runtime_meta.get("mind_context_touched_at")).unwrap_or_default();
    if mind_touched.is_empty() && !mind_present.is_empty() {
        mind_touched.clone_from(&updated);
    }
    let mind_age = age_seconds(&mind_touched, now);
    let churn = runtime_meta
        .get("hot_only_updates_since_mind_touch")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let mind_is_stale = mind_age.is_none_or(|age| age > STALE_AFTER_SECONDS)
        || churn >= MIND_HOT_ONLY_UPDATE_THRESHOLD;
    let execution_status = if !present || execution_fields.is_empty() {
        "missing"
    } else if execution_is_stale {
        "stale"
    } else {
        "ready"
    };
    let mind_status = if !present || mind_present.is_empty() {
        "missing"
    } else if mind_is_stale {
        "stale"
    } else if mind_present.len() < mind_fields.len() {
        "partial"
    } else {
        "ready"
    };
    let recommendation = if !present {
        "update_agent_state_now"
    } else if execution_status == "missing" {
        "fill_execution_state"
    } else if execution_status == "stale" {
        "refresh_execution_state"
    } else if matches!(mind_status, "missing" | "partial") {
        "fill_mind_context"
    } else if mind_status == "stale" {
        "refresh_mind_context"
    } else {
        "state_healthy"
    };
    let missing_execution_fields = [
        "active_task_id",
        "focus",
        "next_action",
        "what_changed",
        "blockers",
    ]
    .into_iter()
    .filter(|field| !execution_fields.contains(field))
    .collect::<Vec<_>>();
    let missing_mind_fields = mind_fields
        .into_iter()
        .filter(|field| !mind_present.contains(field))
        .collect::<Vec<_>>();
    json!({
        "actor_id": actor_id,
        "present": present,
        "age_seconds": execution_age,
        "stale": execution_is_stale,
        "min_fields_ready": !execution_fields.is_empty(),
        "execution_health": {
            "status": execution_status,
            "present_fields": execution_fields,
            "missing_fields": missing_execution_fields,
        },
        "mind_context_health": {
            "status": mind_status,
            "present_fields": mind_present,
            "missing_fields": missing_mind_fields,
            "touched_at": if mind_touched.is_empty() { Value::Null } else { Value::String(mind_touched) },
            "touch_age_seconds": mind_age,
            "hot_only_updates_since_touch": churn,
        },
        "update_command": "cccc_agent_state(action=\"update\", actor_id=\"<self>\", focus=\"...\", next_action=\"...\", what_changed=\"...\", environment_summary=\"...\", user_model=\"...\", persona_notes=\"...\")",
        "execution_update_command": "cccc_agent_state(action=\"update\", actor_id=\"<self>\", focus=\"...\", next_action=\"...\", what_changed=\"...\")",
        "mind_context_update_command": "cccc_agent_state(action=\"update\", actor_id=\"<self>\", environment_summary=\"...\", user_model=\"...\", persona_notes=\"...\")",
        "recommendation": recommendation,
    })
}

fn missing_hygiene(actor_id: &str) -> Value {
    evaluate_hygiene(actor_id, None, None, None, &Map::new(), false, Utc::now())
}

fn age_seconds(value: &str, now: DateTime<Utc>) -> Option<i64> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .ok()?
        .with_timezone(&Utc);
    Some(now.signed_duration_since(parsed).num_seconds().max(0))
}

fn load_runtime_meta(home: &HomeLayout, group_id: &str, actor_id: &str) -> Map<String, Value> {
    let path = home
        .groups_dir()
        .join(group_id)
        .join("state")
        .join("automation.json");
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|root| root.get("actors").cloned())
        .and_then(|actors| actors.get(actor_id).cloned())
        .and_then(|actor| actor.as_object().cloned())
        .unwrap_or_default()
}

fn shrink_recovery_pack(pack: &mut Value) {
    while estimate_tokens(pack) > RECOVERY_TOKEN_BUDGET && pop_last(pack.get_mut("recent_handoffs"))
    {
    }
    while estimate_tokens(pack) > RECOVERY_TOKEN_BUDGET
        && pop_last(pack.get_mut("recent_decisions"))
    {}
    while estimate_tokens(pack) > RECOVERY_TOKEN_BUDGET
        && array_len(pack.pointer("/tasks/attention")) > 1
        && pop_last(pack.pointer_mut("/tasks/attention"))
    {}
    while estimate_tokens(pack) > RECOVERY_TOKEN_BUDGET
        && array_len(pack.pointer("/tasks/assigned_active")) > 1
        && pop_last(pack.pointer_mut("/tasks/assigned_active"))
    {}
    if estimate_tokens(pack) > RECOVERY_TOKEN_BUDGET {
        for bucket in ["assigned_active", "attention"] {
            if let Some(items) = pack
                .pointer_mut(&format!("/tasks/{bucket}"))
                .and_then(Value::as_array_mut)
            {
                for task in items.iter_mut().filter_map(Value::as_object_mut) {
                    task.remove("notes");
                    if let Some(checklist) = task.get_mut("checklist").and_then(Value::as_array_mut)
                    {
                        checklist.truncate(1);
                    }
                }
            }
        }
    }
    if estimate_tokens(pack) > RECOVERY_TOKEN_BUDGET {
        pack.pointer_mut("/coordination_brief")
            .and_then(Value::as_object_mut)
            .map(|brief| brief.remove("project_brief"));
    }
    for field in [
        "environment_summary",
        "user_model",
        "persona_notes",
        "what_changed",
        "commitments",
        "open_loops",
    ] {
        if estimate_tokens(pack) <= RECOVERY_TOKEN_BUDGET {
            break;
        }
        pack.pointer_mut("/agent_state/warm")
            .and_then(Value::as_object_mut)
            .map(|warm| warm.remove(field));
    }
}

fn estimate_tokens(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(1, |bytes| (bytes.len() / 4).max(1))
}

fn pop_last(value: Option<&mut Value>) -> bool {
    value
        .and_then(Value::as_array_mut)
        .is_some_and(|items| items.pop().is_some())
}

fn array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map_or(0, Vec::len)
}

fn clean_object(mut object: Map<String, Value>) -> Map<String, Value> {
    object.retain(|_, value| !is_empty(value));
    object
}

fn is_empty(value: &Value) -> bool {
    matches!(value, Value::Null)
        || value.as_str().is_some_and(str::is_empty)
        || value.as_array().is_some_and(Vec::is_empty)
        || value.as_object().is_some_and(Map::is_empty)
}

fn string_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(value) => Some(value.trim().to_owned()),
        value => Some(value.to_string().trim().to_owned()),
    }
}

fn lower_string(value: Option<&Value>) -> String {
    string_value(value).unwrap_or_default().to_ascii_lowercase()
}

fn trimmed_value(value: Option<&Value>, max_chars: usize) -> Value {
    Value::String(trim_text(
        &string_value(value).unwrap_or_default(),
        max_chars,
    ))
}

fn trimmed_list(value: Option<&Value>, max_items: usize, max_chars: usize) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let text = trim_text(&string_value(Some(item)).unwrap_or_default(), max_chars);
            (!text.is_empty()).then_some(Value::String(text))
        })
        .take(max_items)
        .collect()
}

fn trim_text(value: &str, max_chars: usize) -> String {
    let text = value.trim();
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 3 {
        return text.chars().take(max_chars).collect();
    }
    format!(
        "{}...",
        text.chars()
            .take(max_chars - 3)
            .collect::<String>()
            .trim_end()
    )
}

fn non_blank(value: Option<&Value>) -> bool {
    string_value(value).is_some_and(|value| !value.is_empty())
}

fn non_empty_array(value: Option<&Value>) -> bool {
    value.and_then(Value::as_array).is_some_and(|items| {
        items.iter().any(|item| match item {
            Value::String(value) => !value.trim().is_empty(),
            Value::Object(value) => !value.is_empty(),
            _ => false,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixture_context() -> Map<String, Value> {
        json!({
            "coordination": {
                "brief": {
                    "objective": "verify one lean bootstrap packet",
                    "current_focus": "bootstrap semantic conformance"
                },
                "tasks": [{
                    "id": "T1",
                    "title": "Verify bootstrap semantics",
                    "outcome": "The native engine exposes one useful packet.",
                    "status": "active",
                    "assignee": "peer1"
                }],
                "recent_decisions": [],
                "recent_handoffs": []
            },
            "agent_states": [{
                "id": "peer1",
                "hot": {
                    "focus": "bootstrap semantic conformance",
                    "next_action": "compare packets"
                },
                "warm": {
                    "what_changed": "seeded fixture",
                    "environment_summary": "isolated home",
                    "user_model": "evidence first",
                    "persona_notes": "concise"
                },
                "updated_at": "2026-08-09T05:00:00Z"
            }]
        })
        .as_object()
        .cloned()
        .expect("fixture context")
    }

    #[test]
    fn recovery_restores_owned_work_without_raw_context() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 9, 5, 1, 0)
            .single()
            .expect("timestamp");
        let pack = build_recovery_pack(&fixture_context(), "peer1", &Map::new(), now);
        let recovery = build_recovery(&pack);

        assert_eq!(
            recovery["self_state"]["hot"]["focus"],
            "bootstrap semantic conformance"
        );
        assert_eq!(
            recovery["task_slice"]["assigned_active"][0]["title"],
            "Verify bootstrap semantics"
        );
        assert_eq!(
            recovery["takeover_nudge"],
            cccc_core::peer_insight::BOOTSTRAP_TAKEOVER_NUDGE
        );
        assert_eq!(
            pack["context_hygiene"]["execution_health"]["status"],
            "ready"
        );
        assert_eq!(
            pack["context_hygiene"]["mind_context_health"]["status"],
            "ready"
        );
        assert!(estimate_tokens(&pack) <= RECOVERY_TOKEN_BUDGET);
    }

    #[test]
    fn inbox_preview_is_bounded_and_mail_only() {
        let inbox = json!({"messages": [
            {"id":"e1","ts":"now","by":"user","kind":"chat.message","data":{"text":"read later","message_mode":"mail"}},
            {"id":"e2","ts":"now","by":"peer","kind":"chat.message","data":{"text":"another mail","message_mode":"mail"}}
        ]})
        .as_object()
        .cloned()
        .expect("inbox");

        let preview = build_inbox_preview(&inbox, 1);
        assert_eq!(preview["messages"].as_array().map(Vec::len), Some(1));
        assert_eq!(preview["messages"][0]["text_preview"], "read later");
        assert_eq!(preview["messages"][0]["message_mode"], "mail");
        assert!(preview["messages"][0].get("reply_requested").is_none());
        assert_eq!(preview["truncated"], true);
    }

    #[test]
    fn recall_query_prioritizes_live_execution_state() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 9, 5, 1, 0)
            .single()
            .expect("timestamp");
        let pack = build_recovery_pack(&fixture_context(), "peer1", &Map::new(), now);
        let query = memory_recall_query(&pack);

        assert!(query.starts_with("bootstrap semantic conformance | compare packets"));
        assert!(query.chars().count() <= 240);
    }

    #[test]
    fn public_payload_has_only_the_six_cold_start_sections() {
        let payload = assemble_payload(
            json!({}),
            json!({}),
            json!({"messages": [], "truncated": false}),
            json!({}),
            json!({"required": true, "status": "empty", "query": "q", "hits": []}),
        );
        let mut keys = payload
            .as_object()
            .expect("payload")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "context_hygiene",
                "inbox_preview",
                "memory_recall_gate",
                "next_calls",
                "recovery",
                "session",
            ]
        );
        assert!(payload.get("group").is_none());
        assert!(payload.get("context").is_none());
    }
}
