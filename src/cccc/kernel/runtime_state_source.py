from __future__ import annotations

from typing import Any


def actor_uses_codex_app_server_state(actor: dict[str, Any]) -> bool:
    """Return whether a PTY Codex actor uses app-server events as state source."""

    if not isinstance(actor, dict):
        return False
    return (
        str(actor.get("runtime") or "").strip().lower() == "codex"
        and str(actor.get("runner") or "pty").strip().lower() == "pty"
        and str(actor.get("runtime_state_source") or "terminal").strip().lower() == "app_server"
    )
