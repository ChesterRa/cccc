from __future__ import annotations

import os
import subprocess
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, Optional

from ...daemon.server import DaemonPaths, call_daemon
from ...paths import cccc_home
from ...util.fs import read_json


class MCPError(Exception):
    """MCP tool call error"""

    def __init__(
        self, code: str, message: str, details: Optional[Dict[str, Any]] = None
    ):
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def _env_str(name: str) -> str:
    value = os.environ.get(name)
    return str(value).strip() if value is not None else ""


@dataclass(frozen=True)
class _RuntimeContext:
    home: str
    group_id: str
    actor_id: str


_RUNTIME_CONTEXT_OVERRIDE: ContextVar[Optional[_RuntimeContext]] = ContextVar(
    "cccc_mcp_runtime_context_override",
    default=None,
)


@contextmanager
def runtime_context_override(*, home: str = "", group_id: str = "", actor_id: str = "") -> Iterable[None]:
    ctx = _RuntimeContext(
        home=_normalize_home(str(home or "")),
        group_id=str(group_id or "").strip(),
        actor_id=str(actor_id or "").strip(),
    )
    token = _RUNTIME_CONTEXT_OVERRIDE.set(ctx)
    try:
        yield
    finally:
        _RUNTIME_CONTEXT_OVERRIDE.reset(token)


def _proc_parent_pid_windows(pid: int) -> int:
    if pid <= 0 or os.name != "nt":
        return 0
    try:
        import ctypes
        from ctypes import wintypes

        TH32CS_SNAPPROCESS = 0x00000002
        INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

        class PROCESSENTRY32W(ctypes.Structure):
            _fields_ = [
                ("dwSize", wintypes.DWORD),
                ("cntUsage", wintypes.DWORD),
                ("th32ProcessID", wintypes.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t),
                ("th32ModuleID", wintypes.DWORD),
                ("cntThreads", wintypes.DWORD),
                ("th32ParentProcessID", wintypes.DWORD),
                ("pcPriClassBase", ctypes.c_long),
                ("dwFlags", wintypes.DWORD),
                ("szExeFile", wintypes.WCHAR * 260),
            ]

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
        kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        kernel32.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
        kernel32.Process32FirstW.restype = wintypes.BOOL
        kernel32.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
        kernel32.Process32NextW.restype = wintypes.BOOL
        kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        kernel32.CloseHandle.restype = wintypes.BOOL

        snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
        if snapshot in (0, INVALID_HANDLE_VALUE, None):
            return 0
        try:
            entry = PROCESSENTRY32W()
            entry.dwSize = ctypes.sizeof(PROCESSENTRY32W)
            if not kernel32.Process32FirstW(snapshot, ctypes.byref(entry)):
                return 0
            while True:
                if int(entry.th32ProcessID or 0) == int(pid):
                    parent = int(entry.th32ParentProcessID or 0)
                    return 0 if parent == int(pid) else parent
                if not kernel32.Process32NextW(snapshot, ctypes.byref(entry)):
                    break
        finally:
            kernel32.CloseHandle(snapshot)
    except Exception:
        return 0
    return 0


def _proc_parent_pid(pid: int) -> int:
    if pid <= 0:
        return 0
    if os.name == "nt":
        return _proc_parent_pid_windows(pid)
    if os.name != "posix":
        return 0
    try:
        status_path = Path("/proc") / str(pid) / "status"
        for line in status_path.read_text(encoding="utf-8", errors="ignore").splitlines():
            if line.startswith("PPid:"):
                return int(str(line.split(":", 1)[1] or "").strip() or "0")
    except Exception:
        pass
    try:
        result = subprocess.run(
            ["ps", "-o", "ppid=", "-p", str(pid)],
            capture_output=True,
            text=True,
            timeout=1,
        )
        if result.returncode != 0:
            return 0
        return int(str(result.stdout or "").strip() or "0")
    except Exception:
        return 0
    return 0


def _iter_ancestor_pids(start_pid: Optional[int] = None) -> list[int]:
    pid = int(start_pid or os.getpid() or 0)
    if pid <= 0:
        return []
    out: list[int] = []
    seen: set[int] = set()
    while pid > 0 and pid not in seen:
        out.append(pid)
        seen.add(pid)
        parent = _proc_parent_pid(pid)
        if parent <= 0 or parent == pid:
            break
        pid = parent
    return out


def _proc_environ(pid: int) -> Dict[str, str]:
    if pid <= 0 or os.name != "posix":
        return {}
    try:
        raw = (Path("/proc") / str(pid) / "environ").read_bytes()
    except Exception:
        return {}
    out: Dict[str, str] = {}
    for item in raw.split(b"\x00"):
        if not item or b"=" not in item:
            continue
        key_raw, value_raw = item.split(b"=", 1)
        try:
            key = key_raw.decode("utf-8", "ignore").strip()
        except Exception:
            key = ""
        if not key:
            continue
        try:
            value = value_raw.decode("utf-8", "ignore").strip()
        except Exception:
            value = ""
        out[key] = value
    return out


def _first_ancestor_env_value(ancestor_pids: Iterable[int], name: str) -> str:
    wanted = str(name or "").strip()
    if not wanted:
        return ""
    for pid in ancestor_pids:
        value = str(_proc_environ(int(pid)).get(wanted) or "").strip()
        if value:
            return value
    return ""


def _normalize_home(value: str) -> str:
    raw = str(value or "").strip()
    if not raw:
        return ""
    try:
        return str(Path(raw).expanduser().resolve())
    except Exception:
        return raw


def _find_runtime_binding_from_pty_state(home: str, ancestor_pids: Iterable[int]) -> Optional[tuple[str, str]]:
    home_path_raw = _normalize_home(home)
    if not home_path_raw:
        return None
    home_path = Path(home_path_raw)
    base = home_path / "groups"
    if not base.exists():
        return None
    ancestor_rank = {int(pid): idx for idx, pid in enumerate(ancestor_pids) if int(pid) > 0}
    best_rank: Optional[int] = None
    best: Optional[tuple[str, str]] = None
    try:
        candidates = base.glob("*/state/runners/pty/*.json")
    except Exception:
        return None
    for path in candidates:
        doc = read_json(path)
        if not isinstance(doc, dict):
            continue
        try:
            pid = int(doc.get("pid") or 0)
        except Exception:
            pid = 0
        rank = ancestor_rank.get(pid)
        if rank is None:
            continue
        group_id = str(doc.get("group_id") or "").strip()
        actor_id = str(doc.get("actor_id") or "").strip()
        if not group_id or not actor_id:
            continue
        if best_rank is None or rank < best_rank:
            best_rank = rank
            best = (group_id, actor_id)
    return best


def _runtime_context() -> _RuntimeContext:
    override = _RUNTIME_CONTEXT_OVERRIDE.get()
    if override is not None and (override.group_id or override.actor_id or override.home):
        default_home = _normalize_home(str(cccc_home()))
        return _RuntimeContext(
            home=override.home or default_home,
            group_id=override.group_id,
            actor_id=override.actor_id,
        )

    home = _normalize_home(_env_str("CCCC_HOME"))
    gid = _env_str("CCCC_GROUP_ID")
    aid = _env_str("CCCC_ACTOR_ID")

    ancestor_pids = _iter_ancestor_pids()
    if not home:
        home = _normalize_home(_first_ancestor_env_value(ancestor_pids, "CCCC_HOME"))
    if not gid:
        gid = _first_ancestor_env_value(ancestor_pids, "CCCC_GROUP_ID")
    if not aid:
        aid = _first_ancestor_env_value(ancestor_pids, "CCCC_ACTOR_ID")

    default_home = _normalize_home(str(cccc_home()))
    candidate_homes: list[str] = []
    for item in (home, default_home):
        if item and item not in candidate_homes:
            candidate_homes.append(item)

    if not gid or not aid:
        for candidate in candidate_homes:
            recovered = _find_runtime_binding_from_pty_state(candidate, ancestor_pids)
            if not recovered:
                continue
            gid = gid or recovered[0]
            aid = aid or recovered[1]
            home = home or candidate
            if gid and aid:
                break

    return _RuntimeContext(home=home or default_home, group_id=gid, actor_id=aid)


def _validate_self_actor_id(actor_id: str) -> str:
    aid = str(actor_id or "").strip()
    if not aid:
        raise MCPError(code="missing_actor_id", message="missing actor_id")
    if aid == "user":
        raise MCPError(
            code="invalid_actor_id",
            message="actor_id 'user' is reserved; agents must not act as user",
        )
    return aid


def _resolve_group_id(arguments: Dict[str, Any]) -> str:
    """Resolve group_id from runtime context or tool arguments (runtime context wins)."""
    env_gid = _runtime_context().group_id
    arg_gid = str(arguments.get("group_id") or "").strip()
    gid = env_gid or arg_gid
    if not gid:
        raise MCPError(
            code="missing_group_id",
            message="missing group_id (set CCCC_GROUP_ID env or pass group_id)",
        )
    if env_gid and arg_gid and arg_gid != env_gid:
        raise MCPError(
            code="group_id_mismatch",
            message="group_id mismatch (tool args must match CCCC_GROUP_ID)",
            details={"env": env_gid, "arg": arg_gid},
        )
    return gid


def _resolve_self_actor_id(arguments: Dict[str, Any]) -> str:
    """Resolve the caller actor_id from runtime context or tool arguments (runtime context wins)."""
    env_aid = _runtime_context().actor_id
    arg_aid = str(arguments.get("actor_id") or "").strip()
    aid = env_aid or arg_aid
    if not aid:
        raise MCPError(
            code="missing_actor_id",
            message="missing actor_id (set CCCC_ACTOR_ID env or pass actor_id)",
        )
    if env_aid and arg_aid and arg_aid != env_aid:
        raise MCPError(
            code="actor_id_mismatch",
            message="actor_id mismatch (tool args must match CCCC_ACTOR_ID)",
            details={"env": env_aid, "arg": arg_aid},
        )
    return _validate_self_actor_id(aid)


def _resolve_caller_from_by(arguments: Dict[str, Any]) -> str:
    """Resolve caller identity from ``by`` arg or runtime actor identity only.

    Use for tools where ``actor_id`` refers to a target actor, not the caller.
    """
    env_aid = _runtime_context().actor_id
    arg_by = str(arguments.get("by") or "").strip()
    aid = env_aid or arg_by
    if not aid:
        raise MCPError(
            code="missing_actor_id",
            message="missing actor id (set CCCC_ACTOR_ID env or pass by)",
        )
    if env_aid and arg_by and arg_by != env_aid:
        raise MCPError(
            code="actor_id_mismatch",
            message="by mismatch (tool args must match CCCC_ACTOR_ID)",
            details={"env": env_aid, "arg": arg_by},
        )
    return _validate_self_actor_id(aid)


def _resolve_caller_actor_id(arguments: Dict[str, Any]) -> str:
    """Resolve caller identity from ``by``, ``actor_id``, or runtime actor identity."""
    env_aid = _runtime_context().actor_id
    arg_by = str(arguments.get("by") or "").strip()
    arg_actor_id = str(arguments.get("actor_id") or "").strip()
    if arg_by and arg_actor_id and arg_by != arg_actor_id:
        raise MCPError(
            code="actor_id_mismatch",
            message="by/actor_id mismatch (tool args must use one consistent actor id)",
            details={"by": arg_by, "actor_id": arg_actor_id},
        )
    arg_aid = arg_by or arg_actor_id
    aid = env_aid or arg_aid
    if not aid:
        raise MCPError(
            code="missing_actor_id",
            message="missing actor id (set CCCC_ACTOR_ID env or pass by/actor_id)",
        )
    if env_aid and arg_aid and arg_aid != env_aid:
        raise MCPError(
            code="actor_id_mismatch",
            message="actor id mismatch (tool args must match CCCC_ACTOR_ID)",
            details={"env": env_aid, "arg": arg_aid},
        )
    return _validate_self_actor_id(aid)


def _call_daemon_or_raise(req: Dict[str, Any], *, timeout_s: float = 60.0) -> Dict[str, Any]:
    """Call daemon, raise MCPError on failure."""
    ctx = _runtime_context()
    paths = DaemonPaths(Path(ctx.home)) if str(ctx.home or "").strip() else None
    attempts = []
    if paths is not None:
        attempts.append({"paths": paths, "timeout_s": float(timeout_s)})
        attempts.append({"paths": paths})
    attempts.append({"timeout_s": float(timeout_s)})
    attempts.append({})
    resp = None
    last_type_error: Optional[TypeError] = None
    for kwargs in attempts:
        try:
            resp = call_daemon(req, **kwargs)
            break
        except TypeError as e:
            msg = str(e)
            if "unexpected keyword argument" in msg:
                last_type_error = e
                continue
            raise
    if resp is None:
        if last_type_error is not None:
            raise last_type_error
        resp = call_daemon(req)
    if not resp.get("ok"):
        err = resp.get("error") or {}
        if isinstance(err, dict):
            raise MCPError(
                code=str(err.get("code") or "daemon_error"),
                message=str(err.get("message") or "daemon error"),
                details=(
                    err.get("details") if isinstance(err.get("details"), dict) else {}
                ),
            )
        raise MCPError(code="daemon_error", message=str(err))
    return resp.get("result") if isinstance(resp.get("result"), dict) else {}
