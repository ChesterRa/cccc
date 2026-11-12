use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

type NativeKey = (PathBuf, String, String);

fn native_records() -> &'static Mutex<HashMap<NativeKey, String>> {
    static RECORDS: OnceLock<Mutex<HashMap<NativeKey, String>>> = OnceLock::new();
    RECORDS.get_or_init(Mutex::default)
}

/// Output frequency only: never a delivery claim, read receipt or model acknowledgement.
pub(super) struct NativeStamp {
    key: NativeKey,
    signature: String,
}

impl NativeStamp {
    pub(super) fn new(home: &HomeLayout, group: &GroupDoc, actor: &Actor, marker: &str) -> Self {
        Self {
            key: (
                home.root().to_owned(),
                group.group_id.clone(),
                actor.id.clone(),
            ),
            signature: json!([
                cccc_core::actors::generation_identity(actor),
                actor.external_session_id,
                marker
            ])
            .to_string(),
        }
    }

    pub(super) fn is_due(&self) -> bool {
        native_records()
            .lock()
            .map(|records| records.get(&self.key) != Some(&self.signature))
            .unwrap_or(true)
    }

    pub(super) fn given(&self) {
        if let Ok(mut records) = native_records().lock() {
            records.insert(self.key.clone(), self.signature.clone());
        }
    }

    /// A lost response can consume this opportunity; later recovery markers still apply.
    pub(super) fn include_on_read(&self) -> bool {
        let Ok(mut records) = native_records().lock() else {
            // Frequency tracking is optional; a poisoned lock must not suppress instructions.
            return true;
        };
        if records.get(&self.key) == Some(&self.signature) {
            return false;
        }
        records.insert(self.key.clone(), self.signature.clone());
        true
    }
}

pub(super) fn native_marker(session: &str, now: i64, preamble: &str) -> String {
    format!("{session}:{}\n{preamble}", now / 3_600)
}

pub(super) fn receiving_context(group: &GroupDoc, actor: &Actor) -> String {
    let scope = cccc_core::group_scope::effective_scope(group, Some(actor));
    let context = json!({
        "group_id":group.group_id,
        "group_title":group.title,
        "actor_id":actor.id,
        "role":cccc_core::actors::effective_role(group, &actor.id),
        "scope_key":scope.map(|scope| &scope.scope_key),
        "scope_root":scope.map(|scope| {
            cccc_runtime::compatible_path(std::path::Path::new(&scope.url))
        }),
    });
    format!("[CCCC] Receiving context: {context}")
}

pub fn render(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> String {
    let prompt = super::actor_runtime::session_prompt(home, group, actor);
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
    use cccc_core::{GroupStore, system_prompt};

    #[test]
    fn complete_native_rules_are_isolated_by_group_and_member_generation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("first", "").expect("group");
        let other = store.create("second", "").expect("group");
        let mut actor = Actor::new("peer");
        actor.external_session_id = "claude:fixture".into();
        let marker = native_marker("revision:0", 7_200, "rules");
        let stamp = NativeStamp::new(&home, &group, &actor, &marker);
        assert!(stamp.include_on_read());
        assert!(!stamp.include_on_read());
        assert!(NativeStamp::new(&home, &other, &actor, &marker).is_due());
        actor.generation = "replacement-generation".into();
        assert!(NativeStamp::new(&home, &group, &actor, &marker).include_on_read());
        actor.external_session_id = "claude:another-source".into();
        assert!(NativeStamp::new(&home, &group, &actor, &marker).is_due());
    }

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
