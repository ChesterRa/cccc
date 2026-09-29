use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

/// Known interactive prompts that leave a live-looking runtime blocked on
/// input no scheduled turn will ever provide (Claude folder trust, MCP
/// server trust, generic confirmations). Matched case-insensitively against
/// the rendered terminal tail; each entry is a name plus alternative needles.
const DIALOG_PROMPTS: &[(&str, &[&str])] = &[
    ("folder_trust", &["trust the files in this folder"]),
    (
        "mcp_server_trust",
        &[
            "new mcp servers",
            "mcp servers found",
            "trust these servers",
            "servers in .mcp.json",
        ],
    ),
    (
        "confirm_prompt",
        &["press enter to confirm", "enter to confirm"],
    ),
];

pub fn runtime_actor_fields(
    home: &HomeLayout,
    actor: &Actor,
    group: &GroupDoc,
    running: bool,
) -> Map<String, Value> {
    let runner_effective = if super::actor_runtime::is_structured(actor) {
        "headless"
    } else {
        "pty"
    };
    fields(home, actor, group, running, runner_effective)
}

pub(super) fn fields(
    home: &HomeLayout,
    actor: &Actor,
    group: &GroupDoc,
    running: bool,
    runner_effective: &str,
) -> Map<String, Value> {
    let group_id = &group.group_id;
    let managed_session = super::local_headless::running(group_id, &actor.id)
        || super::local_headless::uses_managed_session(actor);
    let local_state = running
        .then(|| super::local_headless::status(group_id, &actor.id))
        .flatten();
    let (state, reason, updated_at, active_task_id) = if !running {
        (
            "stopped".to_owned(),
            "runner_not_running".to_owned(),
            None,
            None,
        )
    } else if let Some(local_state) = local_state {
        (
            local_state.status,
            if runner_effective == "pty" {
                "managed_agent_session".to_owned()
            } else {
                "provider_headless_session".to_owned()
            },
            Some(local_state.updated_at),
            local_state.task_id,
        )
    } else if managed_session {
        (
            "waiting".to_owned(),
            "managed_agent_session_pending".to_owned(),
            None,
            None,
        )
    } else if runner_effective == "headless" {
        ("idle".to_owned(), "headless_running".to_owned(), None, None)
    } else {
        (
            "waiting".to_owned(),
            "pty_running_state_unknown".to_owned(),
            None,
            None,
        )
    };

    let progress_ago = running
        .then(|| last_progress_ago(home, group, &actor.id))
        .flatten();
    let blocked = (running && matches!(state.as_str(), "waiting" | "idle"))
        .then(|| blocked_dialog(group, &actor.id))
        .flatten();
    let (state, reason) = match blocked {
        Some(pattern) => (
            "blocked_on_dialog".to_owned(),
            format!("interactive_prompt:{pattern}"),
        ),
        None => (state, reason),
    };

    Map::from_iter([
        ("idle_seconds".into(), Value::Null),
        ("runner_effective".into(), json!(runner_effective)),
        ("effective_working_state".into(), json!(state)),
        ("effective_working_reason".into(), json!(reason)),
        ("effective_working_updated_at".into(), json!(updated_at)),
        ("effective_active_task_id".into(), json!(active_task_id)),
        ("blocked_dialog_prompt".into(), json!(blocked)),
        ("last_progress_ago_seconds".into(), json!(progress_ago)),
    ])
}

fn last_progress_ago(home: &HomeLayout, group: &GroupDoc, actor_id: &str) -> Option<i64> {
    let contexts = cccc_core::context::ContextStore::new(home.clone()).ok()?;
    let context = contexts.load(&group.group_id).ok()?;
    let active_task_id = context
        .agent_states
        .get(actor_id)
        .and_then(|state| state.get("hot"))
        .and_then(|hot| hot.get("active_task_id"))
        .and_then(Value::as_str)?;
    let task = context
        .tasks
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(active_task_id))?;
    let progress = cccc_core::automation::task_last_progress_at(task, &context)?;
    Some(chrono::Utc::now().timestamp().saturating_sub(progress))
}

fn blocked_dialog(group: &GroupDoc, actor_id: &str) -> Option<&'static str> {
    let page = cccc_runtime::retained_history_tail(&group.group_id, actor_id, 4_096).ok()?;
    if page.data.is_empty() {
        return None;
    }
    let text = super::terminal_text::render(&page.data, true).to_lowercase();
    for (name, needles) in DIALOG_PROMPTS {
        if dialog_allowed(group, name) || needles.iter().any(|needle| dialog_allowed(group, needle))
        {
            continue;
        }
        if needles.iter().any(|needle| text.contains(needle)) {
            return Some(name);
        }
    }
    None
}

fn dialog_allowed(group: &GroupDoc, entry: &str) -> bool {
    group
        .extra
        .get("dialog_allowlist")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|allowed| allowed.eq_ignore_ascii_case(entry))
}
