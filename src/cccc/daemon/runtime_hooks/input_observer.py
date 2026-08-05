from __future__ import annotations

import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from ...kernel.runtime_hooks.projection import read_launch_identity
from ...kernel.runtime_hooks.store import (
    read_state,
    record_interrupt,
    record_terminal_input,
)

_PASTE_START = b"\x1b[200~"
_PASTE_END = b"\x1b[201~"
_INPUT_LOCK = threading.Lock()
_INPUT_STATE: dict[tuple[str, str], tuple[bool, bytes]] = {}


@dataclass(frozen=True)
class HookSessionCapability:
    runtime: str
    launch_token: str
    pid: int
    runtime_state_source: str
    input_enabled: bool

    def to_dict(self) -> dict[str, Any]:
        return {
            "runtime": self.runtime,
            "launch_token": self.launch_token,
            "pid": self.pid,
            "runtime_state_source": self.runtime_state_source,
            "input_enabled": self.input_enabled,
        }


def current_session_capability(
    supervisor: Any, group_id: str, actor_id: str
) -> Any:
    reader = getattr(supervisor, "input_capability", None)
    if not callable(reader):
        return None
    try:
        return reader(group_id=group_id, actor_id=actor_id)
    except Exception:
        return None


def observe_pty_input(
    home: Path,
    capability: HookSessionCapability,
    session: Any,
    data: bytes,
) -> None:
    group_id = str(getattr(session, "group_id", "") or "")
    actor_id = str(getattr(session, "actor_id", "") or "")
    if (
        not data
        or not capability.input_enabled
        or capability.runtime_state_source != "terminal"
        or int(getattr(session, "pid", 0) or 0) != capability.pid
    ):
        return
    identity = read_launch_identity(home, group_id, actor_id)
    if (
        identity is None
        or identity.get("hook_enabled") is not True
        or identity.get("runtime") != capability.runtime
        or identity.get("launch_token") != capability.launch_token
        or int(identity.get("pid") or 0) != capability.pid
    ):
        return
    runtime = capability.runtime
    if runtime not in {"codex", "claude"}:
        return
    state = read_state(home, runtime, group_id, actor_id)
    if (
        state is None
        or state.launch_token != identity.get("launch_token")
    ):
        return
    outside = _outside_bracketed_paste(group_id, actor_id, data)
    if b"\x03" in outside or outside == b"\x1b":
        record_interrupt(home, runtime, group_id, actor_id)
    elif runtime == "claude" and outside.endswith((b"\r", b"\n")):
        record_terminal_input(home, "claude", group_id, actor_id)


def reset_pty_input(group_id: str, actor_id: str) -> None:
    with _INPUT_LOCK:
        _INPUT_STATE.pop((group_id, actor_id), None)


def _outside_bracketed_paste(
    group_id: str, actor_id: str, data: bytes
) -> bytes:
    key = (group_id, actor_id)
    with _INPUT_LOCK:
        inside, tail = _INPUT_STATE.get(key, (False, b""))
        source = tail + data
        output = bytearray()
        while source:
            marker = _PASTE_END if inside else _PASTE_START
            index = source.find(marker)
            if index >= 0:
                if not inside:
                    output.extend(source[:index])
                source = source[index + len(marker) :]
                inside = not inside
                continue
            keep = _marker_prefix_suffix(source, marker)
            if not inside:
                output.extend(
                    source[: len(source) - keep if keep else None]
                )
            tail = source[-keep:] if keep else b""
            break
        else:
            tail = b""
        _INPUT_STATE[key] = (inside, tail)
        return bytes(output)


def _marker_prefix_suffix(data: bytes, marker: bytes) -> int:
    limit = min(len(data), len(marker) - 1)
    for size in range(limit, 0, -1):
        if data.endswith(marker[:size]):
            return size
    return 0
