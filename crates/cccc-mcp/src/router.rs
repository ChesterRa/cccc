use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::RequestContext;
use crate::mapping;

pub async fn call(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    arguments: Map<String, Value>,
) -> Result<Value, String> {
    call_with_context(home, client, name, arguments, None).await
}

pub(crate) async fn call_with_context(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    mut arguments: Map<String, Value>,
    context: Option<RequestContext<'_>>,
) -> Result<Value, String> {
    add_runtime_context(home, &mut arguments);
    if let Some(context) = context {
        apply_request_context(&mut arguments, context);
    }
    authorize_tool(home, name, &arguments)?;
    let message_operation = is_message_operation(name, &arguments);
    if message_operation {
        arguments.insert("require_peer_insight".into(), Value::Bool(true));
    }
    if name == "cccc_message_send" {
        crate::remote_messages::apply_cross_group_default(&mut arguments)?;
    }
    if matches!(name, "cccc_message_send" | "cccc_message_reply")
        && let Some(result) =
            crate::remote_messages::try_send(home, client, arguments.clone()).await
    {
        return result.map(with_post_message_nudge);
    }
    let payload = match name {
        "cccc_help" => {
            json!({"markdown": help_markdown()})
        }
        "cccc_bootstrap" => return bootstrap(client, arguments).await,
        "cccc_project_info" => return project_info(client, arguments).await,
        "cccc_runtime_list" => json!({"runtimes": cccc_runtime::detect_runtimes()
            .into_iter()
            .map(|runtime| runtime.name)
            .collect::<Vec<_>>() }),
        name if is_repo_tool(name) => {
            let result = crate::local_tools::call(home, client, name, arguments).await?;
            return Ok(if message_operation {
                with_post_message_nudge(result)
            } else {
                result
            });
        }
        name if crate::remote_tools::is_remote_tool(name) => {
            return crate::remote_tools::call(home, name, arguments).await;
        }
        _ => {
            let (op, args) = match mapping::daemon_call(name, arguments.clone()) {
                Ok(mapped) => mapped,
                Err(error) if error.starts_with("tool is not a daemon operation:") => {
                    let mut dynamic = Map::new();
                    if let Some(value) = arguments.get("group_id").cloned() {
                        dynamic.insert("group_id".into(), value);
                    }
                    if let Some(value) = arguments.get("actor_id").cloned() {
                        dynamic.insert("actor_id".into(), value);
                    }
                    if let Some(value) = arguments.get("by").cloned() {
                        dynamic.insert("by".into(), value);
                    }
                    dynamic.insert("tool_name".into(), Value::String(name.into()));
                    dynamic.insert("arguments".into(), Value::Object(arguments));
                    return Ok(tool_result(Value::Object(
                        daemon(client, "capability_tool_call", dynamic).await?,
                    )));
                }
                Err(error) => return Err(error),
            };
            Value::Object(daemon(client, &op, args).await?)
        }
    };
    let result = tool_result(payload);
    Ok(if message_operation {
        with_post_message_nudge(result)
    } else {
        result
    })
}

fn authorize_tool(
    home: &HomeLayout,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<(), String> {
    let actor_id = arguments
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or("user");
    if name.starts_with("cccc_voice_secretary_") && actor_id != "voice-secretary" {
        return Err(format!(
            "{name} is only available to the voice-secretary actor"
        ));
    }
    if !matches!(
        name,
        "cccc_capability_import" | "cccc_capability_block" | "cccc_capability_uninstall"
    ) || actor_id == "user"
    {
        return Ok(());
    }
    let Some(group_id) = arguments.get("group_id").and_then(Value::as_str) else {
        return Err("group_id is required".into());
    };
    let group = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(|error| error.to_string())?;
    let peer = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .and_then(|actor| actor.role)
        == Some(cccc_contracts::ActorRole::Peer);
    if peer {
        Err(format!("{name} is not available to peer actors"))
    } else {
        Ok(())
    }
}

fn help_markdown() -> String {
    format!(
        "{}\n\n{}\n",
        include_str!("../resources/cccc-help.md").trim_end(),
        cccc_core::peer_insight::PEER_INSIGHT_RUNTIME_HELP.as_str()
    )
}

async fn bootstrap(client: &DaemonClient, args: Map<String, Value>) -> Result<Value, String> {
    let group_id = args
        .get("group_id")
        .cloned()
        .ok_or_else(|| "group_id is required".to_owned())?;
    let actor_id = args
        .get("actor_id")
        .cloned()
        .unwrap_or_else(|| Value::String("user".into()));
    let mut group_args = Map::new();
    group_args.insert("group_id".into(), group_id.clone());
    let group = daemon(client, "group_show", group_args).await?;
    let mut inbox_args = Map::new();
    inbox_args.insert("group_id".into(), group_id.clone());
    inbox_args.insert("actor_id".into(), actor_id.clone());
    inbox_args.insert(
        "limit".into(),
        args.get("inbox_limit")
            .cloned()
            .unwrap_or_else(|| json!(50)),
    );
    let inbox = daemon(client, "inbox_list", inbox_args).await?;
    let mut context_args = Map::new();
    context_args.insert("group_id".into(), group_id);
    let context = daemon(client, "context_get", context_args).await?;
    let recovery = bootstrap_recovery(&context, actor_id.as_str().unwrap_or("user"));
    Ok(tool_result(json!({
        "session": {"actor_id": actor_id, "implementation": "rust"},
        "group": group.get("group"), "inbox_preview": inbox, "recovery":recovery, "context": context,
        "next_calls": ["cccc_help", "cccc_inbox_list", "cccc_context_get"]
    })))
}

fn bootstrap_recovery(context: &Map<String, Value>, actor_id: &str) -> Value {
    let root = context.get("context").unwrap_or(&Value::Null);
    if has_native_recoverable_task(root, actor_id) || has_recoverable_work(root) {
        json!({"takeover_nudge":cccc_core::peer_insight::BOOTSTRAP_TAKEOVER_NUDGE})
    } else {
        json!({})
    }
}

fn has_native_recoverable_task(context: &Value, actor_id: &str) -> bool {
    context
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .any(|task| {
            let status = task
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("planned")
                .trim()
                .to_ascii_lowercase();
            if matches!(
                status.as_str(),
                "done" | "completed" | "cancelled" | "canceled" | "archived"
            ) {
                return false;
            }
            let assigned = ["assignee", "handoff_to"].into_iter().any(|key| {
                task.get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.trim() == actor_id)
            });
            let waiting_on = task
                .get("waiting_on")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .trim();
            let waiting_for_actor = waiting_on == "actor" && actor_id != "user";
            let waiting_for_user = waiting_on == "user" && actor_id == "user";
            let attention = task.get("priority").and_then(Value::as_str) == Some("attention");
            assigned || waiting_for_actor || waiting_for_user || attention
        })
}

fn has_recoverable_work(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            let scalar_work = ["active_task_id", "focus", "next_action", "current_focus"]
                .into_iter()
                .any(|key| {
                    object
                        .get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                });
            let list_work = ["blockers", "open_loops", "commitments"]
                .into_iter()
                .any(|key| {
                    object
                        .get(key)
                        .and_then(Value::as_array)
                        .is_some_and(|items| {
                            items.iter().any(|item| match item {
                                Value::String(value) => !value.trim().is_empty(),
                                Value::Object(value) => !value.is_empty(),
                                _ => false,
                            })
                        })
                });
            let task_work = ["assigned_active", "attention"].into_iter().any(|key| {
                object
                    .get(key)
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item.as_object().is_some_and(|item| !item.is_empty()))
                    })
            });
            scalar_work || list_work || task_work || object.values().any(has_recoverable_work)
        }
        Value::Array(items) => items.iter().any(has_recoverable_work),
        _ => false,
    }
}

async fn project_info(client: &DaemonClient, args: Map<String, Value>) -> Result<Value, String> {
    let group_id = args
        .get("group_id")
        .cloned()
        .ok_or_else(|| "group_id is required".to_owned())?;
    let mut daemon_args = Map::new();
    daemon_args.insert("group_id".into(), group_id);
    let result = daemon(client, "group_show", daemon_args).await?;
    let group: GroupDoc =
        serde_json::from_value(result.get("group").cloned().unwrap_or(Value::Null))
            .map_err(|error| error.to_string())?;
    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first());
    let Some(scope) = scope else {
        return Ok(tool_result(json!({"content":"", "scope":null})));
    };
    let root = std::path::Path::new(&scope.url);
    let path = ["PROJECT.md", "README.md"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file());
    let content = path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    Ok(tool_result(
        json!({"content": content, "path": path, "scope": scope}),
    ))
}

pub(crate) async fn daemon(
    client: &DaemonClient,
    op: &str,
    args: Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .map_err(|error| error.to_string())?;
    if response.ok {
        return Ok(response.result);
    }
    Err(response.error.map_or_else(
        || "daemon operation failed".into(),
        |error| format!("{}: {}", error.code, error.message),
    ))
}

fn add_runtime_context(home: &HomeLayout, args: &mut Map<String, Value>) {
    if !args.contains_key("group_id") {
        let group = std::env::var("CCCC_GROUP_ID")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| cccc_core::active::get(home).ok().flatten());
        if let Some(group) = group {
            args.insert("group_id".into(), Value::String(group));
        }
    }
    let actor = std::env::var("CCCC_ACTOR_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    apply_actor_context(args, actor.as_deref());
}

fn apply_actor_context(args: &mut Map<String, Value>, actor: Option<&str>) {
    if let Some(actor) = actor.map(str::trim).filter(|actor| !actor.is_empty()) {
        args.entry("actor_id")
            .or_insert_with(|| Value::String(actor.to_owned()));
        // The process environment is set by the runtime and is authoritative.
        // Tool arguments are model-controlled and must not be able to impersonate user.
        args.insert("by".into(), Value::String(actor.to_owned()));
    }
}

fn apply_request_context(args: &mut Map<String, Value>, context: RequestContext<'_>) {
    // A remote connector is bound to exactly one actor and group. Its request
    // arguments are model-controlled, so the request-scoped binding is authoritative.
    args.insert(
        "group_id".into(),
        Value::String(context.group_id.to_owned()),
    );
    args.insert(
        "actor_id".into(),
        Value::String(context.actor_id.to_owned()),
    );
    args.insert("by".into(), Value::String(context.actor_id.to_owned()));
}

pub(crate) fn tool_result(payload: Value) -> Value {
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
    json!({"content":[{"type":"text","text":text}],"structuredContent":payload})
}

fn is_message_operation(name: &str, arguments: &Map<String, Value>) -> bool {
    matches!(
        name,
        "cccc_message_send" | "cccc_tracked_send" | "cccc_message_reply"
    ) || (name == "cccc_file" && arguments.get("action").and_then(Value::as_str) == Some("send"))
}

fn with_post_message_nudge(mut result: Value) -> Value {
    let nudge = json!({
        "kind":"whole_situation_reconstruction",
        "message":cccc_core::peer_insight::POST_MESSAGE_NUDGE
    });
    if let Some(payload) = result
        .get_mut("structuredContent")
        .and_then(Value::as_object_mut)
    {
        payload.insert("post_message_nudge".into(), nudge);
        let text = serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".into());
        result["content"] = json!([{"type":"text","text":text}]);
    }
    result
}

fn is_repo_tool(name: &str) -> bool {
    matches!(
        name,
        "cccc_repo"
            | "cccc_repo_edit"
            | "cccc_apply_patch"
            | "cccc_shell"
            | "cccc_exec_command"
            | "cccc_write_stdin"
            | "cccc_git"
            | "cccc_code_exec"
            | "cccc_code_wait"
            | "cccc_file"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        apply_actor_context, apply_request_context, bootstrap_recovery, help_markdown,
        is_message_operation, with_post_message_nudge,
    };
    use crate::RequestContext;
    use serde_json::json;

    #[test]
    fn runtime_actor_is_authoritative_but_does_not_replace_target_actor() {
        let mut args = json!({"actor_id":"target-peer","by":"user"})
            .as_object()
            .cloned()
            .expect("args");

        apply_actor_context(&mut args, Some("backend"));

        assert_eq!(args["actor_id"], "target-peer");
        assert_eq!(args["by"], "backend");
    }

    #[test]
    fn runtime_actor_populates_missing_self_context() {
        let mut args = serde_json::Map::new();

        apply_actor_context(&mut args, Some("backend"));

        assert_eq!(args["actor_id"], "backend");
        assert_eq!(args["by"], "backend");
    }

    #[test]
    fn request_scoped_actor_binding_overrides_model_controlled_identity() {
        let mut args = json!({"group_id":"other","actor_id":"other","by":"user"})
            .as_object()
            .cloned()
            .expect("args");

        apply_request_context(
            &mut args,
            RequestContext {
                group_id: "bound-group",
                actor_id: "bound-actor",
            },
        );

        assert_eq!(args["group_id"], "bound-group");
        assert_eq!(args["actor_id"], "bound-actor");
        assert_eq!(args["by"], "bound-actor");
    }

    #[test]
    fn identifies_message_operations_and_adds_reconstruction_nudge() {
        assert!(is_message_operation(
            "cccc_message_send",
            &serde_json::Map::new()
        ));
        assert!(is_message_operation(
            "cccc_file",
            &json!({"action":"send"})
                .as_object()
                .cloned()
                .expect("send args")
        ));
        assert!(!is_message_operation(
            "cccc_file",
            &json!({"action":"read"})
                .as_object()
                .cloned()
                .expect("read args")
        ));
        let result = with_post_message_nudge(super::tool_result(json!({"event":{}})));
        assert_eq!(
            result["structuredContent"]["post_message_nudge"]["kind"],
            "whole_situation_reconstruction"
        );
    }

    #[test]
    fn bootstrap_takeover_requires_unfinished_recovery_material() {
        let active = json!({"context":{"agent_state":{"hot":{"focus":"finish migration"}}}})
            .as_object()
            .cloned()
            .expect("active context");
        assert_eq!(
            bootstrap_recovery(&active, "peer1")["takeover_nudge"],
            cccc_core::peer_insight::BOOTSTRAP_TAKEOVER_NUDGE
        );
        let objective_only = json!({"context":{"coordination":{"objective":"ship"}}})
            .as_object()
            .cloned()
            .expect("objective context");
        assert_eq!(bootstrap_recovery(&objective_only, "peer1"), json!({}));

        let native_task = json!({"context":{"tasks":[{
            "id":"t_1","status":"planned","assignee":"peer1","waiting_on":"none"
        }]}})
        .as_object()
        .cloned()
        .expect("native task context");
        assert_eq!(
            bootstrap_recovery(&native_task, "peer1")["takeover_nudge"],
            cccc_core::peer_insight::BOOTSTRAP_TAKEOVER_NUDGE
        );
        assert_eq!(bootstrap_recovery(&native_task, "peer2"), json!({}));

        let completed = json!({"context":{"tasks":[{
            "id":"t_2","status":"done","assignee":"peer1","priority":"attention"
        }]}})
        .as_object()
        .cloned()
        .expect("completed task context");
        assert_eq!(bootstrap_recovery(&completed, "peer1"), json!({}));
    }

    #[test]
    fn help_uses_the_complete_shared_peer_insight_contract() {
        let help = help_markdown();
        for required in [
            "one move on a living\ndecision path",
            "where reality could break it",
            "switch to Plan B",
            "one fallible projection of the situation",
            "do not inherit the level or frame it claims",
            "clear-sighted, exacting supervisor",
        ] {
            assert!(help.contains(required), "missing help contract: {required}");
        }
        assert_eq!(help.matches("## Peer Insight Contract").count(), 1);
    }
}
