use super::*;

pub(super) fn call(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    const KEYS: &[&str] = &[
        "_cccc_secretary_token",
        "action",
        "task_id",
        "base_version",
        "status",
        "reply_text",
        "draft_text",
        "no_op",
        "source_summary",
        "checked_at",
        "source_urls",
        "handoff_target",
        "handoff_text",
        "resource",
        "path",
        "query",
    ];
    if request.args.keys().any(|key| !KEYS.contains(&key.as_str())) {
        return Err(OpError::new(
            "invalid_args",
            "Secretary tools do not accept a target override",
        ));
    }
    let token = string_arg(request, "_cccc_secretary_token")
        .filter(|token| token.len() == 64)
        .ok_or_else(expired)?;
    let manager = lookup(home).ok_or_else(expired)?;
    let execution = manager
        .grants
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&digest(token.as_bytes()))
        .cloned()
        .ok_or_else(expired)?;
    if string_arg(request, "task_id").as_deref() != Some(execution.task_id.as_str()) {
        return Err(expired());
    }
    let task = manager
        .store
        .load(&execution.task_id)
        .map_err(OpError::io)?;
    validate_execution(&execution, &task)?;
    let action = string_arg(request, "action").unwrap_or_else(|| "context".into());
    if action == "read" {
        super::super::assistants::secretary_validate_request(home, &task).map_err(OpError::io)?;
        return task_reads::read(home, &task, request);
    }
    if action == "context" {
        let group = cccc_core::GroupStore::new(home.clone())
            .map_err(OpError::io)?
            .load(&task.target.group_id)
            .map_err(OpError::io)?;
        let peers = group
            .actors
            .iter()
            .filter(|a| a.internal_kind.is_none())
            .map(|a| {
                json!({"id":a.id,"title":a.title,
            "role":cccc_core::actors::effective_role(&group,&a.id)})
            })
            .collect::<Vec<_>>();
        return object(
            json!({"task_id":task.task_id,"target":task.target,"inputs":task.inputs,
            "guidance":task.guidance,"followup":task.followup,"base_version":task.base_version,
            "working_document":(task.target.kind == SecretaryTaskKind::Document).then(|| format!("{}/document.md", task.task_id)),
            "receipt":task.receipt,"handoff_targets":peers}),
        );
    }
    if let Some(receipt) = task.receipt.as_ref() {
        return object(json!({"receipt":receipt}));
    }
    let submit = || {
        validate_execution(
            &execution,
            &manager
                .store
                .load(&execution.task_id)
                .map_err(OpError::io)?,
        )?;
        if action == "commit" {
            if task.target.kind != SecretaryTaskKind::Document {
                return Err(OpError::new(
                    "invalid_action",
                    "Only document tasks can commit a working copy",
                ));
            }
            let base = required_arg(request, "base_version")?;
            let receipt = manager
                .store
                .commit_document(&task.task_id, &base, |task| {
                    super::super::assistants::secretary_validate_document(home, task)
                })
                .map_err(OpError::io)?;
            execution.submitted.notify_one();
            return object(json!({"receipt":receipt}));
        }
        let (status, output) = match action.as_str() {
            "draft" if task.target.kind == SecretaryTaskKind::Prompt => {
                super::super::assistants::secretary_validate_request(home, &task)
                    .map_err(OpError::io)?;
                let text = bounded_text(request, "draft_text", 32_000)?;
                let no_op = crate::dispatch::bool_arg(request, "no_op", false);
                if text.is_empty() && !no_op {
                    return Err(OpError::new(
                        "invalid_args",
                        "draft_text or no_op=true is required",
                    ));
                }
                (
                    SecretaryTaskPhase::Done,
                    json!({"draft_text":text,"no_op":no_op}),
                )
            }
            "report" => {
                let status = required_arg(request, "status")?;
                let status = match status.as_str() {
                    "done" if task.target.kind == SecretaryTaskKind::Ask => {
                        SecretaryTaskPhase::Done
                    }
                    "needs_user" => SecretaryTaskPhase::NeedsUser,
                    "failed" => SecretaryTaskPhase::Failed,
                    _ => {
                        return Err(OpError::new(
                            "invalid_action",
                            "A document completes through commit; a prompt completes through draft",
                        ));
                    }
                };
                let reply = bounded_text(request, "reply_text", 4_000)?;
                if reply.is_empty() {
                    return Err(OpError::new("invalid_args", "reply_text is required"));
                }
                if task.target.kind == SecretaryTaskKind::Ask {
                    super::super::assistants::secretary_validate_request(home, &task)
                        .map_err(OpError::io)?;
                }
                let urls = request
                    .args
                    .get("source_urls")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let handoff_target = bounded_text(request, "handoff_target", 120)?;
                let handoff_text = bounded_text(request, "handoff_text", 8000)?;
                if !handoff_target.is_empty() || !handoff_text.is_empty() {
                    let group = cccc_core::GroupStore::new(home.clone())
                        .map_err(OpError::io)?
                        .load(&task.target.group_id)
                        .map_err(OpError::io)?;
                    if task.target.kind != SecretaryTaskKind::Ask
                        || status != SecretaryTaskPhase::Done
                        || handoff_text.is_empty()
                        || !group
                            .actors
                            .iter()
                            .any(|a| a.internal_kind.is_none() && a.id == handoff_target)
                    {
                        return Err(OpError::new(
                            "invalid_args",
                            "A handoff proposal requires one current Group peer and an Ask result",
                        ));
                    }
                }
                if !urls.as_array().is_some_and(|urls| {
                    urls.len() <= 12
                        && urls.iter().all(|url| {
                            url.as_str().is_some_and(|s| {
                                s.len() <= 1024
                                    && (s.starts_with("https://") || s.starts_with("http://"))
                            })
                        })
                }) {
                    return Err(OpError::new(
                        "invalid_args",
                        "source_urls must contain at most 12 HTTP URLs",
                    ));
                }
                (
                    status,
                    json!({"reply_text":reply,"source_summary":bounded_text(request,"source_summary",1200)?,
                    "checked_at":bounded_text(request,"checked_at",120)?,"source_urls":urls,
                    "handoff_target":handoff_target,"handoff_text":handoff_text}),
                )
            }
            _ => {
                return Err(OpError::new(
                    "invalid_action",
                    "Unsupported secretary task action",
                ));
            }
        };
        let receipt = manager
            .store
            .update(&task.task_id, |task| {
                if *execution.cancel.borrow() || !task.phase.executing() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Secretary task was cancelled",
                    ));
                }
                if let Some(receipt) = task.receipt.as_ref() {
                    return Ok(receipt.clone());
                }
                let receipt = SecretaryReceipt {
                    status,
                    output,
                    document_version: String::new(),
                    committed_at: cccc_contracts::utc_now(),
                };
                task.receipt = Some(receipt.clone());
                Ok(receipt)
            })
            .map_err(OpError::io)?;
        execution.submitted.notify_one();
        object(json!({"receipt":receipt}))
    };
    // Resource-owned controls remain callable during lifecycle draining; only
    // target mutation acquires the Group permit, never provider shutdown.
    if let Some(locks) = manager.locks.as_ref() {
        locks.with_group_write_blocking(&task.target.group_id, submit)
    } else {
        submit()
    }
}

fn validate_execution(execution: &Execution, task: &SecretaryTask) -> Result<(), OpError> {
    if *execution.cancel.borrow()
        || !execution.attempted.load(Ordering::Acquire)
        || execution
            .generation
            .get()
            .is_none_or(|generation| *generation != task.generation)
        || !task.phase.executing()
    {
        return Err(expired());
    }
    Ok(())
}

fn expired() -> OpError {
    OpError::new(
        "secretary_task_expired",
        "Secretary task grant is unavailable or expired",
    )
}
fn bounded_text(request: &DaemonRequest, key: &str, limit: usize) -> Result<String, OpError> {
    let value = string_arg(request, key)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if value.chars().count() > limit {
        return Err(OpError::new(
            "invalid_args",
            format!("{key} exceeds {limit} characters"),
        ));
    }
    Ok(value)
}
