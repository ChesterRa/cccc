"""Codex-style code mode orchestration for ChatGPT Web Model MCP callers."""

from __future__ import annotations

import json
import os
import queue
import re
import shutil
import subprocess
import threading
import time
from contextvars import ContextVar
from importlib.resources import files
from typing import Any, Callable, Dict, List, Mapping, Optional

from ..common import MCPError, _runtime_context

NestedToolCaller = Callable[[str, Dict[str, Any]], Dict[str, Any]]
ListTools = Callable[[], List[Dict[str, Any]]]

CODE_MODE_EXEC_TOOL = "cccc_code_exec"
CODE_MODE_WAIT_TOOL = "cccc_code_wait"
CODE_MODE_TOOL_NAMES = {CODE_MODE_EXEC_TOOL, CODE_MODE_WAIT_TOOL}

_DEFAULT_YIELD_TIME_MS = 10_000
_DEFAULT_MAX_OUTPUT_TOKENS = 10_000
_MAX_YIELD_TIME_MS = 60_000
_MAX_OUTPUT_TOKENS = 50_000
_MAX_SOURCE_CHARS = 500_000
_MAX_CELLS = 16
_CELL_TTL_SECONDS = 30 * 60
_CODE_MODE_NESTED: ContextVar[bool] = ContextVar("cccc_code_mode_nested", default=False)
_LOCK = threading.Lock()
_CELLS: Dict[str, "_CodeCell"] = {}
_NEXT_CELL_ID = 1
_STORED_VALUES: Dict[str, Dict[str, Any]] = {}


def code_mode_enabled() -> bool:
    raw = str(os.environ.get("CCCC_WEB_MODEL_CODE_MODE") or "1").strip().lower()
    return raw not in {"0", "false", "no", "off", "disabled"}


def code_mode_nested_call_active() -> bool:
    return bool(_CODE_MODE_NESTED.get())


def _coerce_int(value: Any, *, default: int, minimum: int, maximum: int) -> int:
    try:
        out = int(value)
    except Exception:
        out = default
    return max(minimum, min(maximum, out))


def _store_key() -> str:
    ctx = _runtime_context()
    return "\x1f".join([ctx.home, ctx.group_id, ctx.actor_id])


def _normalize_identifier(name: str) -> str:
    raw = str(name or "").strip()
    out: List[str] = []
    for idx, ch in enumerate(raw):
        valid = ch == "_" or ch == "$" or ch.isascii() and (ch.isalpha() or (idx > 0 and ch.isdigit()))
        out.append(ch if valid else "_")
    value = "".join(out).strip("_")
    if not value or value[0].isdigit():
        value = f"tool_{value}"
    return value


def _tool_description(spec: Mapping[str, Any]) -> str:
    name = str(spec.get("name") or "").strip()
    description = str(spec.get("description") or "").strip()
    schema = spec.get("inputSchema")
    schema_text = ""
    if isinstance(schema, dict):
        try:
            schema_text = json.dumps(schema, ensure_ascii=False, sort_keys=True)
        except Exception:
            schema_text = ""
    parts = [description]
    if schema_text:
        parts.append(f"inputSchema={schema_text}")
    return "\n".join(part for part in parts if part).strip() or name


def _enabled_nested_tools(list_tools: ListTools) -> List[Dict[str, str]]:
    out: List[Dict[str, str]] = []
    seen: set[str] = set()
    for spec in list_tools():
        if not isinstance(spec, dict):
            continue
        name = str(spec.get("name") or "").strip()
        if not name or name in CODE_MODE_TOOL_NAMES or name in seen:
            continue
        seen.add(name)
        out.append(
            {
                "name": name,
                "global_name": _normalize_identifier(name),
                "description": _tool_description(spec),
            }
        )
    out.sort(key=lambda item: item["global_name"])
    return out


def _parse_exec_pragma(source: str) -> tuple[str, Dict[str, Any]]:
    if not source.startswith("// @exec:"):
        return source, {}
    first, sep, rest = source.partition("\n")
    if not sep:
        return source, {}
    raw = first[len("// @exec:") :].strip()
    if not raw:
        return rest, {}
    try:
        parsed = json.loads(raw)
    except Exception as exc:
        raise MCPError(code="invalid_pragma", message=f"failed to parse @exec pragma: {exc}") from exc
    if not isinstance(parsed, dict):
        raise MCPError(code="invalid_pragma", message="@exec pragma must be a JSON object")
    allowed = {"yield_time_ms", "max_output_tokens"}
    extra = sorted(set(parsed) - allowed)
    if extra:
        raise MCPError(code="invalid_pragma", message=f"unsupported @exec pragma keys: {', '.join(extra)}")
    return rest, parsed


def _reject_unsupported_source(source: str) -> None:
    if len(source) > _MAX_SOURCE_CHARS:
        raise MCPError(
            code="source_too_large",
            message=f"source exceeds {_MAX_SOURCE_CHARS} characters",
            details={"recommended_action": "Split the work into a smaller cccc_code_exec cell or move large data into files under the active scope."},
        )
    if re.search(r"(^|[^\w$])require\s*\(", source):
        raise MCPError(
            code="unsupported_js",
            message="cccc_code_exec does not expose require()",
            details={"recommended_action": "Use nested MCP tools such as tools.cccc_repo, tools.cccc_shell, or tools.cccc_exec_command instead of Node host APIs."},
        )
    if re.search(r"(^|[^\w$])import\s*\(", source) or re.search(r"(^|[^\w$])import\s+(['\"{*$A-Za-z_])", source):
        raise MCPError(
            code="unsupported_js",
            message="cccc_code_exec does not support import",
            details={"recommended_action": "Use nested MCP tools for filesystem, shell, git, and CCCC operations; do not import Node modules in code mode."},
        )


def _find_node() -> str:
    explicit = str(os.environ.get("CCCC_CODE_MODE_NODE") or "").strip()
    if explicit:
        return explicit
    found = shutil.which("node")
    if found:
        return found
    raise MCPError(
        code="node_not_found",
        message="cccc_code_exec requires Node.js on the CCCC server host; direct MCP tools remain available",
        details={"recommended_action": "Use direct cccc_repo/cccc_shell/cccc_git tools, or install Node.js on the CCCC server host."},
    )


class _CodeCell:
    def __init__(self, *, cell_id: str, proc: subprocess.Popen[str], store_key: str) -> None:
        self.cell_id = cell_id
        self.proc = proc
        self.store_key = store_key
        self.events: "queue.Queue[Dict[str, Any]]" = queue.Queue()
        self.started_at = time.monotonic()
        self.last_used_at = self.started_at
        self.returned_items: List[Dict[str, Any]] = []

    def send(self, payload: Dict[str, Any]) -> None:
        if self.proc.stdin is None or self.proc.poll() is not None:
            raise MCPError(code="cell_closed", message=f"exec cell {self.cell_id} is not running")
        self.proc.stdin.write(json.dumps(payload, ensure_ascii=False) + "\n")
        self.proc.stdin.flush()


def _reader_thread(cell: _CodeCell) -> None:
    stream = cell.proc.stdout
    if stream is None:
        return
    for line in stream:
        text = str(line or "").strip()
        if not text:
            continue
        try:
            event = json.loads(text)
        except Exception:
            event = {"type": "stderr", "text": text}
        if isinstance(event, dict):
            cell.events.put(event)


def _new_cell_id() -> str:
    global _NEXT_CELL_ID
    with _LOCK:
        cell_id = str(_NEXT_CELL_ID)
        _NEXT_CELL_ID += 1
    return cell_id


def _prune_cells() -> None:
    now = time.monotonic()
    stale: List[str] = []
    with _LOCK:
        for cell_id, cell in list(_CELLS.items()):
            if cell.proc.poll() is not None or now - cell.last_used_at > _CELL_TTL_SECONDS:
                stale.append(cell_id)
        while len(_CELLS) - len(stale) >= _MAX_CELLS:
            oldest = min(
                ((cid, cell.last_used_at) for cid, cell in _CELLS.items() if cid not in stale),
                key=lambda item: item[1],
                default=("", 0.0),
            )[0]
            if not oldest:
                break
            stale.append(oldest)
        for cell_id in stale:
            cell = _CELLS.pop(cell_id, None)
            if cell is not None and cell.proc.poll() is None:
                try:
                    cell.proc.terminate()
                except Exception:
                    pass


def _store_cell(cell: _CodeCell) -> None:
    _prune_cells()
    with _LOCK:
        _CELLS[cell.cell_id] = cell


def _pop_cell(cell_id: str) -> Optional[_CodeCell]:
    with _LOCK:
        return _CELLS.pop(cell_id, None)


def _get_cell(cell_id: str) -> Optional[_CodeCell]:
    with _LOCK:
        return _CELLS.get(cell_id)


def _missing_cell_response(cell_id: str) -> Dict[str, Any]:
    return {
        "status": "missing",
        "status_text": f"exec cell {cell_id} not found",
        "cell_id": cell_id,
        "running": False,
        "output": "",
        "items": [],
        "output_truncated": False,
        "error_text": f"exec cell {cell_id} not found",
        "recommended_action": "Check the cell_id from the latest cccc_code_exec result; if the cell expired or belonged to another actor, start a new cccc_code_exec cell.",
    }


def _update_stored_values(store_key: str, value: Any) -> None:
    if isinstance(value, dict):
        with _LOCK:
            _STORED_VALUES[store_key] = value


def _stored_values(store_key: str) -> Dict[str, Any]:
    with _LOCK:
        value = _STORED_VALUES.get(store_key)
        return dict(value) if isinstance(value, dict) else {}


def _content_text(item: Dict[str, Any]) -> str:
    if str(item.get("type") or "text") == "text":
        return str(item.get("text") or "")
    try:
        return json.dumps(item, ensure_ascii=False)
    except Exception:
        return str(item)


def _truncate_output(items: List[Dict[str, Any]], max_output_tokens: int) -> tuple[List[Dict[str, Any]], bool]:
    max_chars = max(1, int(max_output_tokens) * 4)
    remaining = max_chars
    truncated = False
    out: List[Dict[str, Any]] = []
    for item in items:
        text = _content_text(item)
        if len(text) > remaining:
            out.append({"type": "text", "text": text[:remaining] + "\n[truncated]"})
            truncated = True
            break
        out.append(dict(item))
        remaining -= len(text)
        if remaining <= 0:
            truncated = True
            break
    return out, truncated


def _format_response(
    *,
    status: str,
    cell_id: str,
    items: List[Dict[str, Any]],
    started_at: float,
    max_output_tokens: int,
    error_text: str = "",
) -> Dict[str, Any]:
    trimmed, truncated = _truncate_output(items, max_output_tokens)
    output = "\n".join(_content_text(item) for item in trimmed)
    running = status == "running"
    if status == "running":
        status_text = f"Script running with cell ID {cell_id}"
    elif status == "terminated":
        status_text = "Script terminated"
    elif status == "failed":
        status_text = "Script failed"
    else:
        status_text = "Script completed"
    recommended_action = ""
    if status == "failed":
        recommended_action = "Read error_text, fix the JS or nested tool call, then rerun a smaller cccc_code_exec cell."
    elif running:
        recommended_action = "Call cccc_code_wait with this cell_id to collect more output or final status."
    elif truncated:
        recommended_action = (
            "Output was truncated; rerun with narrower commands/line ranges or increase max_output_tokens up to 50000."
        )
    return {
        "status": status,
        "status_text": status_text,
        "cell_id": cell_id,
        "running": running,
        "wall_time_seconds": round(max(0.0, time.monotonic() - started_at), 3),
        "output": output,
        "items": trimmed,
        "output_truncated": truncated,
        "error_text": error_text,
        "recommended_action": recommended_action,
    }


def _send_tool_response(cell: _CodeCell, *, event_id: str, ok: bool, result: Any = None, error: str = "") -> None:
    cell.send(
        {
            "type": "tool_response",
            "id": event_id,
            "ok": ok,
            "result": result,
            "error": error,
        }
    )


def _nested_tool_error_text(exc: MCPError) -> str:
    detail = exc.details if isinstance(exc.details, dict) else {}
    recommended = str(detail.get("recommended_action") or "").strip()
    if recommended:
        return f"{exc.code}: {exc.message}\nrecommended_action: {recommended}"
    return f"{exc.code}: {exc.message}" if exc.code else exc.message


def _call_nested_tool(cell: _CodeCell, event: Dict[str, Any], nested_tool_caller: NestedToolCaller) -> None:
    event_id = str(event.get("id") or "").strip()
    name = str(event.get("name") or "").strip()
    raw_input = event.get("input")
    if not event_id or not name:
        return
    if name in CODE_MODE_TOOL_NAMES:
        _send_tool_response(cell, event_id=event_id, ok=False, error=f"{name} cannot be invoked from code mode")
        return
    if raw_input is None:
        args: Dict[str, Any] = {}
    elif isinstance(raw_input, dict):
        args = raw_input
    else:
        _send_tool_response(cell, event_id=event_id, ok=False, error=f"{name} expects a JSON object argument")
        return
    token = _CODE_MODE_NESTED.set(True)
    try:
        result = nested_tool_caller(name, args)
    except MCPError as exc:
        _send_tool_response(cell, event_id=event_id, ok=False, error=_nested_tool_error_text(exc))
    except Exception as exc:
        _send_tool_response(cell, event_id=event_id, ok=False, error=str(exc))
    else:
        _send_tool_response(cell, event_id=event_id, ok=True, result=result)
    finally:
        _CODE_MODE_NESTED.reset(token)


def _drain_events(
    cell: _CodeCell,
    *,
    nested_tool_caller: NestedToolCaller,
    yield_time_ms: int,
    max_output_tokens: int,
) -> Dict[str, Any]:
    deadline = time.monotonic() + max(0, yield_time_ms) / 1000.0
    items: List[Dict[str, Any]] = []
    cell.last_used_at = time.monotonic()
    while True:
        timeout = max(0.0, deadline - time.monotonic())
        if timeout <= 0:
            if cell.proc.poll() is None:
                cell.returned_items.extend(items)
                return _format_response(
                    status="running",
                    cell_id=cell.cell_id,
                    items=items,
                    started_at=cell.started_at,
                    max_output_tokens=max_output_tokens,
                )
            items.append({"type": "text", "text": "Script error:\nexec runtime ended unexpectedly"})
            return _format_response(
                status="failed",
                cell_id=cell.cell_id,
                items=items,
                started_at=cell.started_at,
                max_output_tokens=max_output_tokens,
                error_text="exec runtime ended unexpectedly",
            )
        try:
            event = cell.events.get(timeout=timeout if timeout > 0 else 0.01)
        except queue.Empty:
            continue
        typ = str(event.get("type") or "").strip()
        if typ == "started":
            continue
        if typ == "content":
            item = event.get("item")
            if isinstance(item, dict):
                items.append(item)
            continue
        if typ == "tool_call":
            _call_nested_tool(cell, event, nested_tool_caller)
            continue
        if typ == "yield":
            _update_stored_values(cell.store_key, event.get("stored_values"))
            cell.returned_items.extend(items)
            return _format_response(
                status="running",
                cell_id=cell.cell_id,
                items=items,
                started_at=cell.started_at,
                max_output_tokens=max_output_tokens,
            )
        if typ == "result":
            _pop_cell(cell.cell_id)
            _update_stored_values(cell.store_key, event.get("stored_values"))
            error_text = str(event.get("error_text") or "").strip()
            status = "failed" if error_text else "completed"
            if error_text:
                items.append({"type": "text", "text": f"Script error:\n{error_text}"})
            return _format_response(
                status=status,
                cell_id=cell.cell_id,
                items=items,
                started_at=cell.started_at,
                max_output_tokens=max_output_tokens,
                error_text=error_text,
            )
        if typ == "stderr":
            items.append({"type": "text", "text": str(event.get("text") or "")})


def _start_node_cell(
    *,
    source: str,
    nested_tools: List[Dict[str, str]],
    yield_time_ms: int,
) -> _CodeCell:
    cell_id = _new_cell_id()
    store_key = _store_key()
    node = _find_node()
    try:
        proc = subprocess.Popen(
            [node, "-e", _NODE_SCRIPT],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )
    except OSError as exc:
        raise MCPError(code="code_mode_start_failed", message=f"failed to start Node.js code runtime: {exc}") from exc
    cell = _CodeCell(cell_id=cell_id, proc=proc, store_key=store_key)
    threading.Thread(target=_reader_thread, args=(cell,), daemon=True).start()
    _store_cell(cell)
    cell.send(
        {
            "type": "start",
            "cell_id": cell_id,
            "source": source,
            "tools": nested_tools,
            "work_loops": _COMMON_WORK_LOOPS,
            "help_aliases": _TOOL_HELP_ALIASES,
            "help_compact_notes": _TOOL_HELP_COMPACT_NOTES,
            "help_curated_tools": _TOOL_HELP_CURATED_TOOLS,
            "help_curated_loops": _TOOL_HELP_CURATED_LOOPS,
            "stored_values": _stored_values(store_key),
            "yield_time_ms": yield_time_ms,
        }
    )
    return cell


def code_exec_tool(
    arguments: Dict[str, Any],
    *,
    nested_tool_caller: NestedToolCaller,
    list_tools: ListTools,
) -> Dict[str, Any]:
    if not code_mode_enabled():
        raise MCPError(
            code="code_mode_disabled",
            message="cccc_code_exec is disabled by CCCC_WEB_MODEL_CODE_MODE=0",
            details={"recommended_action": "Use direct cccc_repo/cccc_shell/cccc_git tools, or enable CCCC_WEB_MODEL_CODE_MODE on the CCCC server."},
        )
    if code_mode_nested_call_active():
        raise MCPError(
            code="recursive_code_mode",
            message="cccc_code_exec cannot be called from inside code mode",
            details={"recommended_action": "Inside code mode, call nested tools through global tools.<toolName>(args) instead."},
        )
    source = str(arguments.get("source") or arguments.get("code") or "")
    source, pragma = _parse_exec_pragma(source)
    if not source.strip():
        raise MCPError(
            code="missing_source",
            message="source is required",
            details={"recommended_action": "Pass raw JavaScript in source; do not wrap it in markdown fences."},
        )
    _reject_unsupported_source(source)
    yield_time_ms = _coerce_int(
        arguments.get("yield_time_ms", pragma.get("yield_time_ms")),
        default=_DEFAULT_YIELD_TIME_MS,
        minimum=0,
        maximum=_MAX_YIELD_TIME_MS,
    )
    max_output_tokens = _coerce_int(
        arguments.get("max_output_tokens", pragma.get("max_output_tokens")),
        default=_DEFAULT_MAX_OUTPUT_TOKENS,
        minimum=1,
        maximum=_MAX_OUTPUT_TOKENS,
    )
    nested_tools = _enabled_nested_tools(list_tools)
    cell = _start_node_cell(source=source, nested_tools=nested_tools, yield_time_ms=yield_time_ms)
    return _drain_events(
        cell,
        nested_tool_caller=nested_tool_caller,
        yield_time_ms=yield_time_ms,
        max_output_tokens=max_output_tokens,
    )


def code_wait_tool(
    arguments: Dict[str, Any],
    *,
    nested_tool_caller: NestedToolCaller,
) -> Dict[str, Any]:
    if not code_mode_enabled():
        raise MCPError(
            code="code_mode_disabled",
            message="cccc_code_wait is disabled by CCCC_WEB_MODEL_CODE_MODE=0",
            details={"recommended_action": "Use direct tools or re-enable CCCC_WEB_MODEL_CODE_MODE before waiting on code cells."},
        )
    if code_mode_nested_call_active():
        raise MCPError(
            code="recursive_code_mode",
            message="cccc_code_wait cannot be called from inside code mode",
            details={"recommended_action": "Use yield_control() inside code mode; call cccc_code_wait only from the outer MCP client."},
        )
    cell_id = str(arguments.get("cell_id") or "").strip()
    if not cell_id:
        raise MCPError(
            code="missing_cell_id",
            message="cell_id is required",
            details={"recommended_action": "Use the cell_id returned by cccc_code_exec when status=running."},
        )
    cell = _get_cell(cell_id)
    if cell is None or cell.store_key != _store_key():
        return _missing_cell_response(cell_id)
    max_output_tokens = _coerce_int(
        arguments.get("max_tokens") or arguments.get("max_output_tokens"),
        default=_DEFAULT_MAX_OUTPUT_TOKENS,
        minimum=1,
        maximum=_MAX_OUTPUT_TOKENS,
    )
    if bool(arguments.get("terminate")):
        _pop_cell(cell_id)
        if cell.proc.poll() is None:
            try:
                cell.proc.terminate()
            except Exception:
                pass
        return _format_response(
            status="terminated",
            cell_id=cell_id,
            items=[],
            started_at=cell.started_at,
            max_output_tokens=max_output_tokens,
        )
    yield_time_ms = _coerce_int(
        arguments.get("yield_time_ms"),
        default=_DEFAULT_YIELD_TIME_MS,
        minimum=0,
        maximum=_MAX_YIELD_TIME_MS,
    )
    return _drain_events(
        cell,
        nested_tool_caller=nested_tool_caller,
        yield_time_ms=yield_time_ms,
        max_output_tokens=max_output_tokens,
    )

def _load_shared_code_mode_resources() -> tuple[str, Dict[str, Any]]:
    resources = files("cccc.resources")
    runtime = resources.joinpath("code_mode_runtime.js").read_text(encoding="utf-8")
    metadata = json.loads(resources.joinpath("code_mode_metadata.json").read_text(encoding="utf-8"))
    if not isinstance(metadata, dict):
        raise RuntimeError("cccc.resources/code_mode_metadata.json must contain a JSON object")
    return runtime, metadata


_NODE_SCRIPT, _SHARED_METADATA = _load_shared_code_mode_resources()
_COMMON_WORK_LOOPS = list(_SHARED_METADATA.get("work_loops") or [])
_TOOL_HELP_ALIASES = dict(_SHARED_METADATA.get("help_aliases") or {})
_TOOL_HELP_COMPACT_NOTES = dict(_SHARED_METADATA.get("help_compact_notes") or {})
_TOOL_HELP_CURATED_TOOLS = dict(_SHARED_METADATA.get("help_curated_tools") or {})
_TOOL_HELP_CURATED_LOOPS = dict(_SHARED_METADATA.get("help_curated_loops") or {})
