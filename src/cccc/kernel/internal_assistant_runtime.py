from __future__ import annotations

from typing import List, Tuple

from .runtime import (
    PRIMARY_RUNTIMES,
    detect_runtime,
    get_runtime_command_with_flags,
    runtime_start_preflight_error,
)


def normalize_internal_assistant_launch_seed(
    *,
    runtime: str,
    runner: str,
    command: List[str],
) -> Tuple[str, str, List[str]]:
    """Normalize built-in assistant runtime fields away from web_model."""

    runtime_value = str(runtime or "").strip()
    runner_value = str(runner or "").strip()
    normalized_runner = runner_value if runner_value else "pty"
    normalized_runtime = runtime_value if runtime_value else "codex"
    normalized_command = list(command or [])

    if normalized_runtime.lower() == "web_model":
        normalized_runtime = "codex"
        normalized_runner = "headless"
        normalized_command = get_runtime_command_with_flags(normalized_runtime)

    if runtime_start_preflight_error(normalized_runtime, list(normalized_command), runner=normalized_runner):
        for candidate in PRIMARY_RUNTIMES:
            if candidate == "web_model":
                continue
            if not detect_runtime(candidate).available:
                continue
            normalized_runtime = candidate
            normalized_command = get_runtime_command_with_flags(candidate)
            break

    return normalized_runtime, normalized_runner, normalized_command
