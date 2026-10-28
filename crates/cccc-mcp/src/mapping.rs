use serde_json::{Map, Value};

use crate::actions;
use crate::argument_normalization::{
    alias, message_mode, normalize_message_author, normalize_recipients,
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
            alias(&mut args, "event_id", "reply_to");
            normalize_message_author(&mut args);
            let mode = message_mode(&mut args, "mail")?;
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
        "cccc_im_bind" => "im_bind_chat",
        "cccc_runtime_wait_next_turn" => "runtime_wait_next_turn",
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
    if namespace == "coordination" {
        match action_name.as_str() {
            "add_decision" => {
                args.insert("kind".into(), Value::String("decision".into()));
            }
            "add_handoff" => {
                args.insert("kind".into(), Value::String("handoff".into()));
            }
            _ => {}
        }
    }
    let op_name = match (namespace, action_name.as_str()) {
        ("coordination", "update_brief" | "brief") => "coordination.brief.update",
        ("coordination", "add_decision" | "add_handoff" | "add_note" | "note") => {
            "coordination.note.add"
        }
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
                    "if_task_rev",
                    "client_request_id",
                    "parent_ref",
                    "blocked_by_refs",
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
                if let Err(error) = daemon_call(name, args) {
                    if !error.starts_with("tool is not a daemon operation:") {
                        failures.push(format!("{name} {action}: {error}"));
                    }
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
