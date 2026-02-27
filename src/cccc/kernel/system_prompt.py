from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List

from ..util.conv import coerce_bool
from .actors import get_effective_role, list_actors
from .group import Group
from .group_space import get_group_space_prompt_state
from .prompt_files import DEFAULT_PREAMBLE_BODY, PREAMBLE_FILENAME, read_group_prompt_file


def _memory_policy_lines(group_id: str) -> List[str]:
    """Memory system guidance for agents."""
    gid = str(group_id or "").strip()
    if not gid:
        return []
    return [
        "Memory:",
        "- Memory vs Notes: Notes are sticky notes (temporary, in Context), Memory is a notebook (persistent, in memory.db).",
        "- cccc_memory_guide: topic-specific memory best practices (store/search/consolidation/lifecycle).",
        "- cccc_memory_store: create/update memories.",
        "- cccc_memory_search: recall memories via FTS5 + filters (default depth=L0 returns summary; depth=L2 returns full content).",
        "- cccc_memory_ingest: ingest recent chat into memory (signal/raw).",
        "- cccc_memory_stats: inspect memory health metrics.",
        "- cccc_memory_decay: identify stale cleanup candidates (non-destructive).",
        "- cccc_memory_delete: delete low-value memories intentionally (single/batch).",
        "- cccc_memory_export: export read-only memory.md + manifest.",
        "- Before storing, run consolidation first (ingest signal + search related memories).",
        "- Use cccc_memory_search(track_hit=true) only for confirmed reinforcement paths that should contribute to auto-solidify.",
        "- Call cccc_memory_guide(topic) before complex memory operations for best practices.",
    ]


def _group_space_policy_lines(group_id: str) -> List[str]:
    gid = str(group_id or "").strip()
    if not gid:
        return []
    try:
        state = get_group_space_prompt_state(gid, provider="notebooklm")
        if not isinstance(state, dict):
            return []
        provider = str(state.get("provider") or "notebooklm")
        mode = str(state.get("mode") or "disabled")
        return [
            "Group Space:",
            f"- Bound memory provider: {provider} ({mode})",
            "- Use cccc_space_query for long-horizon/shared knowledge lookup.",
            "- Use cccc_space_ingest only for stable findings/resources worth reusing.",
            "- For resource_ingest payloads: use source_type + {url|content|file_id} depending on source kind.",
            "- Use cccc_space_artifact for NotebookLM outputs (save_to_space=true persists to repo/space/artifacts).",
            "- If you see files matching '*.conflict.remote.*' under space/, report and ask user for resolution; do not auto-merge/delete.",
            "- If provider is degraded/disabled, continue with Context + ledger and report fallback explicitly.",
        ]
    except Exception:
        return []


def render_system_prompt(*, group: Group, actor: Dict[str, Any]) -> str:
    """Render SYSTEM prompt for an actor.
    
    Design principles:
    - Minimal: Only session-specific context (identity, group, scopes)
    - No tool docs: Agent sees MCP tools automatically
    - Ops playbook lives in MCP: see cccc_help
    """
    group_id = str(group.group_id or "").strip()
    actor_id = str(actor.get("id") or "").strip()
    role = get_effective_role(group, actor_id)
    runner = str(actor.get("runner") or "pty").strip()

    title = str(group.doc.get("title") or group_id)
    topic = str(group.doc.get("topic") or "").strip()
    
    # Count actors
    actors = list_actors(group)
    enabled_actor_ids: List[str] = []
    for a in actors:
        if not isinstance(a, dict) or not coerce_bool(a.get("enabled"), default=True):
            continue
        aid = str(a.get("id") or "").strip()
        if aid:
            enabled_actor_ids.append(aid)
    actor_count = len(enabled_actor_ids)
    is_solo = actor_count <= 1
    
    foremen = [aid for aid in enabled_actor_ids if get_effective_role(group, aid) == "foreman"]

    # Scopes
    scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
    active_scope_key = str(group.doc.get("active_scope_key") or "")
    scope_lines: List[str] = []
    for sc in scopes:
        if not isinstance(sc, dict):
            continue
        sk = str(sc.get("scope_key") or "")
        url = str(sc.get("url") or "")
        label = str(sc.get("label") or sk)
        mark = " *" if sk and sk == active_scope_key else ""
        if url:
            scope_lines.append(f"  {label}: {url}{mark}")

    # PROJECT.md hint (don't inline file content into the prompt by default)
    project_md_line = ""
    project_root = ""
    for sc in scopes:
        if not isinstance(sc, dict):
            continue
        sk = str(sc.get("scope_key") or "")
        if sk and sk == active_scope_key:
            project_root = str(sc.get("url") or "").strip()
            break
    if not project_root:
        for sc in scopes:
            if isinstance(sc, dict):
                project_root = str(sc.get("url") or "").strip()
                if project_root:
                    break
    if project_root:
        try:
            project_root_path = Path(project_root).expanduser()
            project_md_path = project_root_path / "PROJECT.md"
            project_md_lower = project_root_path / "project.md"
            if project_md_path.exists():
                project_md_line = f"project: PROJECT.md found ({project_md_path})"
            elif project_md_lower.exists():
                project_md_line = f"project: PROJECT.md found ({project_md_lower})"
            else:
                project_md_line = f"project: PROJECT.md missing (expected at {project_md_path})"
        except Exception:
            project_md_line = "project: PROJECT.md status unknown"
    else:
        project_md_line = "project: PROJECT.md missing (no scope attached)"

    # Build minimal prompt
    lines = [
        f"[CCCC] You are {actor_id} ({role}) in group '{title}'",
        f"group_id: {group_id}",
    ]
    runtime_name = str(actor.get("runtime") or "").strip()
    if runtime_name:
        lines.append(f"runtime: {runtime_name} ({runner})")
    else:
        lines.append(f"runtime: ({runner})")
    if topic:
        lines.append(f"topic: {topic}")
    
    # Team status
    if is_solo:
        lines.append(f"team: solo (you're the only actor)")
    else:
        show_ids = enabled_actor_ids[:8]
        suffix = "..." if len(enabled_actor_ids) > 8 else ""
        lines.append(f"team: {actor_count} actors ({', '.join(show_ids)}{suffix})")
        if foremen:
            lines.append(f"foreman: {', '.join(foremen)}")
    
    # Runner mode
    if runner == "headless":
        lines.append("runner: headless (MCP-only, no PTY)")

    if project_md_line:
        lines.append(project_md_line)
    
    # Scopes
    if scope_lines:
        lines.append("")
        lines.append("scopes (* = active):")
        lines.extend(scope_lines)
    
    # Minimal platform invariants. Keep this stable and short; group-specific details belong in cccc_help / repo files.
    core_lines = [
        "Non-negotiables:",
        "- No fabrication: do not invent facts/results/sources. Investigate first; mark hypotheses.",
        "- Visible chat MUST be sent via MCP: cccc_message_send / cccc_message_reply.",
        "- Terminal output is NOT delivered as chat. If you replied in the terminal, resend via MCP.",
        "- Keep MCP surface lean: prefer cccc_capability_use for one-step use; pin stable skills via actor-scope capability_enable; uninstall unused external capabilities after work.",
    ]
    memory_lines = _memory_policy_lines(group_id)
    if memory_lines:
        core_lines.extend(["", *memory_lines])

    group_space_lines = _group_space_policy_lines(group_id)
    if group_space_lines:
        core_lines.extend(["", *group_space_lines])

    # Group override: CCCC_PREAMBLE.md under CCCC_HOME.
    pf = read_group_prompt_file(group, PREAMBLE_FILENAME)
    custom_body = str(pf.content or "").strip() if pf.found else ""

    body = custom_body if custom_body else str(DEFAULT_PREAMBLE_BODY or "").strip()

    parts = [
        "\n".join(lines).rstrip(),
        "---\n" + "\n".join(core_lines).rstrip(),
        body.rstrip(),
    ]
    return "\n\n".join([p for p in parts if p]).rstrip() + "\n"
