"""Capability surface model for MCP tool progressive disclosure.

This module defines:
1) default core tool set (always visible),
2) optional built-in capability packs (enable-on-demand),
3) helper utilities for deriving visible MCP tool names.
"""

from __future__ import annotations

from typing import Dict, Iterable, List, Set, Tuple


CORE_BASIC_TOOLS: Tuple[str, ...] = (
    "cccc_help",
    "cccc_bootstrap",
    "cccc_project_info",
    "cccc_capability_search",
    "cccc_capability_state",
    "cccc_inbox_list",
    "cccc_inbox_mark_read",
    "cccc_message_send",
    "cccc_message_reply",
    "cccc_context_get",
    "cccc_coordination",
    "cccc_task",
    "cccc_agent_state",
    "cccc_memory",
)

CORE_ADMIN_TOOLS: Tuple[str, ...] = (
    "cccc_capability_enable",
    "cccc_capability_block",
    "cccc_capability_import",
    "cccc_capability_uninstall",
    "cccc_capability_use",
)

CORE_TOOL_NAMES: Tuple[str, ...] = CORE_BASIC_TOOLS + CORE_ADMIN_TOOLS


BUILTIN_CAPABILITY_PACKS: Dict[str, Dict[str, object]] = {
    "pack:group-runtime": {
        "title": "Group + Runtime Operations",
        "description": "Group state operations and actor/runtime lifecycle controls.",
        "tool_names": (
            "cccc_group",
            "cccc_actor",
            "cccc_runtime_list",
        ),
        "tags": ("group", "actor", "runtime"),
    },
    "pack:file-im": {
        "title": "File + IM",
        "description": "File attachment operations and IM bind support.",
        "tool_names": (
            "cccc_file",
            "cccc_im_bind",
        ),
        "tags": ("file", "attachment", "im"),
    },
    "pack:space": {
        "title": "Group Space",
        "description": "NotebookLM-backed Group Space operations (consolidated action tool).",
        "tool_names": (
            "cccc_space",
        ),
        "tags": ("space", "notebooklm", "knowledge"),
    },
    "pack:automation": {
        "title": "Automation",
        "description": "Automation reminder inspection and mutation (state/manage actions).",
        "tool_names": (
            "cccc_automation",
        ),
        "tags": ("automation", "ops"),
    },
    "pack:context-advanced": {
        "title": "Context Advanced",
        "description": "Low-level context batch sync and memory admin operations.",
        "tool_names": (
            "cccc_context_sync",
            "cccc_memory_admin",
        ),
        "tags": ("context", "memory", "admin"),
    },
    "pack:headless-notify": {
        "title": "Headless + Notify",
        "description": "Headless runner control and system notifications.",
        "tool_names": (
            "cccc_headless",
            "cccc_notify",
        ),
        "tags": ("headless", "notify", "runner"),
    },
    "pack:diagnostics": {
        "title": "Terminal Debug",
        "description": "Terminal transcript and local debug diagnostics.",
        "tool_names": (
            "cccc_terminal",
            "cccc_debug",
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
        for tool_name in pack.get("tool_names", ()):  # type: ignore[arg-type]
            names.add(str(tool_name))
    return names


def resolve_visible_tool_names(
    enabled_capability_ids: Iterable[str],
    *,
    actor_role: str = "",
) -> Set[str]:
    role = str(actor_role or "").strip().lower()
    if role == "peer":
        visible: Set[str] = set(CORE_BASIC_TOOLS)
    else:
        visible = set(CORE_TOOL_NAMES)
    for cap_id in enabled_capability_ids:
        cap = BUILTIN_CAPABILITY_PACKS.get(str(cap_id))
        if not isinstance(cap, dict):
            continue
        for tool_name in cap.get("tool_names", ()):  # type: ignore[arg-type]
            visible.add(str(tool_name))
    return visible
