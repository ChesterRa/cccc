use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout, system_prompt};
use serde_json::json;

pub fn render(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> String {
    let prompt = system_prompt::render_session(home, group, actor);
    if actor.runtime != ActorRuntime::Cursor {
        return prompt;
    }
    let setup = setup_prompt(actor.runtime);
    format!("{setup}\n\n---\n\n{}", prompt.trim_end()) + "\n"
}

fn setup_prompt(runtime: ActorRuntime) -> String {
    let executable = super::codex_mcp::resolve_cccc_executable()
        .map_or_else(|| "cccc".into(), |path| path.to_string_lossy().into_owned());
    let runtime_label = match runtime {
        ActorRuntime::Cursor => "Cursor CLI",
        _ => "this runtime",
    };
    let contract = json!({
        "name": "cccc",
        "transport": "stdio",
        "command": executable,
        "args": ["mcp"],
    });
    format!(
        "[CCCC] MCP setup request\nYou are running inside {runtime_label}. Before setup, check both the available CCCC tools and the registered cccc server command and arguments against the details below.\n\nIdempotency requirement:\n- If cccc_bootstrap is available and the registered command and arguments match, skip MCP setup and continue with the CCCC session bootstrap below. The tool name alone does not establish that this is the current CCCC server.\n- Only when the server is missing or its registration is stale, install or update the \"cccc\" MCP server using this runtime's normal user/global MCP configuration method.\n- Do not reinstall just to verify the config; do not modify unrelated MCP servers. If registration matches but tools are missing or still use an old schema, request a runtime/MCP restart instead of repeatedly rewriting the configuration.\n- The MCP process must inherit CCCC_HOME, CCCC_GROUP_ID and CCCC_ACTOR_ID from this runtime. Do not pin these values in shared user/global configuration: other CCCC instances and Actors have different values.\n\nCCCC MCP server details:\n{}\n\nAfter setup, continue with the CCCC session bootstrap below. If this runtime requires a restart before new MCP tools appear, say so clearly in the terminal.",
        serde_json::to_string_pretty(&contract).unwrap_or_else(|_| "{}".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::GroupStore;

    #[test]
    fn prompt_assisted_runtime_gets_idempotent_mcp_setup_before_preamble() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("test", "").expect("group");
        let mut actor = Actor::new("cursor1");
        actor.runtime = ActorRuntime::Cursor;
        group.actors.push(actor.clone());

        let prompt = render(&home, &group, &actor);
        assert!(prompt.starts_with("[CCCC] MCP setup request\n"));
        assert!(prompt.contains("the registered command and arguments match, skip MCP setup"));
        assert!(prompt.contains("The tool name alone does not establish"));
        assert!(prompt.contains("request a runtime/MCP restart"));
        assert!(prompt.contains("Do not reinstall just to verify the config"));
        assert!(prompt.contains("inherit CCCC_HOME, CCCC_GROUP_ID and CCCC_ACTOR_ID"));
        assert!(!prompt.contains("\"env\""));
        assert!(prompt.contains("[CCCC] You are cursor1"));
    }

    #[test]
    fn managed_kilo_does_not_ask_the_model_to_install_mcp() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("test", "").expect("group");
        let mut actor = Actor::new("kilo-1");
        actor.runtime = ActorRuntime::Kilo;
        assert_eq!(
            render(&home, &group, &actor),
            system_prompt::render_session(&home, &group, &actor)
        );
    }

    #[test]
    fn antigravity_keeps_full_bootstrap_without_model_installed_mcp() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("test", "")
            .expect("group");
        let mut actor = Actor::new("agy");
        actor.runtime = ActorRuntime::Antigravity;
        let prompt = render(&home, &group, &actor);
        assert_eq!(prompt, system_prompt::render_session(&home, &group, &actor));
        assert!(prompt.contains("cccc_bootstrap"));
        assert!(!prompt.contains("MCP setup request"));
    }
}
