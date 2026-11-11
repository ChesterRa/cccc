use cccc_client::DaemonClient;
use cccc_core::{GroupDoc, HomeLayout, group_scope};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs;

use crate::ToolCallError;
use crate::router::daemon;

const RECOVERY_TOKEN_BUDGET: usize = 1_100;
const MAX_RECOVERY_CONSTRAINTS: usize = 6;
const STALE_AFTER_SECONDS: i64 = 20 * 60;
const MIND_HOT_ONLY_UPDATE_THRESHOLD: u64 = 3;
const MAX_LISTED_SKILLS: usize = 8;

pub(crate) async fn build(
    home: &HomeLayout,
    client: &DaemonClient,
    args: Map<String, Value>,
) -> Result<Value, ToolCallError> {
    crate::local_tools::revalidate(home, &args)?;
    let group_id = string_value(args.get("group_id"))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "group_id is required".to_owned())?;
    // actor_id can be a model-selected target on other tools. Bootstrap always
    // reports and reads the authenticated caller, just like local execution.
    let actor_id = string_value(args.get("by"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "user".into());
    let binding = args.get("_cccc_web_binding");
    let native_binding = args.get("_cccc_native_binding");
    let inbox_limit = args
        .get("inbox_limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 200) as usize;
    let group_result = daemon(
        client,
        "group_show",
        request_args(&group_id, &actor_id, binding, native_binding),
    )
    .await?;
    let group: GroupDoc = serde_json::from_value(
        group_result
            .get("group")
            .cloned()
            .ok_or("group_show returned no group")?,
    )
    .map_err(|error| format!("group_show returned an invalid group: {error}"))?;

    let actors_result = daemon(
        client,
        "actor_list",
        request_args(&group_id, &actor_id, binding, native_binding),
    )
    .await?;
    let actors = actors_result
        .get("actors")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    let context = daemon(
        client,
        "context_get",
        request_args(&group_id, &actor_id, binding, native_binding),
    )
    .await?;

    let mut inbox_args = request_args(&group_id, &actor_id, binding, native_binding);
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

    let mut payload = assemble_payload(
        build_session(
            &group,
            actors,
            &actor_id,
            group_result
                .get("effective_scope")
                .cloned()
                .unwrap_or_else(|| json!({})),
        ),
        build_recovery(&recovery_pack),
        build_inbox_preview(&inbox, inbox_limit),
        context_hygiene,
    );
    if native_binding.is_some() {
        name_group_in_examples(&mut payload, &group_id);
    }
    if let Ok(store) = cccc_core::GroupStore::new(home.clone())
        && let Ok(group) = store.load(&group_id)
        && let Ok(Some(pending)) = cccc_core::inbox::mail_pending_summary(home, &group, &actor_id)
    {
        payload["mail_pending"] = pending;
    }
    // Bootstrap must survive a failed read; the agent only misses the list of skills.
    let capabilities = daemon(
        client,
        "capability_state",
        request_args(&group_id, &actor_id, binding, native_binding),
    )
    .await
    .unwrap_or_default();
    if let Some(skills) = available_skills(&capabilities) {
        payload["available_skills"] = skills;
    }
    crate::local_tools::revalidate(home, &args)?;
    Ok(payload)
}

/// The skills this agent may use, assigned ones first. An assigned skill counts before
/// the agent restarts, since using it enables it. The agent is told only that they
/// exist: their rules can be long, and most tasks need none of them.
fn available_skills(capabilities: &Map<String, Value>) -> Option<Value> {
    let mut seen = std::collections::BTreeSet::new();
    let skills = ["autoload_skills", "active_capsule_skills"]
        .iter()
        .filter_map(|key| capabilities.get(*key).and_then(Value::as_array))
        .flatten()
        .filter(|skill| seen.insert(skill["capability_id"].to_string()))
        .collect::<Vec<_>>();
    if skills.is_empty() {
        return None;
    }
    let listed = skills
        .iter()
        .take(MAX_LISTED_SKILLS)
        .map(|skill| {
            let description = skill["description_short"].as_str().unwrap_or_default();
            json!({
                "capability_id": skill["capability_id"],
                "name": skill["name"],
                "description": description.chars().take(160).collect::<String>(),
            })
        })
        .collect::<Vec<_>>();
    let mut result = json!({
        "skills": listed,
        "note": "For the tasks they fit. cccc_capability_use(capability_id=...) returns a skill's rules in capsule_text.",
    });
    if skills.len() > MAX_LISTED_SKILLS {
        result["omitted"] = json!({
            "count": skills.len() - MAX_LISTED_SKILLS,
            "list": "cccc_capability_use(tool_name=\"cccc_capability_state\")",
        });
    }
    Some(result)
}

fn assemble_payload(
    session: Value,
    recovery: Value,
    inbox_preview: Value,
    context_hygiene: Value,
) -> Value {
    json!({
        "session": session,
        "recovery": recovery,
        "inbox_preview": inbox_preview,
        "context_hygiene": context_hygiene,
        "next_calls": {
            "help": "cccc_help()  # when a CCCC route or state boundary is unclear",
            "context_get": "cccc_context_get(detail=\"overview\")  # brief, recent decisions and handoffs, agent states",
            "tasks": "cccc_task()  # open tasks, one row each; pass task_id to read one in full",
            "connect": "cccc_connect()  # discover accessible remote Groups/Actors; use both instance_id and target_group_id for an external_groups entry",
            "inbox_read": "cccc_inbox_read()",
        }
    })
}

/// A native session has no Group of its own to fall back on, so the calls
/// bootstrap suggests to it name the Group.
fn name_group_in_examples(payload: &mut Value, group_id: &str) {
    let name_group = |example: &mut Value| {
        if let Some((tool, arguments)) = example.as_str().and_then(|call| call.split_once('(')) {
            let separator = if arguments.starts_with(')') { "" } else { ", " };
            *example = json!(format!(
                "{tool}(group_id=\"{group_id}\"{separator}{arguments}"
            ));
        }
    };
    if let Some(calls) = payload["next_calls"].as_object_mut() {
        calls.values_mut().for_each(name_group);
    }
    for key in [
        "update_command",
        "execution_update_command",
        "mind_context_update_command",
    ] {
        if let Some(example) = payload["context_hygiene"].get_mut(key) {
            name_group(example);
        }
    }
}

fn request_args(
    group_id: &str,
    actor_id: &str,
    binding: Option<&Value>,
    native_binding: Option<&Value>,
) -> Map<String, Value> {
    let mut args = Map::from_iter([
        ("group_id".into(), Value::String(group_id.to_owned())),
        ("actor_id".into(), Value::String(actor_id.to_owned())),
        ("by".into(), Value::String(actor_id.to_owned())),
    ]);
    if let Some(binding) = binding {
        args.insert("_cccc_web_binding".into(), binding.clone());
    }
    if let Some(binding) = native_binding {
        args.insert("_cccc_native_binding".into(), binding.clone());
    }
    args
}

fn build_session(
    group: &GroupDoc,
    actors: &[Value],
    actor_id: &str,
    effective_scope: Value,
) -> Value {
    let actor = actors
        .iter()
        .filter_map(Value::as_object)
        .find(|actor| actor.get("id").and_then(Value::as_str) == Some(actor_id));
    let active_scope = group_scope::effective_scope(group, None).map_or_else(
        || json!({}),
        |scope| {
            json!({
                "scope_key": scope.scope_key,
                "path": cccc_runtime::compatible_path(std::path::Path::new(&scope.url)),
            })
        },
    );
    let members = actors
        .iter()
        .map(|member| {
            let mut summary = json!({
                "actor_id": member["id"],
                "runtime": member["runtime"],
                "role": member["role"],
                "enabled": member["enabled"],
            });
            if let Some(source_id) = member
                .get("external_session_id")
                .and_then(Value::as_str)
                .filter(|source_id| !source_id.is_empty())
            {
                summary["source_id"] = json!(source_id);
            }
            summary
        })
        .collect::<Vec<_>>();

    json!({
        "group_id": group.group_id,
        "group_title": if group.title.trim().is_empty() { &group.group_id } else { group.title.trim() },
        "actor_id": actor_id,
        "role": actor.and_then(|item| string_value(item.get("role"))).unwrap_or_default(),
        "runner": actor.and_then(|item| string_value(item.get("runner"))).unwrap_or_default(),
        "members": members,
        "active_scope": active_scope,
        "effective_scope": effective_scope,
    })
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

    let pack = json!({
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
            (
                "constraints".into(),
                Value::Array(Vec::new()),
            ),
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
    fit_constraints(pack, brief.and_then(|value| value.get("constraints")))
}

/// Standing rules outrank the rest of the pack, which is trimmed to keep them.
/// Rules that still do not fit leave whole, behind an explicit omission notice:
/// never turn a partial constraint into an apparently complete instruction.
fn fit_constraints(mut pack: Value, constraints: Option<&Value>) -> Value {
    for count in (1..=MAX_RECOVERY_CONSTRAINTS).rev() {
        let mut candidate = pack.clone();
        set_constraints(&mut candidate, full_constraints(constraints, count));
        shrink_recovery_pack(&mut candidate);
        if estimate_tokens(&candidate) <= RECOVERY_TOKEN_BUDGET {
            return candidate;
        }
    }
    set_constraints(&mut pack, full_constraints(constraints, 0));
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
        } else if cccc_core::context::task_is_blocked(task) {
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
    let mut result = clean_object(Map::from_iter([
        ("id".into(), trimmed_value(task.get("id"), 64)),
        (
            "rev".into(),
            task.get("rev").cloned().unwrap_or(Value::Null),
        ),
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
    ]));
    if task.contains_key("open_blocked_by") {
        result.insert(
            "open_blocked_by".into(),
            Value::Array(trimmed_list(task.get("open_blocked_by"), 4, 64)),
        );
    }
    Value::Object(result)
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
    let recovery = Map::from_iter([
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
    Value::Object(recovery)
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
        "update_command": "cccc_agent_state(action=\"update\", focus=\"...\", next_action=\"...\", what_changed=\"...\", environment_summary=\"...\", user_model=\"...\", persona_notes=\"...\")",
        "execution_update_command": "cccc_agent_state(action=\"update\", focus=\"...\", next_action=\"...\", what_changed=\"...\")",
        "mind_context_update_command": "cccc_agent_state(action=\"update\", environment_summary=\"...\", user_model=\"...\", persona_notes=\"...\")",
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

fn set_constraints(pack: &mut Value, items: Vec<Value>) {
    pack["coordination_brief"]["constraints"] = Value::Array(items);
}

fn full_constraints(value: Option<&Value>, max_items: usize) -> Vec<Value> {
    let mut items = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let text = string_value(Some(item)).unwrap_or_default();
            (!text.is_empty()).then_some(Value::String(text))
        })
        .collect::<Vec<_>>();
    if items.len() > max_items {
        let extra = items.len() - max_items;
        items.truncate(max_items);
        items.push(Value::String(format!(
            "({extra} constraints omitted; read the full list via cccc_context_get before acting)"
        )));
    }
    items
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

    #[test]
    fn session_lists_recipients_without_runtime_configuration() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group = cccc_core::GroupStore::new(home)
            .expect("groups")
            .create("Recipients", "")
            .expect("group");
        let members = [
            json!({"id":"lead","runtime":"codex","role":"foreman","enabled":true,
                "external_session_id":"codex:thread-a","env":{"FIXTURE":"private"},
                "command":["private-launcher"],"pid":123}),
            json!({"id":"peer","runtime":"claude","role":"peer","enabled":false,
                "external_session_id":"","env":{"FIXTURE":"private"}}),
        ];
        let session = build_session(&group, &members, "lead", json!({}));
        assert_eq!(session["actor_id"], "lead");
        assert_eq!(
            session["members"],
            json!([
                {"actor_id":"lead","runtime":"codex","role":"foreman","enabled":true,
                    "source_id":"codex:thread-a"},
                {"actor_id":"peer","runtime":"claude","role":"peer","enabled":false},
            ])
        );
    }

    #[cfg(windows)]
    #[test]
    fn session_shows_the_drive_path_of_a_verbatim_group_scope() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut group = cccc_core::GroupStore::new(home)
            .expect("groups")
            .create("Verbatim scope", "")
            .expect("group");
        group.scopes.push(cccc_core::Scope {
            scope_key: "scope_demo".into(),
            url: r"\\?\C:\work\demo".into(),
            label: "demo".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "scope_demo".into();
        let session = build_session(&group, &[], "lead", json!({}));
        assert_eq!(session["active_scope"]["path"], r"C:\work\demo");
    }

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
        assert!(recovery.get("takeover_nudge").is_none());
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
    fn brief_constraints_are_returned_in_full() {
        let long_rule = "r".repeat(400);
        let mut context = fixture_context();
        context
            .get_mut("coordination")
            .and_then(|value| value.pointer_mut("/brief"))
            .and_then(Value::as_object_mut)
            .expect("brief")
            .insert(
                "constraints".into(),
                json!([long_rule, "short rule", "a", "b", "c", "d", "e", "f"]),
            );
        let pack = build_recovery_pack(&context, "peer1", &Map::new(), Utc::now());
        let constraints = pack["coordination_brief"]["constraints"]
            .as_array()
            .expect("constraints array");
        assert_eq!(constraints[0].as_str().expect("first"), "r".repeat(400));
        assert_eq!(constraints.len(), 7);
        assert_eq!(
            constraints[6].as_str().expect("overflow marker"),
            "(2 constraints omitted; read the full list via cccc_context_get before acting)"
        );
    }

    #[test]
    fn standing_rules_outrank_the_rest_of_the_pack() {
        let rules = json!([
            "Only the user pushes",
            "Relay messages sent only to the foreman",
            "Do not restart the user's services",
            "GitHub actions are the user's",
            "A parked task stays parked",
        ]);
        // Many small notes sit just under the budget, as for a busy agent. Trimming
        // only for the omission notice would free too little for any rule.
        let note = json!({"summary": "n"});
        let mut pack = json!({"coordination_brief": {}, "recent_handoffs": []});
        while estimate_tokens(&pack) + estimate_tokens(&note) < RECOVERY_TOKEN_BUDGET {
            pack["recent_handoffs"]
                .as_array_mut()
                .expect("valid test fixture")
                .push(note.clone());
        }

        let pack = fit_constraints(pack, Some(&rules));

        assert_eq!(pack["coordination_brief"]["constraints"], rules);
        assert!(estimate_tokens(&pack) <= RECOVERY_TOKEN_BUDGET);
    }

    #[test]
    fn long_constraints_respect_budget_without_cutting_individual_rules() {
        for rule in ["r".repeat(1_200), "条件を守る。".repeat(400)] {
            let mut context = fixture_context();
            context.get_mut("coordination").expect("valid test fixture")["brief"]["constraints"] =
                json!(vec![rule.clone(); 8]);
            let pack = build_recovery_pack(&context, "peer1", &Map::new(), Utc::now());
            assert!(estimate_tokens(&pack) <= RECOVERY_TOKEN_BUDGET);
            let constraints = pack["coordination_brief"]["constraints"]
                .as_array()
                .expect("valid test fixture");
            assert!(
                constraints
                    .last()
                    .expect("valid test fixture")
                    .as_str()
                    .expect("valid test fixture")
                    .contains("cccc_context_get")
            );
            for item in &constraints[..constraints.len() - 1] {
                assert_eq!(item.as_str(), Some(rule.as_str()));
            }
            assert!(
                !pack["tasks"]["assigned_active"]
                    .as_array()
                    .expect("valid test fixture")
                    .is_empty()
            );
        }
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
    fn public_payload_has_only_the_five_cold_start_sections() {
        let payload = assemble_payload(
            json!({}),
            json!({}),
            json!({"messages": [], "truncated": false}),
            json!({}),
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
                "next_calls",
                "recovery",
                "session",
            ]
        );
        assert!(payload.get("group").is_none());
        assert!(payload.get("context").is_none());
    }

    #[test]
    fn available_skills_lists_assigned_ones_first_without_their_rules() {
        let skill = |id: &str| {
            json!({
                "capability_id": id,
                "name": id,
                "description_short": "d".repeat(200),
                "capsule_text": "rules",
                "capsule_preview": "rules",
            })
        };
        let state = json!({
            "autoload_skills": [skill("skill:assigned")],
            "active_capsule_skills": [skill("skill:group"), skill("skill:assigned")],
        });

        let result = available_skills(state.as_object().expect("state")).expect("skills");
        let skills = result["skills"].as_array().expect("list");

        assert_eq!(
            skills
                .iter()
                .map(|skill| skill["capability_id"].as_str().expect("id"))
                .collect::<Vec<_>>(),
            ["skill:assigned", "skill:group"]
        );
        assert_eq!(skills[0]["description"].as_str().expect("text").len(), 160);
        assert!(skills[0].get("capsule_text").is_none());
        assert!(skills[0].get("capsule_preview").is_none());
        assert!(result.get("omitted").is_none());
    }

    #[test]
    fn available_skills_says_how_many_it_left_out() {
        let many = (0..20)
            .map(|index| json!({"capability_id":format!("skill:{index}"),"name":"n"}))
            .collect::<Vec<_>>();
        let state = json!({"active_capsule_skills":many});

        let result = available_skills(state.as_object().expect("state")).expect("skills");

        assert_eq!(
            result["skills"].as_array().expect("list").len(),
            MAX_LISTED_SKILLS
        );
        assert_eq!(result["omitted"]["count"], 12);
        assert!(available_skills(&Map::new()).is_none());
    }

    #[test]
    fn a_native_session_gets_examples_that_name_its_group() {
        let hygiene = missing_hygiene("peer");
        for key in [
            "update_command",
            "execution_update_command",
            "mind_context_update_command",
        ] {
            let example = hygiene[key].as_str().expect("example");
            assert!(
                example.starts_with("cccc_agent_state(action=\"update\", "),
                "{example}"
            );
            // These self-update examples use the caller identity and need no actor_id argument.
            assert!(!example.contains("actor_id"), "{example}");
        }
        let managed = assemble_payload(json!({}), json!({}), json!({}), hygiene);
        assert_eq!(managed["next_calls"]["inbox_read"], "cccc_inbox_read()");
        let mut native = managed.clone();
        name_group_in_examples(&mut native, "g_fixture");
        assert_eq!(
            native["next_calls"]["inbox_read"],
            "cccc_inbox_read(group_id=\"g_fixture\")"
        );
        assert_eq!(
            native["next_calls"]["context_get"],
            "cccc_context_get(group_id=\"g_fixture\", detail=\"overview\")  # brief, recent decisions and handoffs, agent states"
        );
        assert_eq!(
            native["context_hygiene"]["execution_update_command"],
            "cccc_agent_state(group_id=\"g_fixture\", action=\"update\", focus=\"...\", next_action=\"...\", what_changed=\"...\")"
        );
        for (key, example) in native["next_calls"].as_object().expect("next calls") {
            let example = example.as_str().expect("example");
            assert!(
                example.contains("(group_id=\"g_fixture\""),
                "{key}: {example}"
            );
        }
        assert_eq!(native["session"], managed["session"]);
    }

    #[test]
    fn next_calls_point_at_the_compact_context_and_task_reads() {
        let payload = assemble_payload(json!({}), json!({}), json!({}), json!({}));
        let calls = &payload["next_calls"];
        assert!(
            calls["context_get"]
                .as_str()
                .expect("context_get")
                .starts_with("cccc_context_get(detail=\"overview\")")
        );
        assert!(
            calls["tasks"]
                .as_str()
                .expect("tasks")
                .starts_with("cccc_task()")
        );
    }

    #[test]
    fn bootstrap_task_preview_keeps_the_recorded_task_revision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        let group = groups.create("task preview", "").expect("group");
        let directory = groups
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("context/tasks");
        let tasks: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../cccc-core/tests/fixtures/task-revision/tasks.json"
        ))
        .expect("recording");
        for task in tasks {
            cccc_core::fs::write_yaml(
                &directory.join(format!("{}.yaml", task["id"].as_str().expect("id"))),
                &task,
            )
            .expect("recorded task");
        }
        let loaded = cccc_core::context::ContextStore::new(home)
            .expect("contexts")
            .load(&group.group_id)
            .expect("read");
        assert_eq!(slim_task(&loaded.tasks[0])["rev"], 0);
    }
    #[test]
    fn completed_dependencies_do_not_enter_the_blocked_recovery_slice() {
        let done = json!({"id":"T2","status":"planned","blocked_by":["T1"],"open_blocked_by":[]});
        let missing =
            json!({"id":"T3","status":"planned","blocked_by":["T99"],"open_blocked_by":["T99"]});
        let (_, attention) = task_slice(&[done.clone(), missing], "peer");
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0]["id"], "T3");
        assert_eq!(
            slim_task(done.as_object().expect("task"))["open_blocked_by"],
            json!([])
        );
    }
}
