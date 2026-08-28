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
    call_with_context(home, client, name, arguments, None, false).await
}

pub(crate) async fn call_with_context(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    mut arguments: Map<String, Value>,
    context: Option<RequestContext<'_>>,
    via_capability_use: bool,
) -> Result<Value, String> {
    add_runtime_context(home, &mut arguments);
    if let Some(context) = context {
        apply_request_context(&mut arguments, context);
    }
    if name == "cccc_task" {
        prepare_task_arguments(home, &mut arguments)?;
    }
    authorize_tool(home, name, &arguments, via_capability_use)?;
    if name == "cccc_capability_use" {
        return capability_use(home, client, arguments, context).await;
    }
    let message_operation = is_message_operation(name, &arguments);
    let message_context = message_operation.then(|| {
        (
            arguments
                .get("group_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            arguments
                .get("by")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        )
    });
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
        return result.map(|result| {
            let (group_id, actor_id) = message_context.as_ref().expect("message context");
            with_post_message_context(home, result, group_id, actor_id)
        });
    }
    let payload = match name {
        "cccc_help" => {
            return group_help(client, arguments).await;
        }
        "cccc_bootstrap" => {
            return Ok(tool_result(
                crate::bootstrap::build(home, client, arguments).await?,
            ));
        }
        "cccc_project_info" => return project_info(client, arguments).await,
        "cccc_runtime_list" => json!({"runtimes": cccc_runtime::detect_runtimes()
            .into_iter()
            .map(|runtime| runtime.name)
            .collect::<Vec<_>>() }),
        name if is_repo_tool(name) => {
            let result = crate::local_tools::call(home, client, name, arguments).await?;
            return Ok(if message_operation {
                let (group_id, actor_id) = message_context.as_ref().expect("message context");
                with_post_message_context(home, result, group_id, actor_id)
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
            let mut result = daemon(client, &op, args).await?;
            crate::context_projection::apply(name, &mut result, &arguments);
            if name == "cccc_task" {
                postprocess_task_result(&mut result, &arguments)?;
                if is_task_mail_boundary(&arguments) {
                    let group_id = arguments
                        .get("group_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let actor_id = arguments
                        .get("by")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    insert_mail_pending_context(home, &mut result, group_id, actor_id);
                }
            }
            if name == "cccc_actor_notes" {
                postprocess_actor_notes(client, &mut result, &arguments).await;
            }
            Value::Object(result)
        }
    };
    let result = tool_result(payload);
    Ok(if message_operation {
        let (group_id, actor_id) = message_context.as_ref().expect("message context");
        with_post_message_context(home, result, group_id, actor_id)
    } else {
        result
    })
}

fn authorize_tool(
    home: &HomeLayout,
    name: &str,
    arguments: &Map<String, Value>,
    via_capability_use: bool,
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
    let group_id = arguments
        .get("group_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let group = if actor_id == "user" || group_id.is_empty() {
        None
    } else {
        Some(
            cccc_core::GroupStore::new(home.clone())
                .and_then(|store| store.load(group_id))
                .map_err(|error| error.to_string())?,
        )
    };
    if let Some(group) = group.as_ref()
        && group
            .actors
            .iter()
            .find(|actor| actor.id == actor_id)
            .is_some_and(|actor| actor.runtime == cccc_contracts::ActorRuntime::WebModel)
    {
        let role = cccc_core::actors::effective_role(group, actor_id)
            .unwrap_or(cccc_contracts::ActorRole::Peer);
        if cccc_core::WEB_MODEL_CORE_TOOL_NAMES.contains(&name)
            || (via_capability_use
                && role == cccc_contracts::ActorRole::Foreman
                && cccc_core::is_builtin_capability_pack_tool(name))
        {
            return Ok(());
        }
        return if role == cccc_contracts::ActorRole::Peer {
            Err(format!("{name} requires a Web Model foreman actor"))
        } else {
            Err(format!("{name} is not available to Web Model actors"))
        };
    }
    if !matches!(
        name,
        "cccc_capability_import" | "cccc_capability_block" | "cccc_capability_uninstall"
    ) || actor_id == "user"
    {
        return Ok(());
    }
    if group_id.is_empty() {
        return Err("group_id is required".into());
    }
    let group = match group {
        Some(group) => group,
        None => cccc_core::GroupStore::new(home.clone())
            .and_then(|store| store.load(group_id))
            .map_err(|error| error.to_string())?,
    };
    let peer = cccc_core::actors::effective_role(&group, actor_id)
        == Some(cccc_contracts::ActorRole::Peer);
    if peer {
        Err(format!("{name} is not available to peer actors"))
    } else {
        Ok(())
    }
}

async fn capability_use(
    home: &HomeLayout,
    client: &DaemonClient,
    arguments: Map<String, Value>,
    context: Option<RequestContext<'_>>,
) -> Result<Value, String> {
    let group_id = arguments
        .get("group_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let actor_id = arguments
        .get("actor_id")
        .and_then(Value::as_str)
        .or_else(|| arguments.get("by").and_then(Value::as_str))
        .unwrap_or("user")
        .trim()
        .to_owned();
    let by = arguments
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or(&actor_id)
        .trim()
        .to_owned();
    let requested_scope = arguments
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("session")
        .trim()
        .to_ascii_lowercase();
    let ttl_seconds = arguments
        .get("ttl_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(3600)
        .clamp(60, 86_400);
    let reason = arguments
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let tool_name = arguments
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if tool_name == "cccc_capability_use" {
        return Err(
            "capability_use_invalid_tool: cccc_capability_use cannot recursively call itself"
                .into(),
        );
    }
    let mut capability_id = arguments
        .get("capability_id")
        .or_else(|| arguments.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();

    let state_probe = if tool_name.is_empty() {
        Map::new()
    } else {
        capability_state(client, &group_id, &actor_id, &by)
            .await
            .unwrap_or_default()
    };
    if capability_id.is_empty() && !tool_name.is_empty() {
        let mut candidates = cccc_core::capabilities::CapabilityStore::new(home.clone())
            .catalog()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|capability| capability.tool_names.iter().any(|name| name == &tool_name))
            .map(|capability| capability.id)
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        if candidates.len() > 1 {
            return Err(format!(
                "capability_use_ambiguous_tool: tool maps to multiple capabilities: {tool_name} ({})",
                candidates.join(", ")
            ));
        }
        capability_id = candidates.pop().unwrap_or_default();
        if capability_id.is_empty() {
            let mut dynamic_candidates = state_probe
                .get("dynamic_tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|tool| {
                    tool.get("name").and_then(Value::as_str) == Some(tool_name.as_str())
                        || tool.get("real_tool_name").and_then(Value::as_str)
                            == Some(tool_name.as_str())
                })
                .filter_map(|tool| tool.get("capability_id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            dynamic_candidates.sort();
            dynamic_candidates.dedup();
            if dynamic_candidates.len() > 1 {
                return Err(format!(
                    "capability_use_ambiguous_tool: tool maps to multiple capabilities: {tool_name} ({})",
                    dynamic_candidates.join(", ")
                ));
            }
            capability_id = dynamic_candidates.pop().unwrap_or_default();
        }
        if capability_id.is_empty() && crate::is_core_tool(&tool_name) {
            capability_id = "core".into();
        }
    }
    if capability_id.is_empty() {
        return Err(
            "missing_capability_id: pass capability_id when it cannot be inferred from tool_name"
                .into(),
        );
    }

    let already_enabled = state_probe
        .get("enabled_capabilities")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(&capability_id))
        });
    let reused_existing_binding = !tool_name.is_empty() && already_enabled;
    let enable_result = if capability_id == "core" {
        json!({
            "state":"runnable","enabled":true,"refresh_required":false,"scope":"core"
        })
        .as_object()
        .cloned()
        .expect("core enable result")
    } else if reused_existing_binding {
        json!({
            "state":"runnable","enabled":true,"refresh_required":false,
            "reused_existing_binding":true,"scope":requested_scope
        })
        .as_object()
        .cloned()
        .expect("reused enable result")
    } else {
        daemon(
            client,
            "capability_enable",
            json!({
                "group_id":group_id,
                "actor_id":actor_id,
                "by":by,
                "capability_id":capability_id,
                "scope":requested_scope,
                "enabled":true,
                "ttl_seconds":ttl_seconds,
                "reason":reason,
            })
            .as_object()
            .cloned()
            .expect("capability enable args"),
        )
        .await?
    };
    let state = enable_result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("runnable")
        .trim()
        .to_ascii_lowercase();
    let enabled = matches!(
        state.as_str(),
        "ready" | "activation_pending" | "runnable" | "verified"
    );
    let scope = enable_result
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or(&requested_scope)
        .to_owned();
    if !enabled || tool_name.is_empty() {
        return Ok(tool_result(json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "capability_id":capability_id,
            "scope":scope,
            "requested_scope":requested_scope,
            "enabled":enabled,
            "state":state,
            "refresh_required":enable_result
                .get("refresh_required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "enable_result":enable_result,
            "tool_called":false,
        })));
    }

    let mut tool_arguments = arguments
        .get("tool_arguments")
        .or_else(|| arguments.get("arguments"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    tool_arguments
        .entry("group_id")
        .or_insert_with(|| Value::String(group_id.clone()));
    tool_arguments
        .entry("by")
        .or_insert_with(|| Value::String(by.clone()));
    tool_arguments
        .entry("actor_id")
        .or_insert_with(|| Value::String(actor_id.clone()));
    let nested = Box::pin(call_with_context(
        home,
        client,
        &tool_name,
        tool_arguments,
        context,
        true,
    ))
    .await?;
    let nested_payload = nested.get("structuredContent").cloned().unwrap_or(nested);
    Ok(tool_result(json!({
        "group_id":group_id,
        "actor_id":actor_id,
        "capability_id":capability_id,
        "scope":scope,
        "requested_scope":requested_scope,
        "enabled":true,
        "state":"verified",
        "refresh_required":false,
        "verification_source":"tool_call",
        "reused_existing_binding":reused_existing_binding,
        "enable_result":enable_result,
        "tool_called":true,
        "tool_name":tool_name,
        "tool_result":nested_payload,
    })))
}

async fn capability_state(
    client: &DaemonClient,
    group_id: &str,
    actor_id: &str,
    by: &str,
) -> Result<Map<String, Value>, String> {
    daemon(
        client,
        "capability_state",
        json!({"group_id":group_id,"actor_id":actor_id,"by":by})
            .as_object()
            .cloned()
            .expect("capability state args"),
    )
    .await
}

fn prepare_task_arguments(
    home: &HomeLayout,
    arguments: &mut Map<String, Value>,
) -> Result<(), String> {
    if arguments.get("action").and_then(Value::as_str) != Some("create")
        || arguments.contains_key("assignee")
    {
        return Ok(());
    }
    let by = arguments
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if by.is_empty() || matches!(by.as_str(), "user" | "system") {
        return Ok(());
    }
    let group_id = arguments
        .get("group_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "group_id is required".to_owned())?;
    let group = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(|error| error.to_string())?;
    if cccc_core::actors::effective_role(&group, &by) == Some(cccc_contracts::ActorRole::Peer) {
        arguments.insert("assignee".into(), Value::String(by));
    }
    Ok(())
}

fn postprocess_task_result(
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) -> Result<(), String> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("list");
    if !matches!(action, "get" | "list")
        || arguments
            .get("include_archived")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return Ok(());
    }
    if result
        .get("task")
        .and_then(Value::as_object)
        .and_then(|task| task.get("status"))
        .and_then(Value::as_str)
        == Some("archived")
    {
        return Err("archived_hidden: archived task is hidden by default".into());
    }
    if let Some(tasks) = result.get_mut("tasks").and_then(Value::as_array_mut) {
        tasks.retain(|task| task.get("status").and_then(Value::as_str) != Some("archived"));
    }
    Ok(())
}

fn is_task_mail_boundary(arguments: &Map<String, Value>) -> bool {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("list");
    if !matches!(action, "create" | "update" | "move") {
        return false;
    }
    if arguments.get("status").and_then(Value::as_str) == Some("done") {
        return true;
    }
    matches!(
        arguments.get("waiting_on").and_then(Value::as_str),
        Some("actor" | "external")
    ) || arguments
        .get("blocked_by")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
}

fn help_markdown() -> String {
    format!(
        "{}\n\n{}\n",
        cccc_core::group_prompts::BUILTIN_HELP_MARKDOWN.trim_end(),
        cccc_core::peer_insight::PEER_INSIGHT_RUNTIME_HELP.as_str()
    )
}

async fn group_help(client: &DaemonClient, arguments: Map<String, Value>) -> Result<Value, String> {
    if !arguments.contains_key("group_id") {
        return Ok(tool_result(json!({
            "markdown": help_markdown(),
            "source": "resources/cccc-help.md",
        })));
    }
    let result = daemon(client, "group_help_get", arguments).await?;
    let base = result
        .get("markdown")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source = result
        .get("source_path")
        .and_then(Value::as_str)
        .unwrap_or("resources/cccc-help.md");
    Ok(tool_result(json!({
        "markdown": append_peer_insight_runtime_help(base),
        "source": source,
    })))
}

fn append_peer_insight_runtime_help(markdown: &str) -> String {
    let reserved = "## Peer Insight Contract (Runtime)";
    let mut kept = Vec::new();
    let mut current = Vec::new();
    let mut skip = false;
    let flush = |kept: &mut Vec<String>, current: &mut Vec<String>, skip: bool| {
        if !skip {
            kept.append(current);
        } else {
            current.clear();
        }
    };
    for line in markdown.lines() {
        if line.starts_with("## ") && !line.starts_with("### ") {
            flush(&mut kept, &mut current, skip);
            skip = line.trim() == reserved;
        }
        current.push(line.to_owned());
    }
    flush(&mut kept, &mut current, skip);
    format!(
        "{}\n\n{}\n",
        kept.join("\n").trim_end(),
        cccc_core::peer_insight::PEER_INSIGHT_RUNTIME_HELP.as_str()
    )
}

async fn postprocess_actor_notes(
    client: &DaemonClient,
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("get");
    let changed = result
        .remove("changed")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !matches!(action, "set" | "clear") || !changed {
        if matches!(action, "set" | "clear") {
            result.insert("notified_actor_ids".into(), json!([]));
        }
        return;
    }
    let Some(group_id) = arguments.get("group_id").and_then(Value::as_str) else {
        result.insert("notified_actor_ids".into(), json!([]));
        return;
    };
    let target = result
        .get("target_actor_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut list_args = Map::new();
    list_args.insert("group_id".into(), Value::String(group_id.into()));
    let actors = daemon(client, "actor_list", list_args)
        .await
        .ok()
        .and_then(|value| value.get("actors").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    let running = actors.iter().find(|actor| {
        actor.get("id").and_then(Value::as_str) == Some(target)
            && actor.get("running").and_then(Value::as_bool) == Some(true)
    });
    if running.is_none() {
        result.insert("notified_actor_ids".into(), json!([]));
        return;
    }
    let notify_args = json!({
        "group_id": group_id,
        "by": "system",
        "kind": "info",
        "priority": "normal",
        "title": "Help updated: your actor note",
        "message": "Updated: your actor note. Run `cccc_help` now to refresh your effective protocol reference; then update `cccc_agent_state` if your plan changes.",
        "target_actor_id": target,
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    let notified = daemon(client, "system_notify", notify_args).await.is_ok();
    result.insert(
        "notified_actor_ids".into(),
        if notified { json!([target]) } else { json!([]) },
    );
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

fn with_post_message_context(
    home: &HomeLayout,
    mut result: Value,
    group_id: &str,
    actor_id: &str,
) -> Value {
    if let Some(payload) = result
        .get_mut("structuredContent")
        .and_then(Value::as_object_mut)
    {
        insert_mail_pending_context(home, payload, group_id, actor_id);
        let text = serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".into());
        result["content"] = json!([{"type":"text","text":text}]);
    }
    result
}

fn insert_mail_pending_context(
    home: &HomeLayout,
    payload: &mut Map<String, Value>,
    group_id: &str,
    actor_id: &str,
) {
    if group_id.is_empty() || matches!(actor_id, "" | "user" | "system") {
        return;
    }
    let Ok(store) = cccc_core::GroupStore::new(home.clone()) else {
        return;
    };
    let Ok(group) = store.load(group_id) else {
        return;
    };
    let Ok(Some(pending)) = cccc_core::inbox::mail_pending_summary(home, &group, actor_id) else {
        return;
    };
    payload.insert("mail_pending".into(), pending);
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
        apply_actor_context, apply_request_context, help_markdown, is_message_operation,
        is_task_mail_boundary, postprocess_task_result, prepare_task_arguments,
    };
    use crate::RequestContext;
    use serde_json::json;

    #[test]
    fn web_model_tool_authorization_is_role_aware_and_capability_routed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");

        let mut foreman_group = store.create("web model foreman", "").expect("group");
        let mut web_foreman = cccc_contracts::Actor::new("web-foreman");
        web_foreman.runtime = cccc_contracts::ActorRuntime::WebModel;
        cccc_core::actors::add(&mut foreman_group, web_foreman).expect("web foreman");
        store.save(&foreman_group).expect("save foreman group");
        let foreman_args = json!({
            "group_id":foreman_group.group_id,
            "by":"web-foreman"
        })
        .as_object()
        .cloned()
        .expect("foreman args");
        assert!(super::authorize_tool(&home, "cccc_repo", &foreman_args, false).is_ok());
        assert_eq!(
            super::authorize_tool(&home, "cccc_actor", &foreman_args, false)
                .expect_err("direct management call must fail"),
            "cccc_actor is not available to Web Model actors"
        );
        assert!(super::authorize_tool(&home, "cccc_actor", &foreman_args, true).is_ok());

        let mut peer_group = store.create("web model peer", "").expect("group");
        cccc_core::actors::add(
            &mut peer_group,
            cccc_contracts::Actor::new("native-foreman"),
        )
        .expect("native foreman");
        let mut web_peer = cccc_contracts::Actor::new("web-peer");
        web_peer.runtime = cccc_contracts::ActorRuntime::WebModel;
        cccc_core::actors::add(&mut peer_group, web_peer).expect("web peer");
        cccc_core::actors::add(&mut peer_group, cccc_contracts::Actor::new("native-peer"))
            .expect("native peer");
        store.save(&peer_group).expect("save peer group");
        let peer_args = json!({"group_id":peer_group.group_id,"by":"web-peer"})
            .as_object()
            .cloned()
            .expect("peer args");
        assert_eq!(
            super::authorize_tool(&home, "cccc_actor", &peer_args, true)
                .expect_err("peer management call must fail"),
            "cccc_actor requires a Web Model foreman actor"
        );
        let native_peer_args = json!({
            "group_id":peer_group.group_id,
            "by":"native-peer"
        })
        .as_object()
        .cloned()
        .expect("native peer args");
        assert_eq!(
            super::authorize_tool(&home, "cccc_capability_import", &native_peer_args, false,)
                .expect_err("peer capability administration must fail"),
            "cccc_capability_import is not available to peer actors"
        );
    }

    #[tokio::test]
    async fn capability_use_enables_hidden_pack_and_calls_inferred_builtin() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .and_then(|store| store.create("capability use", ""))
            .expect("group");
        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        let client = cccc_client::DaemonClient::new(home.clone());
        for _ in 0..100 {
            if client
                .call(&cccc_contracts::DaemonRequest {
                    v: 1,
                    op: "group_list".into(),
                    args: serde_json::Map::new(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }

        let context = Some(RequestContext {
            group_id: &group.group_id,
            actor_id: "user",
        });
        let enabled = super::call_with_context(
            &home,
            &client,
            "cccc_capability_use",
            json!({"capability_id":"pack:space","scope":"session"})
                .as_object()
                .cloned()
                .expect("enable args"),
            context,
            false,
        )
        .await
        .expect("enable pack");
        assert_eq!(enabled["structuredContent"]["enabled"], true);
        assert_eq!(enabled["structuredContent"]["capability_id"], "pack:space");
        assert_eq!(enabled["structuredContent"]["tool_called"], false);

        let state = super::daemon(
            &client,
            "capability_state",
            json!({"group_id":group.group_id,"actor_id":"user","by":"user"})
                .as_object()
                .cloned()
                .expect("state args"),
        )
        .await
        .expect("capability state");
        assert!(
            state["enabled_capabilities"]
                .as_array()
                .expect("enabled capabilities")
                .contains(&json!("pack:space"))
        );

        let called = super::call_with_context(
            &home,
            &client,
            "cccc_capability_use",
            json!({"tool_name":"cccc_project_info","tool_arguments":{}})
                .as_object()
                .cloned()
                .expect("call args"),
            context,
            false,
        )
        .await
        .expect("call inferred built-in");
        assert_eq!(
            called["structuredContent"]["capability_id"],
            "pack:context-advanced"
        );
        assert_eq!(called["structuredContent"]["tool_called"], true);
        assert_eq!(
            called["structuredContent"]["tool_name"],
            "cccc_project_info"
        );

        daemon_task.abort();
    }

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
    fn task_create_defaults_peer_assignment_and_list_hides_archived_tasks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("tasks", "").expect("group");
        store
            .mutate(&group.group_id, |group| {
                cccc_core::actors::add(group, cccc_contracts::Actor::new("lead"))?;
                cccc_core::actors::add(group, cccc_contracts::Actor::new("peer"))?;
                Ok(())
            })
            .expect("actors");
        let mut args = json!({
            "action":"create","group_id":group.group_id,"actor_id":"peer","by":"peer",
            "title":"self-owned"
        })
        .as_object()
        .cloned()
        .expect("args");
        prepare_task_arguments(&home, &mut args).expect("prepare task");
        assert_eq!(args["assignee"], "peer");

        let mut result = json!({
            "tasks":[
                {"id":"T001","status":"planned"},
                {"id":"T002","status":"archived"}
            ]
        })
        .as_object()
        .cloned()
        .expect("result");
        postprocess_task_result(
            &mut result,
            &json!({"action":"list"})
                .as_object()
                .cloned()
                .expect("list args"),
        )
        .expect("filter");
        assert_eq!(result["tasks"].as_array().expect("tasks").len(), 1);
        assert_eq!(result["tasks"][0]["id"], "T001");
    }

    #[test]
    fn identifies_message_operations_for_compact_mail_context() {
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
    }

    #[test]
    fn identifies_task_completion_and_block_boundaries_for_mail_context() {
        for arguments in [
            json!({"action":"move","status":"done"}),
            json!({"action":"update","waiting_on":"external"}),
            json!({"action":"update","blocked_by":["T001"]}),
        ] {
            assert!(is_task_mail_boundary(
                arguments.as_object().expect("arguments")
            ));
        }
        assert!(!is_task_mail_boundary(
            json!({"action":"update","status":"active"})
                .as_object()
                .expect("active arguments")
        ));
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
