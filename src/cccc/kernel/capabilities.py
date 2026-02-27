"""Capability surface model for MCP tool progressive disclosure.

This module defines:
1) default core tool set (always visible),
2) optional built-in capability packs (enable-on-demand),
3) helper utilities for deriving visible MCP tool names.
"""

from __future__ import annotations

from typing import Dict, Iterable, List, Set, Tuple


# Keep the default surface intentionally small.
CORE_TOOL_NAMES: Tuple[str, ...] = (
    "cccc_help",
    "cccc_bootstrap",
    "cccc_capability_search",
    "cccc_capability_enable",
    "cccc_capability_state",
    "cccc_capability_uninstall",
    "cccc_capability_use",
    "cccc_inbox_list",
    "cccc_inbox_mark_read",
    "cccc_inbox_mark_all_read",
    "cccc_message_send",
    "cccc_message_reply",
    "cccc_group_info",
    "cccc_actor_list",
    "cccc_project_info",
    "cccc_context_get",
    "cccc_context_sync",
    "cccc_task_list",
    "cccc_task_create",
    "cccc_task_update",
    "cccc_note_add",
    "cccc_note_update",
    "cccc_note_remove",
    "cccc_reference_add",
    "cccc_reference_update",
    "cccc_reference_remove",
    "cccc_presence_get",
    "cccc_presence_update",
    "cccc_presence_clear",
)


# Built-in packs that can be enabled via cccc_capability_enable.
BUILTIN_CAPABILITY_PACKS: Dict[str, Dict[str, object]] = {
    "pack:groups": {
        "title": "Group Admin",
        "description": "Group-level lifecycle controls and group list operations.",
        "tool_names": (
            "cccc_group_list",
            "cccc_group_set_state",
        ),
        "tags": ("group", "admin"),
    },
    "pack:files": {
        "title": "File Transport",
        "description": "File attachment send/resolve and IM bind support.",
        "tool_names": (
            "cccc_file_send",
            "cccc_blob_path",
            "cccc_im_bind",
        ),
        "tags": ("file", "attachment", "im"),
    },
    "pack:actor-admin": {
        "title": "Actor Admin",
        "description": "Actor profile/runtime lifecycle controls.",
        "tool_names": (
            "cccc_actor_profile_list",
            "cccc_actor_add",
            "cccc_actor_remove",
            "cccc_actor_start",
            "cccc_actor_stop",
            "cccc_actor_restart",
            "cccc_runtime_list",
        ),
        "tags": ("actor", "runtime", "admin"),
    },
    "pack:space": {
        "title": "Group Space",
        "description": "NotebookLM-backed Group Space operations.",
        "tool_names": (
            "cccc_space_status",
            "cccc_space_capabilities",
            "cccc_space_bind",
            "cccc_space_ingest",
            "cccc_space_query",
            "cccc_space_sources",
            "cccc_space_artifact",
            "cccc_space_jobs",
            "cccc_space_sync",
            "cccc_space_provider_auth",
            "cccc_space_provider_credential_status",
            "cccc_space_provider_credential_update",
        ),
        "tags": ("space", "notebooklm", "knowledge"),
    },
    "pack:automation": {
        "title": "Automation",
        "description": "Automation reminder inspection and mutation.",
        "tool_names": (
            "cccc_automation_state",
            "cccc_automation_manage",
        ),
        "tags": ("automation", "ops"),
    },
    "pack:context-advanced": {
        "title": "Context Advanced",
        "description": "Milestone/vision/sketch and memory advanced operations.",
        "tool_names": (
            "cccc_vision_update",
            "cccc_sketch_update",
            "cccc_milestone_create",
            "cccc_milestone_update",
            "cccc_milestone_complete",
            "cccc_memory_guide",
            "cccc_memory_store",
            "cccc_memory_search",
            "cccc_memory_stats",
            "cccc_memory_ingest",
            "cccc_memory_decay",
            "cccc_memory_export",
            "cccc_memory_delete",
        ),
        "tags": ("context", "memory", "advanced"),
    },
    "pack:headless": {
        "title": "Headless Runtime",
        "description": "Headless runner control tools.",
        "tool_names": (
            "cccc_headless_status",
            "cccc_headless_set_status",
            "cccc_headless_ack_message",
        ),
        "tags": ("headless", "runner"),
    },
    "pack:notify": {
        "title": "System Notify",
        "description": "System notification send/ack tools.",
        "tool_names": (
            "cccc_notify_send",
            "cccc_notify_ack",
        ),
        "tags": ("notify", "system"),
    },
    "pack:terminal-debug": {
        "title": "Terminal Debug",
        "description": "Terminal transcript and local debug log diagnostics.",
        "tool_names": (
            "cccc_terminal_tail",
            "cccc_debug_snapshot",
            "cccc_debug_tail_logs",
        ),
        "tags": ("terminal", "debug", "diagnostics"),
    },
}


def all_builtin_pack_ids() -> List[str]:
    return sorted(BUILTIN_CAPABILITY_PACKS.keys())


def core_tool_name_set() -> Set[str]:
    return set(CORE_TOOL_NAMES)


def all_pack_tool_name_set() -> Set[str]:
    names: Set[str] = set()
    for pack in BUILTIN_CAPABILITY_PACKS.values():
        for tool_name in pack.get("tool_names", ()):
            names.add(str(tool_name))
    return names


def resolve_visible_tool_names(enabled_capability_ids: Iterable[str]) -> Set[str]:
    visible: Set[str] = set(CORE_TOOL_NAMES)
    for cap_id in enabled_capability_ids:
        cap = BUILTIN_CAPABILITY_PACKS.get(str(cap_id))
        if not isinstance(cap, dict):
            continue
        for tool_name in cap.get("tool_names", ()):
            visible.add(str(tool_name))
    return visible
