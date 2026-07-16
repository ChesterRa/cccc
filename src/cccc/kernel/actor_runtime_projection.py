from __future__ import annotations

from typing import Any, Dict

from ..util.conv import coerce_bool
from .working_state import derive_effective_working_state


def actor_runtime_enabled(actor: dict[str, Any]) -> bool:
    return coerce_bool((actor or {}).get("enabled"), default=True)


def disabled_actor_runtime_projection(*, effective_runner: str, runtime: str) -> Dict[str, Any]:
    return {
        "running": False,
        "idle_seconds": None,
        "runner_effective": effective_runner,
        **derive_effective_working_state(
            running=False,
            effective_runner=effective_runner,
            runtime=runtime,
            idle_seconds=None,
            pty_terminal_text="",
            headless_state=None,
        ),
    }
