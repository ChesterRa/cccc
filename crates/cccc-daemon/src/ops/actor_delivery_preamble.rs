use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout, system_prompt};
use serde_json::json;

pub fn render(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> String {
    let prompt = system_prompt::render_session(home, group, actor);
    if !matches!(
        actor.runtime,
        ActorRuntime::Antigravity | ActorRuntime::Cursor
    ) {
        return prompt;
    }
    let setup = setup_prompt(home, actor.runtime);
    format!("{setup}\n\n---\n\n{}", prompt.trim_end()) + "\n"
}

fn setup_prompt(home: &HomeLayout, runtime: ActorRuntime) -> String {
    let executable = super::codex_mcp::resolve_cccc_executable()
        .map_or_else(|| "cccc".into(), |path| path.to_string_lossy().into_owned());
    let runtime_label = match runtime {
        ActorRuntime::Antigravity => "Antigravity",
        ActorRuntime::Cursor => "Cursor CLI",
        _ => "this runtime",
    };
    let contract = json!({
        "name": "cccc",
        "transport": "stdio",
        "command": executable,
        "args": ["mcp"],
        "env": {"CCCC_HOME": home.root()},
    });
    format!(
        "[CCCC] MCP setup request\nYou are running inside {runtime_label}. Before setup, check whether cccc_bootstrap is available in this session.\n\nIdempotency requirement:\n- If cccc_bootstrap is available, skip MCP setup entirely and continue with the CCCC session bootstrap below.\n- Only when cccc_bootstrap is not available, install or update the \"cccc\" MCP server using this runtime's normal user/global MCP configuration method.\n- Do not reinstall just to verify the config; do not modify unrelated MCP servers.\n\nCCCC MCP server details:\n{}\n\nAfter setup, continue with the CCCC session bootstrap below. If this runtime requires a restart before new MCP tools appear, say so clearly in the terminal.",
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
        assert!(prompt.contains("If cccc_bootstrap is available, skip MCP setup entirely"));
        assert!(prompt.contains("Do not reinstall just to verify the config"));
        assert!(prompt.contains("\"CCCC_HOME\""));
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
}
