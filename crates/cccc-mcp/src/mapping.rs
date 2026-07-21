use serde_json::{Map, Value, json};

use crate::actions;

pub fn daemon_call(
    name: &str,
    mut args: Map<String, Value>,
) -> Result<(String, Map<String, Value>), String> {
    normalize_recipients(&mut args);
    let op = match name {
        "cccc_inbox_list" => "inbox_list",
        "cccc_message_send" => {
            normalize_message_author(&mut args);
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
            normalize_message_author(&mut args);
            "reply"
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
        "cccc_capability_use" => {
            alias(&mut args, "id", "capability_id");
            "capability_tool_call"
        }
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
        "cccc_coordination" => return context_action(args, "coordination"),
        "cccc_task" => return context_action(args, "task"),
        "cccc_agent_state" => return context_action(args, "agent_state"),
        "cccc_memory" => return action(args, actions::memory),
        "cccc_memory_admin" => return action(args, actions::memory_admin),
        "cccc_automation" => return action(args, actions::automation),
        "cccc_notify" => return action(args, actions::notify),
        "cccc_presentation" => return action(args, actions::presentation),
        "cccc_space" => return action(args, actions::space),
        "cccc_headless" => return action(args, actions::headless),
        "cccc_terminal" => return action(args, actions::terminal),
        "cccc_debug" => return action(args, actions::debug),
        "cccc_im_bind" => return action(args, actions::im),
        "cccc_runtime_wait_next_turn" => "web_model_runtime_wait_next_turn",
        "cccc_runtime_complete_turn" => "web_model_runtime_complete_turn",
        "cccc_voice_secretary_document" => return action(args, actions::voice_document),
        "cccc_voice_secretary_composer" => return action(args, actions::voice_composer),
        "cccc_voice_secretary_request" => "assistant_voice_request",
        _ => return Err(format!("tool is not a daemon operation: {name}")),
    };
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
    if action_name == "get" || action_name == "list" {
        return Ok((
            if namespace == "task" {
                "task_list"
            } else {
                "context_get"
            }
            .into(),
            args,
        ));
    }
    let op_name = match (namespace, action_name.as_str()) {
        ("coordination", "update_brief" | "brief") => "coordination.brief.update",
        ("coordination", "add_note" | "note") => "coordination.note.add",
        ("task", "create" | "update" | "move" | "restore" | "delete") => match action_name.as_str()
        {
            "create" => "task.create",
            "update" => "task.update",
            "move" => "task.move",
            "restore" => "task.restore",
            _ => "task.delete",
        },
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

fn alias(args: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = args.remove(from) {
        args.entry(to).or_insert(value);
    }
}
fn normalize_recipients(args: &mut Map<String, Value>) {
    if let Some(Value::String(value)) = args.get("to").cloned() {
        args.insert("to".into(), json!([value]));
    }
}

pub(crate) fn normalize_message_author(args: &mut Map<String, Value>) {
    if args
        .get("by")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return;
    }
    if let Some(actor_id) = args
        .get("actor_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
    {
        args.insert("by".into(), Value::String(actor_id));
    }
}

#[cfg(test)]
mod tests {
    use super::{daemon_call, normalize_message_author};
    use serde_json::{Map, json};

    #[test]
    fn terminal_resize_maps_to_daemon_operation() {
        let args = json!({"action":"resize","group_id":"g_test","actor_id":"peer1"})
            .as_object()
            .cloned()
            .unwrap_or_else(Map::new);
        let (op, args) = daemon_call("cccc_terminal", args).expect("mapping");
        assert_eq!(op, "terminal_resize");
        assert!(!args.contains_key("action"));
    }

    #[test]
    fn message_actor_id_becomes_daemon_author() {
        for tool in [
            "cccc_message_send",
            "cccc_tracked_send",
            "cccc_message_reply",
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
}
