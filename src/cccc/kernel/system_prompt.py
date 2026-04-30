from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List

from ..util.conv import coerce_bool
from .actors import get_effective_role, is_pet_actor, is_voice_secretary_actor, list_visible_actors
from .group import Group
from .prompt_files import DEFAULT_PREAMBLE_BODY, PREAMBLE_FILENAME, read_group_prompt_file


def render_role_system_prompt(
    *,
    group: Group,
    actor_id: str,
    role: str,
    runtime_name: str = "",
    runner: str = "pty",
) -> str:
    """Render the shared system prompt frame for a concrete role/identity.

    This is the common prompt scaffold used by normal peer/foreman actors.
    Other role-adjacent surfaces, like the pet peer context injector, should
    reuse this frame instead of hand-rolling a parallel prompt format.
    """
    group_id = str(group.group_id or "").strip()
    actor_id = str(actor_id or "").strip()
    role = str(role or "").strip()
    runner = str(runner or "pty").strip() or "pty"

    title = str(group.doc.get("title") or group_id)
    topic = str(group.doc.get("topic") or "").strip()

    # Count actors
    actors = list_visible_actors(group)
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
    runtime_label = str(runtime_name or "").strip()
    if runtime_label:
        lines.append(f"runtime: {runtime_label} ({runner})")
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
    
    # Keep this stable and short. Long-lived playbook details belong in cccc_help.
    visible_reply_line = "- Visible replies must go through MCP: cccc_message_send / cccc_message_reply."

    core_lines = [
        "Working Style:",
        "- Work like a sharp teammate, not a customer-service script.",
        "- Prefer silence over low-signal chatter; speak for real changes, not filler or routine @all updates.",
        "- For simple exchanges, use normal sentences and keep them brief unless structure helps.",
        "- Skip empty ceremony; say the actual state, risk, or next move.",
        "",
        "Platform Invariants:",
        "- No fabrication. Verify before claiming done.",
        visible_reply_line,
        "- Before sending, verify `reply_to` and `to`; make the audience explicit when it differs.",
        "- Terminal output is not delivery.",
        "- A status message, plan, or promise is not task progress; for action requests, either start the work now or state the exact blocker.",
        "- At key transitions, sync shared control-plane state and your cccc_agent_state.",
        "- Once scope is approved, finish it end-to-end; do not ask to continue on obvious next steps.",
        "- For strategy or scope discussion, align first; implement only after explicit action intent.",
    ]

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


def render_system_prompt(*, group: Group, actor: Dict[str, Any]) -> str:
    """Render SYSTEM prompt for an actor.

    Design principles:
    - Minimal: Only session-specific context (identity, group, scopes)
    - No tool docs: Agent sees MCP tools automatically
    - Ops playbook lives in MCP: see cccc_help
    """
    actor_id = str(actor.get("id") or "").strip()
    if is_pet_actor(actor):
        from .pet_prompt import render_pet_system_prompt

        return render_pet_system_prompt(group, actor=actor)
    if is_voice_secretary_actor(actor):
        from .voice_secretary_prompt import render_voice_secretary_actor_system_prompt

        return render_voice_secretary_actor_system_prompt(group, actor=actor)
    role = get_effective_role(group, actor_id)
    runner = str(actor.get("runner") or "pty").strip()
    runtime_name = str(actor.get("runtime") or "").strip()
    return render_role_system_prompt(
        group=group,
        actor_id=actor_id,
        role=role,
        runtime_name=runtime_name,
        runner=runner,
    )
