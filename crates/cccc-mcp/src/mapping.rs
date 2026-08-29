use serde_json::{Map, Value};

use crate::actions;
use crate::argument_normalization::{alias, normalize_message_author, normalize_recipients};

pub fn daemon_call(
    name: &str,
    mut args: Map<String, Value>,
) -> Result<(String, Map<String, Value>), String> {
    normalize_recipients(&mut args);
    let op = match name {
        "cccc_inbox_read" => "inbox_read",
        "cccc_message_history" => "message_history",
        "cccc_message_send" => {
            alias(&mut args, "event_id", "reply_to");
            normalize_message_author(&mut args);
            let mode = args
                .remove("mode")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "mail".into());
            args.insert("message_mode".into(), Value::String(mode));
            if args
                .get("dst_group_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            {
                "send_cross_group"
            } else if args
                .get("reply_to")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            {
                args.remove("message_mode");
                "reply"
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
            normalize_message_author(&mut args);
            let mode = args
                .remove("mode")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "send".into());
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
        "cccc_context_get" => "context_get",
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
        "cccc_group" => return action(args, actions::group),
        "cccc_actor" => return action(args, actions::actor),
        "cccc_actor_notes" => return action(args, actions::actor_notes),
        "cccc_coordination" => return context_action(args, "coordination"),
        "cccc_task" => return context_action(args, "task"),
        "cccc_agent_state" => return context_action(args, "agent_state"),
        "cccc_memory" => return action(args, actions::memory),
        "cccc_memory_admin" => return action(args, actions::memory_admin),
        "cccc_automation" => return action(args, actions::automation),
        "cccc_notify" => return action(args, actions::notify),
        "cccc_presentation" => return action(args, actions::presentation),
        "cccc_space" => return space(args),
        "cccc_headless" => return action(args, actions::headless),
        "cccc_terminal" => return action(args, actions::terminal),
        "cccc_debug" => return action(args, actions::debug),
        "cccc_im_bind" => return action(args, actions::im),
        "cccc_runtime_wait_next_turn" => "runtime_wait_next_turn",
        "cccc_runtime_complete_turn" => "runtime_complete_turn",
        "cccc_voice_secretary_document" => return voice_document(args),
        "cccc_voice_secretary_composer" => return voice_composer(args),
        "cccc_voice_secretary_request" => return voice_request(args),
        _ => return Err(format!("tool is not a daemon operation: {name}")),
    };
    Ok((op.into(), args))
}

fn voice_document(mut args: Map<String, Value>) -> Result<(String, Map<String, Value>), String> {
    let action_name = args
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "list".into());
    let op = actions::voice_document(&action_name)
        .ok_or_else(|| format!("unsupported action: {action_name}"))?;
    args.insert(
        "by".into(),
        Value::String("assistant:voice_secretary".into()),
    );
    if action_name == "create" {
        args.insert("create_new".into(), Value::Bool(true));
    }
    Ok((op.into(), args))
}

fn voice_composer(mut args: Map<String, Value>) -> Result<(String, Map<String, Value>), String> {
    let action_name = args
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "submit_prompt_draft".into());
    let op = actions::voice_composer(&action_name)
        .ok_or_else(|| format!("unsupported action: {action_name}"))?;
    args.insert(
        "by".into(),
        Value::String("assistant:voice_secretary".into()),
    );
    Ok((op.into(), args))
}

fn voice_request(mut args: Map<String, Value>) -> Result<(String, Map<String, Value>), String> {
    let action_name = args
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "handoff".into());
    match action_name.as_str() {
        "handoff" => Ok(("assistant_voice_request".into(), args)),
        "report" => {
            alias(&mut args, "source_request_id", "request_id");
            Ok(("assistant_voice_instruction_feedback".into(), args))
        }
        _ => Err(format!("unsupported action: {action_name}")),
    }
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
        ("coordination", "add_note" | "note") => "coordination.note.add",
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
        if let Some(value) = args.remove("task_id") {
            request.insert("task_id".into(), value);
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
                ],
            );
            let has_patch = args.keys().any(|key| key != "task_id");
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
                operations.push(Value::Object(movement));
            }
        }
        "move" | "restore" | "delete" => {
            retain_fields(
                &mut args,
                if action_name == "move" {
                    &["task_id", "status"]
                } else {
                    &["task_id"]
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

fn retain_fields(args: &mut Map<String, Value>, allowed: &[&str]) {
    args.retain(|key, _| allowed.contains(&key.as_str()));
}

#[cfg(test)]
mod tests {
    use super::daemon_call;
    use crate::argument_normalization::normalize_message_author;
    use serde_json::{Map, json};

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
                "to":["user"],
                "reply_to":"event-1"
            });
            let args = value.as_object_mut().expect("args").clone();
            let (_, args) = daemon_call(tool, args).expect("mapping");
            assert_eq!(args["by"], "backend", "tool={tool}");
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
    fn memory_actions_map_to_reme_operations() {
        for (tool, action, expected) in [
            ("cccc_memory", "layout_get", "memory_reme_layout_get"),
            ("cccc_memory", "search", "memory_reme_search"),
            ("cccc_memory", "get", "memory_reme_get"),
            ("cccc_memory", "write", "memory_reme_write"),
            ("cccc_memory_admin", "index_sync", "memory_reme_index_sync"),
            (
                "cccc_memory_admin",
                "context_check",
                "memory_reme_context_check",
            ),
            (
                "cccc_memory_admin",
                "daily_flush",
                "memory_reme_daily_flush",
            ),
        ] {
            let args = json!({"action":action}).as_object().cloned().expect("args");
            let (op, _) = daemon_call(tool, args).expect("mapping");
            assert_eq!(op, expected);
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
    fn voice_secretary_actions_use_python_contract() {
        let create = json!({"action":"create","title":"Notes"})
            .as_object()
            .cloned()
            .expect("args");
        let (op, args) = daemon_call("cccc_voice_secretary_document", create).expect("create");
        assert_eq!(op, "assistant_voice_document_save");
        assert_eq!(args["create_new"], true);
        assert_eq!(args["by"], "assistant:voice_secretary");

        let composer = json!({"action":"submit_prompt_draft","draft_text":"Refined"})
            .as_object()
            .cloned()
            .expect("args");
        let (op, args) = daemon_call("cccc_voice_secretary_composer", composer).expect("composer");
        assert_eq!(op, "assistant_voice_prompt_draft_submit");
        assert_eq!(args["draft_text"], "Refined");
        assert!(args.get("text").is_none());
        assert_eq!(args["by"], "assistant:voice_secretary");

        let report = json!({"action":"report","source_request_id":"request-1","status":"done"})
            .as_object()
            .cloned()
            .expect("args");
        let (op, args) =
            daemon_call("cccc_voice_secretary_request", report).expect("report mapping");
        assert_eq!(op, "assistant_voice_instruction_feedback");
        assert_eq!(args["request_id"], "request-1");
    }

    #[test]
    fn message_with_reply_target_maps_to_reply() {
        let args = json!({
            "group_id":"g_test","actor_id":"backend","text":"done",
            "reply_to":"event-1"
        })
        .as_object()
        .cloned()
        .expect("args");
        let (op, args) = daemon_call("cccc_message_send", args).expect("mapping");
        assert_eq!(op, "reply");
        assert_eq!(args["by"], "backend");
    }
}
