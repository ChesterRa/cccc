use crate::capabilities::Capability;

pub const SELF_EVOLUTION_CAPABILITY_ID: &str = "skill:cccc:self-evolution";
pub const LEGACY_SELF_EVOLUTION_CAPABILITY_ID: &str =
    "skill:agent_self_proposed:cccc-self-evolution";
pub const DEFAULT_GROUP_CAPABILITY_SEED_VERSION: u64 = 2;

const SELF_EVOLUTION_CAPSULE: &str = include_str!("../../../resources/cccc-self-evolution.md");

pub const WEB_MODEL_CORE_TOOL_NAMES: &[&str] = &[
    "cccc_help",
    "cccc_bootstrap",
    "cccc_capability_search",
    "cccc_capability_use",
    "cccc_inbox_read",
    "cccc_message_history",
    "cccc_message_send",
    "cccc_message_reply",
    "cccc_message_deliver",
    "cccc_reply_request_cancel",
    "cccc_file",
    "cccc_context_get",
    "cccc_coordination",
    "cccc_task",
    "cccc_agent_state",
    "cccc_project_info",
    "cccc_capability_state",
    "cccc_capability_enable",
    "cccc_capability_install",
    "cccc_tracked_send",
    "cccc_repo",
    "cccc_presentation",
    "cccc_memory",
    "cccc_runtime_wait_next_turn",
    "cccc_runtime_complete_turn",
    "cccc_code_exec",
    "cccc_code_wait",
    "cccc_repo_edit",
    "cccc_apply_patch",
    "cccc_shell",
    "cccc_exec_command",
    "cccc_write_stdin",
    "cccc_git",
];

pub fn is_builtin_capability_pack_tool(name: &str) -> bool {
    all()
        .iter()
        .any(|capability| capability.tool_names.iter().any(|tool| tool == name))
}

pub fn all() -> Vec<Capability> {
    vec![
        skill(
            SELF_EVOLUTION_CAPABILITY_ID,
            "cccc-self-evolution",
            "Review complete visible CCCC group history and propose confirmed improvements at the prompt, structured-context, workflow, Harness, or optimizer level.",
            SELF_EVOLUTION_CAPSULE,
            &[
                "self-evolution",
                "learning",
                "workflow",
                "harness",
                "optimizer",
                "cccc-glue",
            ],
        ),
        pack(
            "pack:group-runtime",
            "Group + Runtime Operations",
            &[
                "cccc_group",
                "cccc_actor",
                "cccc_runtime_list",
                "cccc_actor_notes",
            ],
            &["group", "actor", "runtime"],
        ),
        pack(
            "pack:file-im",
            "IM Bind",
            &["cccc_im_bind"],
            &["im", "bind"],
        ),
        pack(
            "pack:group_bridge",
            "Group Bridge Remote Access",
            &[
                "cccc_remote_access",
                "cccc_remote_context",
                "cccc_remote_repo",
                "cccc_remote_git",
                "cccc_remote_repo_edit",
                "cccc_remote_apply_patch",
                "cccc_remote_shell",
                "cccc_remote_exec_command",
                "cccc_remote_write_stdin",
            ],
            &["group-bridge", "remote"],
        ),
        pack(
            "pack:space",
            "Group Space",
            &["cccc_space"],
            &["space", "notebooklm", "knowledge"],
        ),
        pack(
            "pack:automation",
            "Automation",
            &["cccc_automation"],
            &["automation", "ops"],
        ),
        pack(
            "pack:context-advanced",
            "Extended Context + Delegation",
            &[
                "cccc_project_info",
                "cccc_tracked_send",
                "cccc_context_sync",
                "cccc_memory",
                "cccc_memory_admin",
            ],
            &["context", "delegation", "memory"],
        ),
        pack(
            "pack:headless-notify",
            "Headless + Notify",
            &["cccc_headless", "cccc_notify"],
            &["headless", "notify"],
        ),
        pack(
            "pack:diagnostics",
            "Workspace Utilities",
            &[
                "cccc_repo",
                "cccc_presentation",
                "cccc_terminal",
                "cccc_debug",
            ],
            &["workspace", "repo", "presentation", "diagnostics"],
        ),
        pack(
            "pack:capability-admin",
            "Capability Admin",
            &[
                "cccc_capability_state",
                "cccc_capability_enable",
                "cccc_capability_install",
                "cccc_capability_import",
                "cccc_capability_block",
                "cccc_capability_uninstall",
            ],
            &["capability", "install", "admin", "governance"],
        ),
    ]
}

fn skill(id: &str, name: &str, description: &str, capsule_text: &str, tags: &[&str]) -> Capability {
    Capability {
        id: id.into(),
        kind: "skill".into(),
        name: name.into(),
        description: description.into(),
        tool_names: Vec::new(),
        tags: tags.iter().map(|value| (*value).into()).collect(),
        capsule_text: capsule_text.trim().into(),
        source: "cccc_builtin".into(),
        source_uri: String::new(),
        qualification_status: "qualified".into(),
        enable_supported: true,
    }
}

fn pack(id: &str, title: &str, tools: &[&str], tags: &[&str]) -> Capability {
    Capability {
        id: id.into(),
        kind: "pack".into(),
        name: title.into(),
        description: format!("Built-in CCCC capability pack: {title}."),
        tool_names: tools.iter().map(|value| (*value).into()).collect(),
        tags: tags.iter().map(|value| (*value).into()).collect(),
        capsule_text: String::new(),
        source: "cccc_builtin".into(),
        source_uri: String::new(),
        qualification_status: "qualified".into(),
        enable_supported: true,
    }
}
