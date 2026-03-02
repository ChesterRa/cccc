"""Blueprint generation ops (skeleton — LLM integration TODO)."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse


def _error(
    code: str, message: str, *, details: Optional[Dict[str, Any]] = None
) -> DaemonResponse:
    return DaemonResponse(
        ok=False,
        error=DaemonError(code=code, message=message, details=(details or {})),
    )


# Predefined blueprint IDs (must match web/src/data/blueprints.ts)
_PREDEFINED_IDS = ("shield", "house", "rocket")


def _hash_fnv1a(s: str) -> int:
    """FNV-1a hash — mirrors web/src/utils/blueprintMatcher.ts."""
    h = 0x811C9DC5
    for ch in s:
        h ^= ord(ch)
        h = (h * 0x01000193) & 0xFFFFFFFF
    return h


def handle_blueprint_generate(args: Dict[str, Any]) -> DaemonResponse:
    """Generate a blueprint for a task.

    Currently returns a predefined blueprint ID (no LLM).
    TODO: Integrate LLM (Haiku/Flash) to generate custom block layouts.
    """
    task_id = str(args.get("task_id") or "").strip()
    if not task_id:
        return _error("missing_task_id", "task_id is required")

    # task_name = str(args.get("task_name") or "")   # noqa: ERA001
    # task_goal = str(args.get("task_goal") or "")    # noqa: ERA001
    # theme_hint = str(args.get("theme_hint") or "")  # noqa: ERA001

    # --- LLM generation path (TODO) ---
    # 1. Build prompt with task_name, task_goal, theme_hint
    # 2. Call LLM (Haiku/Flash, ~$0.0003/call)
    #    System prompt: output JSON array [{x,y,z,color}], max 50 blocks
    #    gridSize limit: 8x8x8
    # 3. Validate JSON schema + bounds
    # 4. Cache result in task metadata
    # 5. Return full Blueprint object
    # ---

    # Fallback: deterministic predefined selection
    h = _hash_fnv1a(task_id)
    blueprint_id = _PREDEFINED_IDS[h % len(_PREDEFINED_IDS)]

    return DaemonResponse(
        ok=True,
        result={
            "source": "predefined",
            "blueprint_id": blueprint_id,
            "variant": h % 3,
        },
    )


def try_handle_blueprint_op(
    op: str, args: Dict[str, Any]
) -> Optional[DaemonResponse]:
    """Handle blueprint-related ops. Returns None if op not matched."""
    if op == "blueprint_generate":
        return handle_blueprint_generate(args)
    return None
