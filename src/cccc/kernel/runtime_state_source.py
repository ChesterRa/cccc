from __future__ import annotations

import ntpath
from pathlib import Path
from typing import Any

from ..contracts.v1.actor import RuntimeStateSource


_CODEX_PROVIDER_CONFIG_KEYS = (
    "openai_base_url",
    "model_provider",
    "model_providers",
)


def _command_stem(command: str) -> str:
    raw = str(command or "").strip()
    if not raw:
        return ""
    try:
        return str(Path(ntpath.basename(raw)).stem or "").strip().lower()
    except Exception:
        return raw.lower()


def _codex_config_key(value: str) -> str:
    return str(value or "").split("=", 1)[0].strip()


def codex_pty_command_prefers_terminal_state(command: list[str] | None) -> bool:
    """Return whether a Codex PTY command should use terminal-derived state.

    App-server state is preferable only when the app-server process faithfully
    represents the actor command. Profile/provider/local-model flags currently
    bind behavior to the terminal command, so terminal state is the safer truth.
    """

    items = [str(item or "").strip() for item in list(command or []) if str(item or "").strip()]
    if not items:
        return False
    stem = _command_stem(items[0])
    if stem != "codex":
        return False

    i = 1
    while i < len(items):
        arg = items[i]
        if arg == "--":
            break
        if arg in {"-p", "--profile", "--oss", "--local-provider"}:
            return True
        if arg.startswith("--profile=") or arg.startswith("--local-provider="):
            return True
        if arg in {"-c", "--config"}:
            if i + 1 < len(items):
                key = _codex_config_key(items[i + 1])
                if any(key == wanted or key.startswith(f"{wanted}.") for wanted in _CODEX_PROVIDER_CONFIG_KEYS):
                    return True
            i += 2
            continue
        if arg.startswith("--config="):
            key = _codex_config_key(arg.split("=", 1)[1])
            if any(key == wanted or key.startswith(f"{wanted}.") for wanted in _CODEX_PROVIDER_CONFIG_KEYS):
                return True
        i += 1

    return False


def default_runtime_state_source(
    *,
    runtime: str,
    runner: str,
    requested_source: str | None = None,
    command: list[str] | None = None,
) -> RuntimeStateSource:
    source = str(requested_source or "").strip().lower()
    if source in {"terminal", "app_server"}:
        return source  # type: ignore[return-value]
    if str(runtime or "").strip().lower() == "codex" and str(runner or "pty").strip().lower() == "pty":
        if codex_pty_command_prefers_terminal_state(command):
            return "terminal"
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
