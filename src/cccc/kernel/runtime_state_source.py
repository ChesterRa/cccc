from __future__ import annotations

from typing import Any

from ..contracts.v1.actor import RuntimeStateSource


def default_runtime_state_source(
    *,
    runtime: str,
    runner: str,
    requested_source: str | None = None,
) -> RuntimeStateSource:
    source = str(requested_source or "").strip().lower()
    if source in {"terminal", "app_server"}:
        return source  # type: ignore[return-value]
    if str(runtime or "").strip().lower() == "codex" and str(runner or "pty").strip().lower() == "pty":
        return "app_server"
    return "terminal"


def actor_uses_codex_app_server_state(actor: dict[str, Any]) -> bool:
    """Return whether a PTY Codex actor uses app-server events as state source."""

    if not isinstance(actor, dict):
        return False
    return (
        str(actor.get("runtime") or "").strip().lower() == "codex"
        and str(actor.get("runner") or "pty").strip().lower() == "pty"
        and str(actor.get("runtime_state_source") or "terminal").strip().lower() == "app_server"
    )
