use serde_json::{Map, Value};

use crate::actions;
use crate::argument_normalization::{
    alias, default_new_message_mode, message_mode, normalize_message_author, normalize_recipients,
};

pub fn daemon_call(
    name: &str,
    mut args: Map<String, Value>,
) -> Result<(String, Map<String, Value>), String> {
    normalize_recipients(&mut args);
    let op = match name {
        "cccc_inbox_read" => "inbox_read",
        "cccc_connect" => {
            normalize_message_author(&mut args);
            "connect_catalog"
        }
        "cccc_message_history" => "message_history",
        "cccc_message_send" => {
            reject_reply_target(&mut args)?;
            alias(&mut args, "idempotency_key", "client_id");
            normalize_message_author(&mut args);
            let default = default_new_message_mode(&args, false);
            let mode = message_mode(&mut args, default)?;
            args.insert("message_mode".into(), Value::String(mode));
            if connect_destination(&mut args)? {
                return Ok(("connect_send".into(), args));
            }
            if args
                .get("dst_group_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            {
                "send_cross_group"
            } else {
                "send"
            }
        }
        "cccc_tracked_send" => {
            normalize_message_author(&mut args);
            "tracked_send"
        }
        "cccc_message_reply" => {
            alias(&mut args, "event_id", "reply_to");
            alias(&mut args, "idempotency_key", "client_id");
            normalize_message_author(&mut args);
            let mode = message_mode(&mut args, "send")?;
            args.insert("message_mode".into(), Value::String(mode));
            "reply"
        }
        "cccc_message_deliver" => {
            normalize_message_author(&mut args);
            "message_deliver"
        }
        "cccc_reply_request_cancel" => {
            normalize_message_author(&mut args);
            "reply_request_cancel"
        }
        "cccc_context_get" => {
            bound_context_page(&mut args);
            "context_get"
        }
        "cccc_context_sync" => "context_sync",
        "cccc_capability_search" => "capability_search",
        "cccc_capability_state" => "capability_state",
        "cccc_capability_enable" => {
            alias(&mut args, "id", "capability_id");
            "capability_enable"
        }
        "cccc_capability_install" => "capability_install_target",
        "cccc_capability_import" => "capability_import",
        "cccc_capability_block" => {
            alias(&mut args, "id", "capability_id");
            "capability_block"
        }
        "cccc_capability_uninstall" => {
            alias(&mut args, "id", "capability_id");
            "capability_uninstall"
        }
        "cccc_group" => {
            let (op, mut args) = action(args, actions::group)?;
            if op == "group_set_state"
                && args.get("state").and_then(Value::as_str) == Some("stopped")
            {
                args.remove("state");
                return Ok(("group_stop".into(), args));
            }
            return Ok((op, args));
        }
        "cccc_actor" => return action(args, actions::actor),
        "cccc_actor_notes" => return action(args, actions::actor_notes),
        "cccc_coordination" => return context_action(args, "coordination"),
        "cccc_task" => return context_action(args, "task"),
        "cccc_backlog" => {
            let (op, mut args) = action(args, actions::backlog)?;
            // One entry is cut from the whole snapshot here (backlog_projection.rs);
            // the daemon reads the file as a whole.
            if op == "backlog_get" {
                args.remove("entry");
            }
            return Ok((op, args));
        }
        "cccc_agent_state" => return context_action(args, "agent_state"),
        "cccc_automation" => return action(args, actions::automation),
        "cccc_notify" => {
            // The tool's `actor_id` names the caller, as for every Actor tool, but a
            // stored notification would read it as its target, and the sender never
            // receives its own event. Resolve the caller into `by`; only
            // `target_actor_id` (or `to`) addresses someone.
            normalize_message_author(&mut args);
            args.remove("actor_id");
            // The tool promises an internal notification; IM visibility stays a
            // daemon producer's explicit choice.
            if args
                .get("im_visibility")
                .is_some_and(|value| !value.is_null() && value != "internal")
            {
                return Err("cccc_notify sends internal notifications; omit im_visibility".into());
            }
            return action(args, actions::notify);
        }
        "cccc_presentation" => return action(args, actions::presentation),
        "cccc_space" => return space(args),
        "cccc_headless" => return action(args, actions::headless),
        "cccc_terminal" => return action(args, actions::terminal),
        "cccc_debug" => return action(args, actions::debug),
        "cccc_im_bind" => "im_bind_chat",
        "cccc_runtime_wait_next_turn" => {
            // Browser delivery calls the daemon itself. A tool caller pulls.
            args.remove("transport");
            args.remove("resume_turn_id");
            "runtime_wait_next_turn"
        }
        "cccc_runtime_complete_turn" => "runtime_complete_turn",
        "cccc_voice_secretary_task" => "voice_secretary_task",
        _ => return Err(format!("tool is not a daemon operation: {name}")),
    };
    Ok((op.into(), args))
}

fn space(mut args: Map<String, Value>) -> Result<(String, Map<String, Value>), String> {
    let action_name = args
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "status".into());
    let op =
        actions::space(&action_name).ok_or_else(|| format!("unsupported action: {action_name}"))?;
    if let Some(sub_action) = args.remove("sub_action") {
        args.insert("action".into(), sub_action);
    }
    Ok((op.into(), args))
}

fn action(
    mut args: Map<String, Value>,
    resolve: fn(&str) -> Option<&'static str>,
) -> Result<(String, Map<String, Value>), String> {
    let name = args
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "get".into());
    let op = resolve(&name).ok_or_else(|| format!("unsupported action: {name}"))?;
    Ok((op.into(), args))
}

fn context_action(
    mut args: Map<String, Value>,
    namespace: &str,
) -> Result<(String, Map<String, Value>), String> {
    let action_name = args
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "get".into());
    if namespace == "task" {
        return task_action(args, &action_name);
    }
    if action_name == "get" || action_name == "list" {
        return Ok(("context_get".into(), args));
    }
    let op_name = match (namespace, action_name.as_str()) {
        ("coordination", "update_brief" | "brief") => "coordination.brief.update",
        ("agent_state", "update" | "clear") => {
            if action_name == "update" {
                "agent_state.update"
            } else {
                "agent_state.clear"
            }
        }
        _ => return Err(format!("unsupported {namespace} action: {action_name}")),
    };
    let group_id = args.get("group_id").cloned();
    let by = args.get("by").cloned();
    args.insert("op".into(), Value::String(op_name.into()));
    let mut request = Map::new();
    if let Some(value) = group_id {
        request.insert("group_id".into(), value);
    }
    if let Some(value) = by {
        request.insert("by".into(), value);
    }
    request.insert("ops".into(), Value::Array(vec![Value::Object(args)]));
    Ok(("context_sync".into(), request))
}

fn task_action(
    mut args: Map<String, Value>,
    action_name: &str,
) -> Result<(String, Map<String, Value>), String> {
    let group_id = args.remove("group_id");
    let by = args.remove("by");
    args.remove("actor_id");
    args.remove("include_archived");
    if matches!(action_name, "get" | "list") {
        let mut request = Map::new();
        if let Some(value) = group_id {
            request.insert("group_id".into(), value);
        }
        let task_id = match args.remove("task_id") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if value.trim().is_empty() => None,
            Some(Value::String(value)) => Some(value),
            Some(_) => return Err("invalid_args: task_id must be a string".into()),
        };
        if let Some(task_id) = task_id {
            request.insert("task_id".into(), Value::String(task_id));
        } else {
            // Status, paging and the counts are applied to the summary rows (task_summary.rs);
            // the daemon filters by text, assignee and attention.
            for key in ["query", "assignee", "attention"] {
                if let Some(value) = args.remove(key) {
                    request.insert(key.into(), value);
                }
            }
        }
        return Ok(("task_list".into(), request));
    }
    let mut operations = Vec::new();
    match action_name {
        "create" => {
            if let Some(value) = args.remove("type") {
                args.insert("task_type".into(), value);
            }
            retain_fields(
                &mut args,
                &[
                    "title",
                    "outcome",
                    "status",
                    "parent_id",
                    "assignee",
                    "priority",
                    "blocked_by",
                    "waiting_on",
                    "handoff_to",
                    "task_type",
                    "notes",
                    "checklist",
                    "if_task_rev",
                    "client_request_id",
                    "parent_ref",
                    "blocked_by_refs",
                    "source",
                ],
            );
            args.insert("op".into(), Value::String("task.create".into()));
            operations.push(Value::Object(args));
        }
        "update" => {
            if let Some(value) = args.remove("type") {
                args.insert("task_type".into(), value);
            }
            let status = args.remove("status");
            let guard = args.get("if_task_rev").cloned();
            let task_id = args.get("task_id").cloned();
            retain_fields(
                &mut args,
                &[
                    "task_id",
                    "title",
                    "outcome",
                    "parent_id",
                    "assignee",
                    "priority",
                    "blocked_by",
                    "waiting_on",
                    "handoff_to",
                    "task_type",
                    "notes",
                    "checklist",
                    "if_task_rev",
                    "source",
                ],
            );
            let has_patch = args
                .keys()
                .any(|key| !matches!(key.as_str(), "task_id" | "if_task_rev"));
            if has_patch || status.is_none() {
                args.insert("op".into(), Value::String("task.update".into()));
                operations.push(Value::Object(args));
            }
            if let Some(status) = status {
                let mut movement = Map::new();
                movement.insert("op".into(), Value::String("task.move".into()));
                if let Some(task_id) = task_id {
                    movement.insert("task_id".into(), task_id);
                }
                movement.insert("status".into(), status);
                if operations.is_empty()
                    && let Some(guard) = guard
                {
                    movement.insert("if_task_rev".into(), guard);
                }
                operations.push(Value::Object(movement));
            }
        }
        "move" | "restore" | "delete" => {
            retain_fields(
                &mut args,
                if action_name == "move" {
                    &["task_id", "status", "if_task_rev"]
                } else {
                    &["task_id", "if_task_rev"]
                },
            );
            args.insert(
                "op".into(),
                Value::String(
                    match action_name {
                        "move" => "task.move",
                        "restore" => "task.restore",
                        _ => "task.delete",
                    }
                    .into(),
                ),
            );
            operations.push(Value::Object(args));
        }
        _ => return Err(format!("unsupported task action: {action_name}")),
    }
    let mut request = Map::new();
    if let Some(value) = group_id {
        request.insert("group_id".into(), value);
    }
    if let Some(value) = by {
        request.insert("by".into(), value);
    }
    request.insert("ops".into(), Value::Array(operations));
    Ok(("context_sync".into(), request))
}

/// An Actor's context read is paged: at most 100 tasks and 256 KiB per answer
/// (a caller may ask for less). Large groups otherwise return megabytes that
/// providers reject, and the turn retries the same oversized read.
const CONTEXT_PAGE_LIMIT: u64 = 100;
const CONTEXT_PAGE_MAX_BYTES: u64 = 256 * 1024;

fn bound_context_page(args: &mut Map<String, Value>) {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(CONTEXT_PAGE_LIMIT)
        .clamp(1, CONTEXT_PAGE_LIMIT);
    let max_bytes = args
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(CONTEXT_PAGE_MAX_BYTES)
        .clamp(4096, CONTEXT_PAGE_MAX_BYTES);
    args.insert("limit".into(), Value::from(limit));
    args.insert("max_bytes".into(), Value::from(max_bytes));
    args.entry("include_archived").or_insert(Value::Bool(false));
}

fn retain_fields(args: &mut Map<String, Value>, allowed: &[&str]) {
    args.retain(|key, _| allowed.contains(&key.as_str()));
}

/// A send starts a new message. Answering an existing event belongs to
/// cccc_message_reply: routing a target through send used to drop an explicit
/// mode, so a request for a reply went out as a plain message.
fn reject_reply_target(args: &mut Map<String, Value>) -> Result<(), String> {
    for key in ["event_id", "reply_to"] {
        let blank = match args.get(key) {
            None => continue,
            Some(Value::Null) => true,
            Some(Value::String(value)) => value.trim().is_empty(),
            Some(_) => false,
        };
        if !blank {
            return Err("invalid_args: cccc_message_send starts a new message; to answer an existing event, use cccc_message_reply with its event_id".into());
        }
        args.remove(key);
    }
    Ok(())
}

/// Qualified remote routes are selected before any legacy/local Group lookup.
pub(crate) fn connect_destination(args: &mut Map<String, Value>) -> Result<bool, String> {
    let Some(instance) = args.remove("dst_instance_id") else {
        return Ok(false);
    };
    let instance = instance
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("dst_instance_id must be a nonempty instance ID")?;
    let target = args
        .remove("dst_group_id")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
        .ok_or("dst_group_id is required with dst_instance_id")?;
    args.insert("instance_id".into(), Value::String(instance.into()));
    args.insert("target_group_id".into(), Value::String(target));
    if let Some(key) = args.remove("idempotency_key") {
        args.entry("client_id").or_insert(key);
    }
    args.entry("client_id")
        .or_insert_with(|| Value::String(uuid::Uuid::new_v4().to_string()));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::daemon_call;
    use crate::argument_normalization::normalize_message_author;
    use serde_json::{Map, json};

    #[test]
    fn a_tool_caller_cannot_take_a_browser_turn_back() {
        let args = serde_json::json!({
            "limit":1,"transport":"web_model_browser","resume_turn_id":"turn"
        })
        .as_object()
        .cloned()
        .expect("arguments");
        let (op, args) = daemon_call("cccc_runtime_wait_next_turn", args).expect("mapping");
        assert_eq!(op, "runtime_wait_next_turn");
        assert_eq!(
            args,
            serde_json::json!({"limit":1})
                .as_object()
                .cloned()
                .expect("expected")
        );
    }

    #[test]
    fn group_set_state_stopped_uses_the_stop_operation_with_authority_intact() {
        let input = json!({"action":"set_state", "state":"stopped", "group_id":"g", "by":"foreman", "_cccc_native_source_binding":{"revision":7}});
        let (op, args) = daemon_call(
            "cccc_group",
            input.as_object().expect("Group arguments").clone(),
        )
        .expect("Group mapping");
        assert_eq!(op, "group_stop");
        assert!(!args.contains_key("state"));
        assert_eq!(args["by"], "foreman");
        assert_eq!(args["group_id"], "g");
        assert_eq!(args["_cccc_native_source_binding"]["revision"], 7);
    }

    #[test]
    fn context_reads_never_ask_for_more_than_one_bounded_page() {
        let (op, args) = daemon_call("cccc_context_get", Map::new()).expect("mapping");
        assert_eq!(op, "context_get");
        assert_eq!(args["limit"], 100);
        assert_eq!(args["max_bytes"], 256 * 1024);
        assert_eq!(args["include_archived"], false);
        let asked = json!({"limit":5000,"max_bytes":10_000_000,"include_archived":true});
        let (_, args) = daemon_call(
            "cccc_context_get",
            asked.as_object().cloned().expect("args"),
        )
        .expect("mapping");
        assert_eq!(args["limit"], 100);
        assert_eq!(args["max_bytes"], 256 * 1024);
        assert_eq!(args["include_archived"], true);
    }

    /// Every argument a message tool offers reaches the daemon under a key
    /// that operation accepts, together with the keys the router adds.
    #[test]
    fn message_tool_arguments_reach_the_daemon_under_accepted_keys() {
        use cccc_contracts::message_args::{MessageWrite, unknown};
        let properties = |tool: &str| {
            crate::tools::catalog()
                .into_iter()
                .find(|entry| entry["name"] == tool)
                .unwrap_or_else(|| panic!("{tool}"))["inputSchema"]["properties"]
                .as_object()
                .expect("properties")
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };
        for (tool, left_out, operation) in [
            (
                "cccc_message_send",
                &["dst_instance_id", "dst_group_id"][..],
                MessageWrite::Send,
            ),
            (
                "cccc_message_send",
                &["dst_instance_id"][..],
                MessageWrite::SendCrossGroup,
            ),
            ("cccc_message_send", &[][..], MessageWrite::ConnectSend),
            // `event_id` is the same field as `reply_to`.
            ("cccc_message_reply", &["event_id"][..], MessageWrite::Reply),
            ("cccc_tracked_send", &[][..], MessageWrite::TrackedSend),
            ("cccc_notify", &[][..], MessageWrite::SystemNotify),
        ] {
            let mut args = Map::new();
            for key in properties(tool) {
                if left_out.contains(&key.as_str()) {
                    continue;
                }
                let value = match key.as_str() {
                    "mode" | "action" => json!("send"),
                    "to" => json!(["peer"]),
                    _ => json!("x"),
                };
                args.insert(key, value);
            }
            let (op, mut mapped) = daemon_call(tool, args).expect(tool);
            for key in ["by", "_cccc_native_binding"] {
                mapped.insert(key.into(), json!("router"));
            }
            if crate::router::is_message_operation(tool) {
                mapped.insert("require_peer_insight".into(), json!(true));
            }
            assert!(
                unknown(operation, mapped.keys()).is_empty(),
                "{tool} reaches {op} with {:?}",
                unknown(operation, mapped.keys())
            );
        }
    }

    #[test]
    fn local_message_routes_forward_the_retry_key_to_the_daemon() {
        for (destination, expected_operation) in
            [(None, "send"), (Some("other"), "send_cross_group")]
        {
            let mut input = json!({
                "group_id": "source",
                "actor_id": "worker",
                "to": ["peer"],
                "text": "one logical message",
                "mode": "send",
                "idempotency_key": "stable-message"
            })
            .as_object()
            .expect("arguments")
            .clone();
            if let Some(destination) = destination {
                input.insert("dst_group_id".into(), json!(destination));
            }
            let (operation, mapped) = daemon_call("cccc_message_send", input).expect("mapping");
            assert_eq!(operation, expected_operation);
            assert_eq!(mapped["client_id"], "stable-message");
            assert!(!mapped.contains_key("idempotency_key"));
        }
    }

    #[test]
    fn connect_destination_is_qualified_even_when_group_ids_match() {
        let args=serde_json::json!({"group_id":"g_same","actor_id":"worker","dst_instance_id":"i_other","dst_group_id":"g_same","text":"hello","mode":"send","idempotency_key":"stable"}).as_object().expect("args").clone();
        let (op, args) = daemon_call("cccc_message_send", args).expect("mapping");
        assert_eq!(op, "connect_send");
        assert_eq!(args["by"], "worker");
        assert_eq!(args["group_id"], "g_same");
        assert_eq!(args["target_group_id"], "g_same");
        assert_eq!(args["instance_id"], "i_other");
        assert_eq!(args["client_id"], "stable");
        assert!(
            !args.contains_key("dst_group_id"),
            "must not escalate to a global dispatcher write"
        );
        assert!(!args.contains_key("dst_instance_id"));
    }

    #[test]
    fn connect_directory_and_missing_destination_do_not_fall_back_to_local_send() {
        let (op,args)=daemon_call("cccc_connect",serde_json::json!({"group_id":"g","actor_id":"worker","instance_id":"remote","after":"cursor","limit":2}).as_object().expect("args").clone()).expect("directory");
        assert_eq!(op, "connect_catalog");
        assert_eq!(args["after"], "cursor");
        for destination in [
            serde_json::json!({"dst_instance_id":"remote"}),
            serde_json::json!({"dst_instance_id":"","dst_group_id":"g"}),
        ] {
            assert!(
                daemon_call(
                    "cccc_message_send",
                    destination.as_object().expect("args").clone()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn advertised_daemon_tool_actions_have_executable_mappings() {
        let mut failures = Vec::new();
        for tool in crate::tools::catalog() {
            let Some(name) = tool["name"].as_str() else {
                continue;
            };
            let Some(actions) = tool["inputSchema"]["properties"]["action"]["enum"].as_array()
            else {
                continue;
            };
            for action in actions {
                let args = json!({"action":action,"group_id":"g_fixture","by":"lead","task_id":"T001", "title":"fixture", "summary":"fixture"}).as_object().cloned().expect("args");
                if let Err(error) = daemon_call(name, args)
                    && !error.starts_with("tool is not a daemon operation:")
                {
                    failures.push(format!("{name} {action}: {error}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "unmapped advertised actions: {failures:#?}"
        );
    }

    #[test]
    fn terminal_resize_maps_to_standard_daemon_operation() {
        let args = json!({"action":"resize","group_id":"g_test","actor_id":"peer1"})
            .as_object()
            .cloned()
            .unwrap_or_else(Map::new);
        let (op, args) = daemon_call("cccc_terminal", args).expect("mapping");
        assert_eq!(op, "term_resize");
        assert!(!args.contains_key("action"));
    }

    #[test]
    fn message_actor_id_becomes_daemon_author() {
        for tool in [
            "cccc_message_send",
            "cccc_tracked_send",
            "cccc_message_reply",
            "cccc_message_deliver",
            "cccc_reply_request_cancel",
        ] {
            let mut value = json!({
                "group_id":"g_test",
                "actor_id":"backend",
                "text":"done",
                "to":["user"]
            });
            let args = value.as_object_mut().expect("args");
            // A send starts a new message, so only the other tools take a target.
            if tool != "cccc_message_send" {
                args.insert("reply_to".into(), json!("event-1"));
            }
            let args = args.clone();
            let (_, args) = daemon_call(tool, args).expect("mapping");
            assert_eq!(args["by"], "backend", "tool={tool}");
        }
    }

    #[test]
    fn notify_resolves_its_actor_id_into_the_author_never_the_target() {
        for (input, author) in [
            // Without runtime context, actor_id is the caller, as the schema says.
            (json!({"actor_id":"lead"}), "lead"),
            // A bound or environment caller's own actor_id.
            (json!({"actor_id":"lead","by":"lead"}), "lead"),
            // Any other actor_id still names a caller, never a recipient.
            (json!({"actor_id":"peer","by":"lead"}), "lead"),
        ] {
            let mut args = input.as_object().cloned().expect("args");
            args.insert("action".into(), json!("send"));
            args.insert("title".into(), json!("Build finished"));
            args.insert("target_actor_id".into(), json!("reviewer"));
            let (op, args) = daemon_call("cccc_notify", args).expect("mapping");
            assert_eq!(op, "system_notify", "{input}");
            assert_eq!(args["by"], author, "{input}");
            assert!(!args.contains_key("actor_id"), "{input}");
            assert_eq!(args["target_actor_id"], "reviewer", "{input}");
            assert_eq!(args["title"], "Build finished", "{input}");
        }
        for (visibility, accepted) in [
            (json!("internal"), true),
            (json!(null), true),
            (json!("public"), false),
        ] {
            let args = json!({"action":"send","by":"lead","im_visibility":visibility})
                .as_object()
                .cloned()
                .expect("args");
            assert_eq!(
                daemon_call("cccc_notify", args).is_ok(),
                accepted,
                "{visibility}"
            );
        }
    }

    #[test]
    fn explicit_message_author_is_preserved_without_runtime_context() {
        let mut args = json!({"actor_id":"backend","by":"trusted-proxy"})
            .as_object()
            .cloned()
            .expect("args");
        normalize_message_author(&mut args);
        assert_eq!(args["by"], "trusted-proxy");
    }

    #[test]
    fn message_with_destination_group_maps_to_cross_group_send() {
        let args = json!({
            "group_id":"g_source","dst_group_id":"g_destination",
            "actor_id":"backend","text":"review","to":["peer1"]
        })
        .as_object()
        .cloned()
        .expect("args");
        let (op, args) = daemon_call("cccc_message_send", args).expect("mapping");
        assert_eq!(op, "send_cross_group");
        assert_eq!(args["by"], "backend");
    }

    #[test]
    fn message_reply_maps_mail_mode_to_the_daemon_contract() {
        let args = json!({
            "group_id":"g_test","actor_id":"backend","text":"done",
            "event_id":"event-1","mode":"mail"
        })
        .as_object()
        .cloned()
        .expect("args");
        let (op, args) = daemon_call("cccc_message_reply", args).expect("mapping");
        assert_eq!(op, "reply");
        assert_eq!(args["reply_to"], "event-1");
        assert_eq!(args["message_mode"], "mail");
        assert!(!args.contains_key("mode"));
    }

    #[test]
    fn message_modes_preserve_explicit_choices_and_existing_defaults() {
        for (tool, default, modes) in [
            (
                "cccc_message_send",
                "mail",
                vec!["mail", "send", "request_reply"],
            ),
            ("cccc_message_reply", "send", vec!["mail", "send"]),
        ] {
            for mode in std::iter::once(None).chain(modes.into_iter().map(Some)) {
                let mut input =
                    json!({"group_id":"g_test","to":["peer"],"text":"Synthetic message"});
                if let Some(mode) = mode {
                    input["mode"] = json!(mode);
                }
                let (_, args) = daemon_call(tool, input.as_object().expect("args").clone())
                    .expect("valid mode");
                assert_eq!(args["message_mode"], mode.unwrap_or(default), "{tool}");
                assert!(!args.contains_key("mode"));
            }
        }
    }

    #[test]
    fn message_modes_reject_wire_fields_and_invalid_values_instead_of_defaulting() {
        for tool in ["cccc_message_send", "cccc_message_reply"] {
            for input in [
                json!({"message_mode":"mail"}),
                json!({"mode":"send","message_mode":"mail"}),
                json!({"mode":"mail","message_mode":"mail"}),
                json!({"mode":null}),
                json!({"mode":false}),
                json!({"mode":42}),
                json!({"mode":"invalid"}),
            ] {
                let error = daemon_call(tool, input.as_object().expect("args").clone())
                    .expect_err("invalid mode");
                assert!(error.starts_with("invalid_args:"), "{tool}: {error}");
                assert!(error.contains("mode"));
            }
        }
    }

    #[test]
    fn message_control_tools_map_to_existing_event_operations() {
        for (tool, expected) in [
            ("cccc_message_deliver", "message_deliver"),
            ("cccc_reply_request_cancel", "reply_request_cancel"),
        ] {
            let args = json!({
                "group_id":"g_source","actor_id":"backend",
                "source_event_id":"event-1","actor_ids":["peer1"]
            })
            .as_object()
            .cloned()
            .expect("args");
            let (op, args) = daemon_call(tool, args).expect("mapping");
            assert_eq!(op, expected);
            assert_eq!(args["by"], "backend");
            assert_eq!(args["source_event_id"], "event-1");
        }
    }

    #[test]
    fn task_mapping_rejects_the_out_of_schema_archive_alias() {
        let args = json!({"action":"archive","group_id":"g_test","task_id":"t1"})
            .as_object()
            .cloned()
            .expect("args");
        assert_eq!(
            daemon_call("cccc_task", args).expect_err("archive must be rejected"),
            "unsupported task action: archive"
        );
    }

    #[test]
    fn task_mapping_normalizes_public_fields_and_combines_update_with_move() {
        let create = json!({
            "action":"create","group_id":"g_test","actor_id":"peer","by":"peer",
            "title":"typed","type":"optimization"
        })
        .as_object()
        .cloned()
        .expect("create args");
        let (op, args) = daemon_call("cccc_task", create).expect("create mapping");
        assert_eq!(op, "context_sync");
        assert_eq!(args["group_id"], "g_test");
        assert_eq!(args["by"], "peer");
        assert_eq!(args["ops"][0]["op"], "task.create");
        assert_eq!(args["ops"][0]["task_type"], "optimization");
        for leaked in ["group_id", "actor_id", "by", "type"] {
            assert!(args["ops"][0].get(leaked).is_none(), "leaked {leaked}");
        }

        let update = json!({
            "action":"update","group_id":"g_test","actor_id":"peer","by":"peer",
            "task_id":"T001","notes":"done","status":"active"
        })
        .as_object()
        .cloned()
        .expect("update args");
        let (_, args) = daemon_call("cccc_task", update).expect("update mapping");
        assert_eq!(args["ops"].as_array().expect("ops").len(), 2);
        assert_eq!(args["ops"][0]["op"], "task.update");
        assert_eq!(args["ops"][0]["notes"], "done");
        assert!(args["ops"][0].get("status").is_none());
        assert_eq!(
            args["ops"][1],
            json!({"op":"task.move","task_id":"T001","status":"active"})
        );
    }

    #[test]
    fn task_create_mapping_keeps_the_callers_ids_and_refs() {
        let create = json!({
            "action":"create","group_id":"g_test","actor_id":"lead","by":"lead",
            "title":"publish","client_request_id":"publish","parent_ref":"release",
            "blocked_by_refs":["build"]
        })
        .as_object()
        .cloned()
        .expect("create args");
        let (_, args) = daemon_call("cccc_task", create).expect("create mapping");
        assert_eq!(args["ops"][0]["client_request_id"], "publish");
        assert_eq!(args["ops"][0]["parent_ref"], "release");
        assert_eq!(args["ops"][0]["blocked_by_refs"], json!(["build"]));
    }

    #[test]
    fn task_mapping_forwards_the_backlog_source_on_create_and_update() {
        let source = json!({"scope_key":"s_1","path":"BACKLOG.md","entry_id":"0123456789ab"});
        let create = json!({
            "action":"create","group_id":"g_test","by":"lead","title":"t","source":source
        })
        .as_object()
        .cloned()
        .expect("create args");
        let (_, args) = daemon_call("cccc_task", create).expect("create mapping");
        assert_eq!(args["ops"][0]["source"], source);

        let update = json!({
            "action":"update","group_id":"g_test","by":"lead","task_id":"T1","source":source,
            "status":"archived"
        })
        .as_object()
        .cloned()
        .expect("update args");
        let (_, args) = daemon_call("cccc_task", update).expect("update mapping");
        assert_eq!(args["ops"][0]["op"], "task.update");
        assert_eq!(args["ops"][0]["source"], source);
        assert_eq!(args["ops"][1]["op"], "task.move");
    }

    #[test]
    fn backlog_actions_map_to_daemon_operations_and_reads_send_no_entry() {
        for (action, expected) in [
            ("get", "backlog_get"),
            ("use", "backlog_use"),
            ("create", "backlog_create"),
            ("exclude", "backlog_exclude"),
            ("add", "backlog_add"),
            ("edit", "backlog_edit"),
            ("move", "backlog_move"),
            ("close", "backlog_close"),
            ("promote", "backlog_promote"),
        ] {
            let args = json!({
                "action":action,"group_id":"g_test","by":"lead",
                "entry":{"id":"0123456789ab"},"if_file_sha256":"a"
            })
            .as_object()
            .cloned()
            .expect("args");
            let (op, args) = daemon_call("cccc_backlog", args).expect(action);
            assert_eq!(op, expected);
            assert!(args.get("action").is_none());
            assert_eq!(args.contains_key("entry"), action != "get", "{action}");
        }
        let (op, _) = daemon_call(
            "cccc_backlog",
            json!({"group_id":"g_test"})
                .as_object()
                .cloned()
                .expect("args"),
        )
        .expect("default action");
        assert_eq!(op, "backlog_get");
        let error = daemon_call(
            "cccc_backlog",
            json!({"action":"delete","group_id":"g_test"})
                .as_object()
                .cloned()
                .expect("args"),
        )
        .expect_err("unsupported");
        assert!(error.contains("delete"), "{error}");
    }

    #[test]
    fn actor_notes_actions_map_to_the_daemon_owned_help_contract() {
        for (action, expected) in [
            ("get", "actor_notes_get"),
            ("set", "actor_notes_set"),
            ("clear", "actor_notes_clear"),
        ] {
            let args = json!({
                "action":action,
                "group_id":"g_test",
                "target_actor_id":"peer",
                "by":"lead"
            })
            .as_object()
            .cloned()
            .expect("args");
            let (op, args) = daemon_call("cccc_actor_notes", args).expect("mapping");
            assert_eq!(op, expected);
            assert!(!args.contains_key("action"));
            assert_eq!(args["target_actor_id"], "peer");
        }
    }

    #[test]
    fn message_send_rejects_reply_targets_on_every_route() {
        let routes = [
            json!({}),
            json!({"dst_group_id":"g_remote"}),
            json!({"dst_instance_id":"i_other","dst_group_id":"g_remote"}),
        ];
        let targets = [
            json!({"event_id":"event-1"}),
            json!({"reply_to":"event-1"}),
            json!({"event_id":"event-1","reply_to":"event-2"}),
            json!({"reply_to":7}),
        ];
        for route in &routes {
            for target in &targets {
                for mode in [None, Some("send"), Some("mail"), Some("request_reply")] {
                    let mut args = json!({
                        "group_id":"g_test","actor_id":"backend","to":["peer"],"text":"done"
                    })
                    .as_object()
                    .cloned()
                    .expect("args");
                    for fields in [route, target] {
                        args.extend(fields.as_object().expect("fields").clone());
                    }
                    if let Some(mode) = mode {
                        args.insert("mode".into(), json!(mode));
                    }
                    let error = daemon_call("cccc_message_send", args)
                        .expect_err("a send must not answer an existing event");
                    assert!(error.starts_with("invalid_args:"), "{error}");
                    assert!(error.contains("cccc_message_reply"), "{error}");
                }
            }
        }
    }

    #[test]
    fn message_send_treats_blank_reply_targets_as_absent() {
        for key in ["event_id", "reply_to"] {
            for blank in [json!(null), json!(""), json!("  ")] {
                let mut args = json!({
                    "group_id":"g_test","actor_id":"backend","to":["peer"],
                    "text":"new","mode":"mail"
                })
                .as_object()
                .cloned()
                .expect("args");
                args.insert(key.into(), blank);
                let (op, args) = daemon_call("cccc_message_send", args).expect("new message");
                assert_eq!(op, "send");
                assert_eq!(args["message_mode"], "mail");
                assert!(!args.contains_key("event_id") && !args.contains_key("reply_to"));
            }
        }
    }

    #[test]
    fn message_reply_keeps_the_target_and_an_explicit_mail_mode() {
        let args = json!({
            "group_id":"g_test","actor_id":"backend","event_id":"event-1",
            "text":"done","mode":"mail"
        })
        .as_object()
        .cloned()
        .expect("args");
        let (op, args) = daemon_call("cccc_message_reply", args).expect("reply");
        assert_eq!(op, "reply");
        assert_eq!(args["reply_to"], "event-1");
        assert_eq!(args["message_mode"], "mail");
        assert_eq!(args["by"], "backend");
    }

    #[test]
    fn task_revision_guard_is_preserved_once_in_compound_updates() {
        for arguments in [
            json!({"action":"update","task_id":"T792","waiting_on":"none","status":"active","if_task_rev":0}),
            json!({"action":"update","task_id":"T792","status":"active","if_task_rev":0}),
            json!({"action":"move","task_id":"T792","status":"active","if_task_rev":0}),
            json!({"action":"restore","task_id":"T792","if_task_rev":0}),
            json!({"action":"delete","task_id":"T792","if_task_rev":0}),
        ] {
            let (_, args) = daemon_call("cccc_task", arguments.as_object().expect("args").clone())
                .expect("mapping");
            let operations = args["ops"].as_array().expect("ops");
            assert_eq!(operations[0]["if_task_rev"], 0);
            assert!(
                operations
                    .iter()
                    .skip(1)
                    .all(|op| op.get("if_task_rev").is_none())
            );
        }
    }
}
