use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::mapping;

pub async fn call(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    mut arguments: Map<String, Value>,
) -> Result<Value, String> {
    add_runtime_context(home, &mut arguments);
    let payload = match name {
        "cccc_help" => {
            json!({"markdown": include_str!("../resources/cccc-help.md")})
        }
        "cccc_bootstrap" => return bootstrap(client, arguments).await,
        "cccc_project_info" => return project_info(client, arguments).await,
        "cccc_runtime_list" => json!({"runtimes": [
            "claude","codex","copilot","cursor","devin","kiro","kilo","antigravity",
            "droid","amp","auggie","grok","hermes","kimi","opencode","web_model","custom"
        ]}),
        name if is_repo_tool(name) => {
            return crate::local_tools::call(home, client, name, arguments).await;
        }
        name if crate::remote_tools::is_remote_tool(name) => {
            return crate::remote_tools::call(home, name, arguments).await;
        }
        _ => {
            let (op, args) = mapping::daemon_call(name, arguments)?;
            Value::Object(daemon(client, &op, args).await?)
        }
    };
    Ok(tool_result(payload))
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
    Ok(tool_result(json!({
        "session": {"actor_id": actor_id, "implementation": "rust"},
        "group": group.get("group"), "inbox_preview": inbox, "context": context,
        "next_calls": ["cccc_help", "cccc_inbox_list", "cccc_context_get"]
    })))
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

pub(crate) fn tool_result(payload: Value) -> Value {
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
    json!({"content":[{"type":"text","text":text}],"structuredContent":payload})
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
    use super::apply_actor_context;
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
}
