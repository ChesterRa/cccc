use cccc_contracts::{ActorRole, DaemonRequest, Event};
use cccc_core::context::{ContextDoc, ContextStore};
use cccc_core::{HomeLayout, actors, ledger};
use serde_json::{Map, Value, json};

use crate::dispatch::{
    OpError, OpResult, first_non_blank_arg, object, required_arg, store, string_arg,
};

pub(super) fn handle(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?.trim().to_owned();
    let by = first_non_blank_arg(request, &["by"]).unwrap_or_else(|| "user".into());
    let text = string_arg(request, "text")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let title = first_non_blank_arg(request, &["title"]).unwrap_or_else(|| compact(&text, 120));
    if title.is_empty() {
        return Err(OpError::new(
            "missing_title",
            "tracked_send requires a title or non-empty text",
        ));
    }
    if text.trim().is_empty() {
        return Err(OpError::new(
            "empty_message",
            "tracked_send message text cannot be empty",
        ));
    }
    let message_priority = first_non_blank_arg(request, &["message_priority", "priority"])
        .unwrap_or_else(|| "normal".into());
    if !matches!(message_priority.as_str(), "normal" | "attention") {
        return Err(OpError::new(
            "invalid_priority",
            "priority must be 'normal' or 'attention'",
        ));
    }

    let mut normalized_request = request.clone();
    normalized_request
        .args
        .insert("group_id".into(), json!(group_id));
    normalized_request.args.insert("by".into(), json!(by));
    normalized_request.args.insert("text".into(), json!(text));
    normalized_request
        .args
        .insert("message_priority".into(), json!(message_priority));
    normalized_request.args.insert(
        "to".into(),
        Value::Array(normalize_to(request.args.get("to"))),
    );
    normalized_request.args.insert(
        "refs".into(),
        Value::Array(normalize_refs(request.args.get("refs"))),
    );
    let request = &normalized_request;
    let group = super::load(home, request)?;
    let key =
        first_non_blank_arg(request, &["idempotency_key", "client_request_id"]).unwrap_or_default();
    let client_id = if key.is_empty() {
        String::new()
    } else {
        super::super::message_idempotency::tracked_client_id(&group_id, &by, &key)
    };
    if !client_id.is_empty()
        && let Some(event) = super::super::message_idempotency::find(
            home,
            &group_id,
            "chat.message",
            &by,
            &Map::from_iter([("client_id".into(), json!(client_id))]),
        )
    {
        return replayed(event);
    }
    // Match the ordinary send gate before creating a durable task. This keeps
    // recipient, scope, attachment, and peer-insight failures side-effect free.
    let preflight = message_request(request, &client_id);
    let mut data: Map<String, Value> = preflight
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    super::message_validation::normalize(home, &group, &mut data)?;
    super::super::messaging_recipients::normalize_chat_data(&group, &by, &mut data)?;
    let task_assignee = assignee(request);
    authorize_task_create(&group, &by, &task_assignee)?;

    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let document = contexts.load(&group_id).map_err(OpError::io)?;
    let existing = (!client_id.is_empty())
        .then(|| find_task(&document, &client_id))
        .flatten()
        .cloned();
    let (task, context_result, task_created) = if let Some(task) = existing {
        (task, Value::Null, false)
    } else {
        let operation = task_operation(request, &title, &text, &client_id);
        let result = contexts
            .sync(&group_id, &[operation], None, &by, false)
            .map_err(OpError::invalid)?;
        append_context_event(home, &group_id, &by, &result)?;
        let task = if client_id.is_empty() {
            result.context.tasks.last()
        } else {
            find_task(&result.context, &client_id)
        }
        .cloned()
        .ok_or_else(|| {
            OpError::new(
                "tracked_send_task_missing",
                "task.create succeeded but did not return a task_id",
            )
        })?;
        (
            task,
            serde_json::to_value(&result).unwrap_or(Value::Null),
            true,
        )
    };
    let task_id = task
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let task_ref = task_reference(&task);
    let mut forwarded = message_request(request, &client_id);
    let mut refs = forwarded
        .args
        .remove("refs")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    refs.push(task_ref.clone());
    forwarded.args.insert("refs".into(), Value::Array(refs));

    match super::send(home, &forwarded, "chat.message") {
        Ok(sent) => {
            let event = sent.get("event").cloned().unwrap_or(Value::Null);
            object(json!({
                "task_id":task_id,
                "task_ref":task_ref,
                "context_result":context_result,
                "event_id":event.get("id").and_then(Value::as_str).unwrap_or_default(),
                "event":event,
                "delivery":sent.get("delivery"),
                "task_created":task_created,
                "message_sent":true,
                "partial_failure":false,
                "replayed":false,
                "recovered_from_partial_failure":!task_created,
            }))
        }
        Err(error) => object(json!({
            "task_id":task_id,
            "task_ref":task_ref,
            "context_result":context_result,
            "task_created":task_created,
            "message_sent":false,
            "partial_failure":true,
            "message_error":{"code":error.code,"message":error.message,"details":error.details},
            "recovered_from_partial_failure":false,
        })),
    }
}

fn task_operation(
    request: &DaemonRequest,
    title: &str,
    text: &str,
    client_id: &str,
) -> Map<String, Value> {
    let mut task = Map::from_iter([
        ("op".into(), json!("task.create")),
        ("title".into(), json!(title)),
        (
            "outcome".into(),
            json!(
                first_non_blank_arg(request, &["outcome", "goal"]).unwrap_or_else(|| text.into())
            ),
        ),
        (
            "status".into(),
            json!(first_non_blank_arg(request, &["status"]).unwrap_or_else(|| "planned".into())),
        ),
        (
            "priority".into(),
            json!(
                first_non_blank_arg(request, &["task_priority", "message_priority"])
                    .unwrap_or_else(|| "normal".into())
            ),
        ),
        (
            "waiting_on".into(),
            json!(
                first_non_blank_arg(request, &["waiting_on"]).unwrap_or_else(|| {
                    if assignee(request).is_empty() {
                        "none".into()
                    } else {
                        "actor".into()
                    }
                })
            ),
        ),
        (
            "task_type".into(),
            json!(
                first_non_blank_arg(request, &["task_type"]).unwrap_or_else(|| "standard".into())
            ),
        ),
    ]);
    if !client_id.is_empty() {
        task.insert("client_request_id".into(), json!(client_id));
    }
    for key in ["notes", "handoff_to"] {
        if let Some(value) = first_non_blank_arg(request, &[key]) {
            task.insert(key.into(), json!(value));
        }
    }
    if let Some(value) = request
        .args
        .get("blocked_by")
        .filter(|value| !value.is_null())
    {
        task.insert("blocked_by".into(), value.clone());
    }
    if let Some(checklist) = normalize_checklist(request.args.get("checklist")) {
        task.insert("checklist".into(), checklist);
    }
    let assignee = assignee(request);
    if !assignee.is_empty() {
        task.insert("assignee".into(), json!(assignee));
    }
    task
}

fn assignee(request: &DaemonRequest) -> String {
    if let Some(value) = first_non_blank_arg(request, &["assignee"]) {
        return value;
    }
    let recipients = request
        .args
        .get("to")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    match recipients.as_slice() {
        [only] if !only.starts_with('@') && *only != "user" => (*only).into(),
        _ => String::new(),
    }
}

fn message_request(request: &DaemonRequest, client_id: &str) -> DaemonRequest {
    let mut args = Map::new();
    for key in [
        "group_id",
        "by",
        "text",
        "to",
        "path",
        "format",
        "refs",
        "attachments",
        "insight",
        "require_peer_insight",
        "suggested_user_message",
    ] {
        if let Some(value) = request.args.get(key) {
            args.insert(key.into(), value.clone());
        }
    }
    args.insert(
        "priority".into(),
        request
            .args
            .get("message_priority")
            .or_else(|| request.args.get("priority"))
            .cloned()
            .unwrap_or_else(|| json!("normal")),
    );
    args.insert(
        "reply_required".into(),
        request
            .args
            .get("reply_required")
            .cloned()
            .unwrap_or(Value::Bool(true)),
    );
    if !client_id.is_empty() {
        args.insert("client_id".into(), json!(client_id));
    }
    DaemonRequest {
        v: request.v,
        op: "send".into(),
        args,
    }
}

fn normalize_to(raw: Option<&Value>) -> Vec<Value> {
    match raw {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| json!(value))
            .collect(),
        Some(Value::String(value)) if !value.trim().is_empty() => vec![json!(value.trim())],
        _ => Vec::new(),
    }
}

fn normalize_refs(raw: Option<&Value>) -> Vec<Value> {
    raw.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.is_object())
        .cloned()
        .collect()
}

fn authorize_task_create(
    group: &cccc_core::GroupDoc,
    by: &str,
    assignee: &str,
) -> Result<(), OpError> {
    if matches!(by, "user" | "system")
        || actors::effective_role(group, by) == Some(ActorRole::Foreman)
        || assignee.is_empty()
        || assignee == by
    {
        return Ok(());
    }
    Err(OpError::new(
        "context_sync_error",
        format!("Permission denied: peer cannot create task assigned to {assignee}"),
    ))
}

fn normalize_checklist(raw: Option<&Value>) -> Option<Value> {
    match raw? {
        Value::Array(items) => Some(Value::Array(
            items
                .iter()
                .filter_map(|item| match item {
                    Value::Object(object) => {
                        let text = object.get("text")?.as_str()?.trim();
                        if text.is_empty() {
                            None
                        } else {
                            let mut normalized = object.clone();
                            normalized.insert("text".into(), json!(text));
                            Some(Value::Object(normalized))
                        }
                    }
                    other => {
                        let text = value_text(other);
                        (!text.is_empty()).then(|| json!({"text":text}))
                    }
                })
                .collect(),
        )),
        other => {
            let text = value_text(other);
            let items = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(|line| json!({"text":line}))
                .collect::<Vec<_>>();
            (!items.is_empty()).then_some(Value::Array(items))
        }
    }
}

fn value_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| match value {
            Value::Null => String::new(),
            _ => value.to_string(),
        })
        .trim()
        .to_owned()
}

fn find_task<'a>(document: &'a ContextDoc, client_id: &str) -> Option<&'a Map<String, Value>> {
    document
        .tasks
        .iter()
        .rev()
        .find(|task| task.get("client_request_id").and_then(Value::as_str) == Some(client_id))
}

fn task_reference(task: &Map<String, Value>) -> Value {
    json!({
        "kind":"task_ref",
        "task_id":task.get("id").and_then(Value::as_str).unwrap_or_default(),
        "title":task.get("title").and_then(Value::as_str).unwrap_or_default(),
        "status":task.get("status").and_then(Value::as_str).unwrap_or("planned"),
        "waiting_on":task.get("waiting_on").and_then(Value::as_str).unwrap_or("none"),
        "handoff_to":task.get("handoff_to").and_then(Value::as_str).unwrap_or_default(),
    })
}

fn append_context_event(
    home: &HomeLayout,
    group_id: &str,
    by: &str,
    result: &cccc_core::context::ContextSyncResult,
) -> Result<(), OpError> {
    let mut event = Event::new("context.sync", group_id);
    event.by = by.into();
    event.data = json!({"version":result.version,"changes":result.changes})
        .as_object()
        .cloned()
        .unwrap_or_default();
    ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)
}

fn replayed(event: Event) -> OpResult {
    let task_ref = event
        .data
        .get("refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|item| item.get("kind").and_then(Value::as_str) == Some("task_ref"))
        .cloned()
        .unwrap_or(Value::Null);
    object(json!({
        "task_id":task_ref.get("task_id").and_then(Value::as_str).unwrap_or_default(),
        "task_ref":task_ref,
        "event_id":event.id,
        "event":event,
        "task_created":false,
        "message_sent":true,
        "partial_failure":false,
        "replayed":true,
    }))
}

fn compact(text: &str, limit: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}
