use super::*;
use crate::ops::assistants;

fn owned_task(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<(SecretaryTaskStore, SecretaryTask), OpError> {
    configuration::require_user(request)?;
    let group_id = required_arg(request, "group_id")?;
    let store = task_store(home);
    let task = store
        .load(&required_arg(request, "task_id")?)
        .map_err(OpError::not_found)?;
    if task.target.group_id != group_id {
        return Err(OpError::new(
            "permission_denied",
            "Task belongs to another Group",
        ));
    }
    Ok((store, task))
}

pub(super) fn handoff(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (store, task) = owned_task(home, request)?;
    if !task.cleanup_confirmed
        || task.phase != SecretaryTaskPhase::Done
        || task.target.kind != SecretaryTaskKind::Ask
    {
        return Err(OpError::new(
            "secretary_task_busy",
            "Wait for a confirmed Ask result before forwarding",
        ));
    }
    assistants::secretary_validate_request(home, &task).map_err(OpError::io)?;
    let proposal = task
        .receipt
        .as_ref()
        .ok_or_else(|| OpError::new("not_found", "Handoff proposal is missing"))?;
    let target = proposal.output["handoff_target"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| OpError::new("not_found", "Handoff proposal is missing"))?;
    let text = proposal.output["handoff_text"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| OpError::new("not_found", "Handoff text is missing"))?;
    let forwarded = DaemonRequest {
        v: 1,
        op: "voice_secretary_task_handoff".into(),
        args: json!({
            "group_id": task.target.group_id,
            "scope_key": task.target.scope_key,
            "by": "user",
            "target": target,
            "text": text,
            "secretary_task_id": task.task_id,
        })
        .as_object()
        .expect("args")
        .clone(),
    };
    let result = assistants::confirm_secretary_handoff(home, &forwarded)?;
    let event_id = result["notify_event"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    store
        .update(&task.task_id, |task| {
            task.forwarded_event_id = event_id;
            Ok(())
        })
        .map_err(OpError::io)?;
    Ok(result)
}

pub(super) fn candidate(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (store, task) = owned_task(home, request)?;
    if task.target.kind != SecretaryTaskKind::Document
        || task.phase.executing()
        || !task.cleanup_confirmed
    {
        return Err(OpError::new(
            "secretary_candidate_unavailable",
            "Wait for document task cleanup before reviewing its candidate",
        ));
    }
    object(
        json!({"task_id":task.task_id,"target":task.target,"content":store.candidate(&task.task_id).map_err(OpError::io)?,"base_version":task.base_version}),
    )
}

/// Explicit user continuation creates a new task and grant in the shared session. The original
/// receipt and candidate remain immutable; uncertain work is never auto-retried.
pub(super) fn retry(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (store, old) = owned_task(home, request)?;
    if !old.cleanup_confirmed || old.phase.executing() || old.phase == SecretaryTaskPhase::Queued {
        return Err(OpError::new(
            "secretary_task_busy",
            "Cancel the active task and wait for cleanup before retrying",
        ));
    }
    if old.phase == SecretaryTaskPhase::Unconfirmed
        && !crate::dispatch::bool_arg(request, "confirm_unconfirmed", false)
    {
        return Err(OpError::new(
            "secretary_unconfirmed_requires_confirmation",
            "The previous task may have run. Review its outcome before explicitly retrying.",
        ));
    }
    if old.phase == SecretaryTaskPhase::Done {
        if !old.projected_at.is_empty() && old.projection_error.is_empty() {
            return object(json!({"task":projection(&old)}));
        }
        assistants::secretary_project_result(home, &old).map_err(OpError::io)?;
        store
            .update(&old.task_id, |t| {
                t.projected_at = cccc_contracts::utc_now();
                t.projection_error.clear();
                Ok(())
            })
            .map_err(OpError::io)?;
        return object(json!({"task":projection(&store.load(&old.task_id).map_err(OpError::io)?)}));
    }
    let followup = string_arg(request, "followup")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if followup.chars().count() > 8000 {
        return Err(OpError::new(
            "invalid_args",
            "followup is limited to 8,000 characters",
        ));
    }
    if old.phase == SecretaryTaskPhase::NeedsUser && followup.is_empty() {
        return Err(OpError::new(
            "invalid_args",
            "Provide the requested information before continuing",
        ));
    }
    let mut next = SecretaryTask::new(old.target.clone(), old.inputs.clone());
    next.previous_task_id = old.task_id.clone();
    // Other Groups may run between these turns. Carry this request's confirmed
    // dialogue so a short answer does not depend on the shared conversation history.
    let question = old
        .receipt
        .as_ref()
        .filter(|r| r.status == SecretaryTaskPhase::NeedsUser)
        .and_then(|r| r.output["reply_text"].as_str())
        .unwrap_or_default();
    let mut dialogue = Vec::new();
    if !old.followup.is_empty() {
        dialogue.push(old.followup.clone());
    }
    if !question.is_empty() {
        dialogue.push(format!("Secretary clarification (quoted):\n{question}"));
    }
    if !followup.is_empty() {
        dialogue.push(format!("User clarification:\n{followup}"));
    }
    next.followup = dialogue.join("\n\n");
    if next.followup.chars().count() > 32_000 {
        return Err(OpError::new(
            "invalid_args",
            "Accumulated clarification exceeds 32,000 characters; start a new request with a concise summary",
        ));
    }
    let manager = lookup(home)
        .ok_or_else(|| OpError::new("secretary_unavailable", "Secretary owner is unavailable"))?;
    if manager.closing.load(Ordering::Acquire) {
        return Err(OpError::new("secretary_stopping", "Secretary is stopping"));
    }
    let _acceptance = manager.acceptance.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = store.successor(&old.task_id).map_err(OpError::io)? {
        if existing.followup != next.followup {
            return Err(OpError::new(
                "secretary_continuation_changed",
                "This task already has a continuation with different information; review that task or submit a new request",
            ));
        }
        return object(json!({"task":projection(&existing)}));
    }
    if store.queued().map_err(OpError::io)?.len() >= MAX_QUEUED_TASKS {
        return Err(OpError::new(
            "secretary_queue_full",
            "Secretary queue is full; the original input is retained",
        ));
    }
    assistants::secretary_validate_request(home, &old).map_err(OpError::invalid)?;
    next.guidance =
        assistants::secretary_guidance(home, &old.target.group_id).map_err(OpError::io)?;
    store.create(&next).map_err(OpError::io)?;
    store
        .update(&old.task_id, |t| {
            t.superseded_by = next.task_id.clone();
            Ok(())
        })
        .map_err(OpError::io)?;
    manager.notify();
    object(json!({"task":projection(&next)}))
}
