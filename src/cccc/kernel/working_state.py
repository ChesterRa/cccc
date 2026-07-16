from __future__ import annotations

from typing import Any, Dict, Literal, Optional

from .pty_terminal_state import derive_pty_terminal_override, strip_ansi


EffectiveWorkingState = Literal["stopped", "idle", "working", "waiting", "stuck"]

DEFAULT_PTY_STUCK_IDLE_SECONDS = 300.0
DEFAULT_PTY_WORKING_IDLE_SECONDS = 5.0
DEFAULT_PTY_TERMINAL_SIGNAL_TAIL_BYTES = 12_000
DEFAULT_PTY_ACTIVITY_SIGNAL_TAIL_BYTES = 4_000


def _clean_text(value: Any) -> str:
    return str(value or "").strip()


def _safe_float(value: Any) -> Optional[float]:
    try:
        if value is None:
            return None
        return float(value)
    except Exception:
        return None


def _has_visible_terminal_output(text: str) -> bool:
    cleaned = strip_ansi(str(text or ""))
    for ch in cleaned:
        code = ord(ch)
        if code in (9, 10):
            continue
        if 32 <= code <= 126 or code > 159:
            return True
    return False


def _derive_pty_activity_state(
    *,
    idle_seconds: Optional[float],
    pty_terminal_text: str,
    updated_at: str,
    active_task_id: str,
    pty_stuck_idle_seconds: float,
) -> Optional[Dict[str, Any]]:
    idle = _safe_float(idle_seconds)
    if idle is None:
        return None
    if idle < DEFAULT_PTY_WORKING_IDLE_SECONDS:
        if _has_visible_terminal_output(pty_terminal_text):
            return {
                "effective_working_state": "working",
                "effective_working_reason": "pty_no_prompt_recent_output",
                "effective_working_updated_at": updated_at or None,
                "effective_active_task_id": active_task_id or None,
            }
        return {
            "effective_working_state": "waiting",
            "effective_working_reason": "pty_no_prompt_waiting",
            "effective_working_updated_at": updated_at or None,
            "effective_active_task_id": active_task_id or None,
        }
    if idle < float(pty_stuck_idle_seconds):
        return {
            "effective_working_state": "waiting",
            "effective_working_reason": "pty_no_prompt_waiting",
            "effective_working_updated_at": updated_at or None,
            "effective_active_task_id": active_task_id or None,
        }
    return {
        "effective_working_state": "stuck",
        "effective_working_reason": "pty_no_prompt_stuck",
        "effective_working_updated_at": updated_at or None,
        "effective_active_task_id": active_task_id or None,
    }


def derive_effective_working_state(
    *,
    running: bool,
    effective_runner: str,
    runtime: str = "",
    idle_seconds: Optional[float] = None,
    pty_terminal_text: str = "",
    pty_terminal_override: Optional[Dict[str, Any]] = None,
    agent_state: Optional[Dict[str, Any]] = None,
    headless_state: Optional[Dict[str, Any]] = None,
    pty_stuck_idle_seconds: float = DEFAULT_PTY_STUCK_IDLE_SECONDS,
) -> Dict[str, Any]:
    hot = agent_state.get("hot") if isinstance(agent_state, dict) and isinstance(agent_state.get("hot"), dict) else {}
    active_task_id = _clean_text((headless_state or {}).get("current_task_id")) or _clean_text(hot.get("active_task_id"))
    updated_at = _clean_text((headless_state or {}).get("updated_at")) or _clean_text((agent_state or {}).get("updated_at"))

    if not running:
        return {
            "effective_working_state": "stopped",
            "effective_working_reason": "runner_not_running",
            "effective_working_updated_at": updated_at or None,
            "effective_active_task_id": active_task_id or None,
        }

    if effective_runner == "headless":
        status = _clean_text((headless_state or {}).get("status")).lower() or "idle"
        if status not in {"idle", "working", "waiting", "stopped"}:
            status = "idle"
        return {
            "effective_working_state": "stopped" if status == "stopped" else status,
            "effective_working_reason": f"headless_{status}",
            "effective_working_updated_at": updated_at or None,
            "effective_active_task_id": active_task_id or None,
        }

    terminal_override = (
        dict(pty_terminal_override)
        if isinstance(pty_terminal_override, dict) and pty_terminal_override
        else derive_pty_terminal_override(runtime=runtime, terminal_text=pty_terminal_text)
    )
    if terminal_override is not None:
        return {
            **terminal_override,
            "effective_working_updated_at": updated_at or None,
            "effective_active_task_id": active_task_id or None,
        }

    pty_activity_state = _derive_pty_activity_state(
        idle_seconds=idle_seconds,
        pty_terminal_text=pty_terminal_text,
        updated_at=updated_at,
        active_task_id=active_task_id,
        pty_stuck_idle_seconds=pty_stuck_idle_seconds,
    )
    if pty_activity_state is not None:
        return pty_activity_state

    return {
        "effective_working_state": "waiting",
        "effective_working_reason": "pty_running_state_unknown",
        "effective_working_updated_at": updated_at or None,
        "effective_active_task_id": active_task_id or None,
    }
