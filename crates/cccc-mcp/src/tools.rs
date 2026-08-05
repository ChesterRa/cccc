use serde_json::{Value, json};

use crate::schemas;

const TOOLS: &[(&str, &str)] = &[
    (
        "cccc_help",
        "Load the effective CCCC collaboration playbook.",
    ),
    (
        "cccc_bootstrap",
        "Bootstrap the current actor session, inbox, and context.",
    ),
    (
        "cccc_project_info",
        "Read project information for the active scope.",
    ),
    ("cccc_inbox_list", "List unread collaboration messages."),
    (
        "cccc_inbox_mark_read",
        "Mark one or all inbox messages read.",
    ),
    (
        "cccc_message_send",
        "Send a visible collaboration message or reply. For a trusted remote Group Bridge route, set dst_group_id to its exact remote_group_id; omitted to defaults to the target group's unique available foreman.",
    ),
    (
        "cccc_message_reply",
        "Reply to a visible collaboration message by event ID.",
    ),
    (
        "cccc_tracked_send",
        "Create a durable delegation and linked message.",
    ),
    ("cccc_file", "Read or send a CCCC blob attachment."),
    ("cccc_repo", "Inspect files under the active project scope."),
    (
        "cccc_presentation",
        "Publish, read, or clear presentation cards.",
    ),
    (
        "cccc_context_get",
        "Read structured group coordination context.",
    ),
    (
        "cccc_coordination",
        "Update the coordination brief or add a note.",
    ),
    (
        "cccc_task",
        "List, create, update, move, restore, or archive tasks.",
    ),
    (
        "cccc_agent_state",
        "Read or update actor-owned working state. Returns post-write agent_state and context_hygiene confirmation.",
    ),
    (
        "cccc_memory",
        "Search, read, or write durable group memory.",
    ),
    (
        "cccc_capability_search",
        "Search available capability packs.",
    ),
    (
        "cccc_capability_state",
        "Inspect enabled and blocked capabilities.",
    ),
    ("cccc_capability_enable", "Enable a capability pack."),
    (
        "cccc_capability_install",
        "Inspect the capability installation target.",
    ),
    (
        "cccc_capability_use",
        "Load and invoke a capability capsule.",
    ),
    ("cccc_group", "Manage group state and attached scopes."),
    ("cccc_actor", "Manage actors and runtime lifecycle."),
    ("cccc_runtime_list", "List supported agent runtimes."),
    (
        "cccc_actor_notes",
        "Read or update role notes for an actor.",
    ),
    ("cccc_im_bind", "Manage IM chat authorization."),
    (
        "cccc_remote_access",
        "Inspect or configure Group Bridge access.",
    ),
    (
        "cccc_remote_context",
        "Read context from a trusted remote group.",
    ),
    ("cccc_remote_repo", "Inspect a trusted remote repository."),
    ("cccc_remote_git", "Run read-only git operations remotely."),
    ("cccc_remote_repo_edit", "Edit a trusted remote repository."),
    (
        "cccc_remote_apply_patch",
        "Apply a patch to a trusted remote repository.",
    ),
    (
        "cccc_remote_shell",
        "Run a bounded trusted remote shell command.",
    ),
    (
        "cccc_remote_exec_command",
        "Start a remote command session.",
    ),
    (
        "cccc_remote_write_stdin",
        "Write to a remote command session.",
    ),
    (
        "cccc_space",
        "Manage provider-backed Group Space. Query may return status=pending|queued; wait for the later system.notify before reading results.",
    ),
    ("cccc_automation", "Inspect and update automation rules."),
    (
        "cccc_context_sync",
        "Apply a low-level Context Ops v3 batch.",
    ),
    ("cccc_memory_admin", "Run memory maintenance operations."),
    ("cccc_headless", "Inspect or update headless actor state."),
    ("cccc_notify", "Send or acknowledge system notifications."),
    ("cccc_terminal", "Inspect or control an actor terminal."),
    ("cccc_debug", "Read bounded daemon diagnostics."),
    (
        "cccc_capability_import",
        "Import a custom capability capsule.",
    ),
    ("cccc_capability_block", "Block or unblock a capability."),
    ("cccc_capability_uninstall", "Remove a custom capability."),
    (
        "cccc_runtime_wait_next_turn",
        "Wait for a structured headless actor turn.",
    ),
    (
        "cccc_runtime_complete_turn",
        "Complete a structured headless actor turn.",
    ),
    (
        "cccc_code_exec",
        "Execute code in a managed session. Discovery helpers: COMMON_WORK_LOOPS, tool_help(query), tool_names(query), list_tools(query), detail:'schema'; max_output_tokens up to 50000.",
    ),
    ("cccc_code_wait", "Wait for a managed code session."),
    ("cccc_repo_edit", "Edit files under the active scope."),
    (
        "cccc_apply_patch",
        "Apply a Codex-style patch under the active scope.",
    ),
    (
        "cccc_shell",
        "Run a bounded shell command under the active scope.",
    ),
    ("cccc_exec_command", "Start a bounded command session."),
    ("cccc_write_stdin", "Write to a bounded command session."),
    (
        "cccc_git",
        "Run allowlisted git operations under the active scope.",
    ),
    (
        "cccc_voice_secretary_document",
        "Manage Voice Secretary documents.",
    ),
    (
        "cccc_voice_secretary_composer",
        "Manage Voice Secretary prompt drafts.",
    ),
    (
        "cccc_voice_secretary_request",
        "Submit a Voice Secretary request.",
    ),
];

pub fn catalog() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|(name, description)| {
            let mut tool = json!({
                "name": name,
                "description": description,
                "inputSchema": schemas::input(name),
            });
            if let Some(annotations) = schemas::annotations(name) {
                tool.as_object_mut()
                    .map(|object| object.insert("annotations".into(), annotations));
            }
            tool
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_exposes_python_message_tools() {
        let names: Vec<_> = super::catalog()
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect();
        assert!(names.iter().any(|name| name == "cccc_message_send"));
        assert!(names.iter().any(|name| name == "cccc_message_reply"));
    }
}
