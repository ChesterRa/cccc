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
_COMMON_WORK_LOOPS = [
    {
        "name": "review_current_diff",
        "steps": [
            "cccc_git(action='status')",
            "cccc_git(action='diff')",
            "cccc_repo(action='read', start_line/end_line as needed)",
            "cccc_shell or cccc_exec_command for focused tests",
            "cccc_message_reply with findings/evidence",
            "cccc_runtime_complete_turn for the processed event ids",
        ],
    },
    {
        "name": "patch_safely",
        "steps": [
            "cccc_repo(action='read') to get content and sha256",
            "cccc_repo_edit(action='replace' or 'multi_replace', expected_sha256=sha256) for exact edits",
            "cccc_apply_patch only for Codex *** Begin Patch file patches",
            "cccc_git(action='diff')",
            "focused validation command",
        ],
    },
    {
        "name": "attachments",
        "steps": [
            "cccc_file(action='read', rel_path=...) for text blobs delivered by CCCC",
            "cccc_file(action='blob_path', rel_path=...) for binary/local inspection",
            "create output files under the active scope",
            "cccc_file(action='send', path=..., text=...) to return files as CCCC attachments",
        ],
    },
    {
        "name": "finish_turn",
        "steps": [
            "send visible result with cccc_message_reply or cccc_message_send",
            "refresh cccc_agent_state when focus/next_action/blockers changed",
            "call cccc_runtime_complete_turn with the turn/event ids from the delivered work",
        ],
    },
]
_TOOL_HELP_ALIASES = {
    "turn_complete": ["runtime_complete", "complete_turn", "finish_turn"],
    "complete_turn": ["runtime_complete", "turn_complete", "finish_turn"],
    "runtime_complete": ["complete_turn", "turn_complete", "finish_turn"],
    "finish_turn": ["complete_turn", "runtime_complete", "message", "state"],
    "complete": ["runtime_complete", "complete_turn", "finish_turn"],
    "reply": ["message", "message_reply"],
    "send": ["message", "message_send"],
    "message": ["reply", "send"],
    "state": ["agent_state"],
    "agent": ["agent_state"],
    "repo": ["repository", "patch", "git"],
    "patch": ["repo_edit", "apply_patch", "repository"],
    "file": ["attachment", "blob"],
    "attachment": ["file", "blob"],
    "shell": ["exec", "command"],
    "exec": ["shell", "command"],
    "code": ["code_exec"],
}
_TOOL_HELP_COMPACT_NOTES = {
    "repo": [
        "Use cccc_repo(action='read') to inspect text and get sha256.",
        "Use cccc_repo_edit(action='replace'|'multi_replace', expected_sha256=...) for exact small edits.",
        "Use cccc_apply_patch only for Codex *** Begin Patch file patches.",
        "After edits, inspect cccc_git(action='diff') and run focused validation.",
    ],
    "file": [
        "CCCC attachments are blob references, not browser uploads.",
        "Use cccc_file(action='read', rel_path=...) for UTF-8 text attachments.",
        "Use cccc_file(action='blob_path', rel_path=...) for binary/local inspection.",
        "Use cccc_file(action='send', path=..., text=...) to return active-scope files as chat attachments.",
    ],
    "shell": [
        "Use cccc_shell for short one-shot commands.",
        "Use cccc_exec_command plus cccc_write_stdin for long-running or interactive commands.",
        "Keep cwd under the active scope and cap output with max_output_bytes.",
    ],
    "message": [
        "Visible CCCC delivery must use cccc_message_send or cccc_message_reply.",
        "Use cccc_message_reply with event_id/reply_to for an existing thread.",
        "Verify to/reply_to before sending; avoid routine @all.",
    ],
    "state": [
        "Use cccc_agent_state to keep focus, next_action, what_changed, active_task_id, and blockers fresh.",
        "Update state at real transitions; do not let status replies replace actual work.",
    ],
    "runtime_complete": [
        "Use cccc_runtime_complete_turn after processing Web Model turn event ids.",
        "Completion is explicit control-plane hygiene; visible web chat text alone does not complete a turn.",
        "If a browser-delivered turn has already been committed, missing completion should not block later delivery, but it still leaves stale status.",
    ],
    "code_exec": [
        "Use cccc_code_exec for multi-step read/patch/test/diff/report loops.",
        "Inside code mode, call nested tools as await tools.<toolName>({...}).",
        "Use COMMON_WORK_LOOPS, tool_names(query), list_tools(query), and tool_help(query) when unsure.",
    ],
}
_TOOL_HELP_CURATED_TOOLS = {
    "finish_turn": [
        "cccc_message_reply",
        "cccc_message_send",
        "cccc_agent_state",
        "cccc_runtime_complete_turn",
        "cccc_runtime_wait_next_turn",
    ],
    "turn_complete": [
        "cccc_runtime_complete_turn",
        "cccc_message_reply",
        "cccc_message_send",
        "cccc_agent_state",
        "cccc_runtime_wait_next_turn",
    ],
    "complete_turn": [
        "cccc_runtime_complete_turn",
        "cccc_message_reply",
        "cccc_message_send",
        "cccc_agent_state",
        "cccc_runtime_wait_next_turn",
    ],
    "runtime_complete": [
        "cccc_runtime_complete_turn",
        "cccc_runtime_wait_next_turn",
        "cccc_message_reply",
        "cccc_agent_state",
    ],
    "repo": [
        "cccc_repo",
        "cccc_repo_edit",
        "cccc_apply_patch",
        "cccc_git",
        "cccc_shell",
        "cccc_exec_command",
        "cccc_write_stdin",
    ],
    "patch": [
        "cccc_repo",
        "cccc_repo_edit",
        "cccc_apply_patch",
        "cccc_git",
        "cccc_shell",
    ],
    "file": [
        "cccc_file",
        "cccc_repo",
        "cccc_repo_edit",
        "cccc_shell",
    ],
    "attachment": [
        "cccc_file",
        "cccc_message_reply",
        "cccc_message_send",
    ],
    "message": [
        "cccc_message_reply",
        "cccc_message_send",
        "cccc_tracked_send",
        "cccc_inbox_list",
        "cccc_inbox_mark_read",
    ],
    "reply": [
        "cccc_message_reply",
        "cccc_message_send",
        "cccc_inbox_list",
    ],
    "state": [
        "cccc_agent_state",
        "cccc_context_get",
        "cccc_coordination",
        "cccc_task",
    ],
    "runtime": [
        "cccc_runtime_complete_turn",
        "cccc_runtime_wait_next_turn",
        "cccc_runtime_list",
        "cccc_agent_state",
    ],
    "code_exec": [
        "cccc_code_exec",
        "cccc_code_wait",
        "cccc_repo",
        "cccc_repo_edit",
        "cccc_apply_patch",
        "cccc_shell",
        "cccc_git",
    ],
}
_TOOL_HELP_CURATED_LOOPS = {
    "finish_turn": ["finish_turn"],
    "turn_complete": ["finish_turn"],
    "complete_turn": ["finish_turn"],
    "runtime_complete": ["finish_turn"],
    "repo": ["patch_safely", "review_current_diff"],
    "patch": ["patch_safely", "review_current_diff"],
    "file": ["attachments", "patch_safely"],
    "attachment": ["attachments"],
    "message": ["finish_turn"],
    "reply": ["finish_turn"],
    "state": ["finish_turn"],
    "runtime": ["finish_turn"],
    "code_exec": ["review_current_diff", "patch_safely", "finish_turn"],
}

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


_NODE_SCRIPT = r"""
const readline = require("node:readline");
const vm = require("node:vm");

const EXIT_SENTINEL = "__cccc_code_mode_exit__";
let pending = new Map();
let storedValues = {};
let nextToolId = 1;
let started = false;

function send(payload) {
  process.stdout.write(JSON.stringify(payload) + "\n");
}

function finish(errorText = "", storedValuesOverride = null) {
  const values = storedValuesOverride && typeof storedValuesOverride === "object" ? storedValuesOverride : storedValues;
  const payload = JSON.stringify({ type: "result", stored_values: values, error_text: errorText }) + "\n";
  process.stdout.write(payload, () => process.exit(0));
}

function jsonString(value) {
  try {
    const text = JSON.stringify(value === undefined ? null : value);
    return typeof text === "string" ? text : "null";
  } catch (_err) {
    return "null";
  }
}

function parseJsonObject(text, fallback = {}) {
  try {
    const parsed = JSON.parse(String(text || "{}"));
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : fallback;
  } catch (_err) {
    return fallback;
  }
}

function hardenFunction(fn) {
  Object.setPrototypeOf(fn, null);
  return Object.freeze(fn);
}

function buildBridge() {
  const bridge = Object.create(null);
  Object.defineProperties(bridge, {
    content: {
      value: hardenFunction((itemJson) => {
        send({ type: "content", item: JSON.parse(String(itemJson || "{}")) });
      }),
    },
    toolCall: {
      value: hardenFunction((rawName, payloadJson, resolveJson, rejectMessage) => {
        const id = `tool-${nextToolId++}`;
        let payload = null;
        try {
          payload = JSON.parse(String(payloadJson || "null"));
        } catch (err) {
          rejectMessage(String(err && err.message || err));
          return;
        }
        pending.set(id, { resolveJson, rejectMessage });
        send({ type: "tool_call", id, name: String(rawName || ""), input: payload });
      }),
    },
    yield: {
      value: hardenFunction((storedJson) => {
        send({ type: "yield", stored_values: parseJsonObject(storedJson, {}) });
      }),
    },
    setTimeout: { value: hardenFunction((callback, ms, ...args) => setTimeout(callback, ms, ...args)) },
    clearTimeout: { value: hardenFunction((id) => clearTimeout(id)) },
  });
  return Object.freeze(bridge);
}

function buildContext(toolsMetadata, initialStoredValues, workLoops, helpAliases, helpCompactNotes, helpCuratedTools, helpCuratedLoops) {
  const sandbox = Object.create(null);
  Object.defineProperties(sandbox, {
    __cccc_bridge__: { value: buildBridge(), configurable: true },
    __cccc_tools_metadata_json__: { value: JSON.stringify(Array.isArray(toolsMetadata) ? toolsMetadata : []), configurable: true },
    __cccc_work_loops_json__: { value: JSON.stringify(Array.isArray(workLoops) ? workLoops : []), configurable: true },
    __cccc_help_aliases_json__: { value: JSON.stringify(helpAliases && typeof helpAliases === "object" ? helpAliases : {}), configurable: true },
    __cccc_help_compact_notes_json__: { value: JSON.stringify(helpCompactNotes && typeof helpCompactNotes === "object" ? helpCompactNotes : {}), configurable: true },
    __cccc_help_curated_tools_json__: { value: JSON.stringify(helpCuratedTools && typeof helpCuratedTools === "object" ? helpCuratedTools : {}), configurable: true },
    __cccc_help_curated_loops_json__: { value: JSON.stringify(helpCuratedLoops && typeof helpCuratedLoops === "object" ? helpCuratedLoops : {}), configurable: true },
    __cccc_stored_values_json__: { value: jsonString(initialStoredValues && typeof initialStoredValues === "object" ? initialStoredValues : {}), configurable: true },
    __cccc_exit_sentinel__: { value: EXIT_SENTINEL, configurable: true },
    constructor: { value: undefined, configurable: true },
    console: { value: undefined, configurable: true },
    require: { value: undefined, configurable: true },
    process: { value: undefined, configurable: true },
    fetch: { value: undefined, configurable: true },
    WebSocket: { value: undefined, configurable: true },
  });
  const context = vm.createContext(sandbox, {
    name: "cccc_code_mode",
    codeGeneration: { strings: false, wasm: false },
  });
  const bootstrap = `
(() => {
  const bridge = globalThis.__cccc_bridge__;
  const toolsMetadata = JSON.parse(globalThis.__cccc_tools_metadata_json__ || "[]");
  const commonWorkLoops = Object.freeze(JSON.parse(globalThis.__cccc_work_loops_json__ || "[]"));
  const helpAliases = JSON.parse(globalThis.__cccc_help_aliases_json__ || "{}");
  const helpCompactNotes = JSON.parse(globalThis.__cccc_help_compact_notes_json__ || "{}");
  const helpCuratedTools = JSON.parse(globalThis.__cccc_help_curated_tools_json__ || "{}");
  const helpCuratedLoops = JSON.parse(globalThis.__cccc_help_curated_loops_json__ || "{}");
  const exitSentinel = String(globalThis.__cccc_exit_sentinel__ || "");
  let storedValues = JSON.parse(globalThis.__cccc_stored_values_json__ || "{}");
  delete globalThis.__cccc_bridge__;
  delete globalThis.__cccc_tools_metadata_json__;
  delete globalThis.__cccc_work_loops_json__;
  delete globalThis.__cccc_help_aliases_json__;
  delete globalThis.__cccc_help_compact_notes_json__;
  delete globalThis.__cccc_help_curated_tools_json__;
  delete globalThis.__cccc_help_curated_loops_json__;
  delete globalThis.__cccc_stored_values_json__;
  delete globalThis.__cccc_exit_sentinel__;

  function define(name, value) {
    Object.defineProperty(globalThis, name, {
      value,
      writable: false,
      configurable: false,
      enumerable: false,
    });
  }

  define("constructor", undefined);
  define("console", undefined);
  define("require", undefined);
  define("process", undefined);
  define("fetch", undefined);
  define("WebSocket", undefined);

  function stringify(value) {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    if (typeof value === "string") return value;
    try {
      return JSON.stringify(value);
    } catch (_err) {
      return String(value);
    }
  }

  function cloneSerializable(value, label) {
    if (value === undefined) return undefined;
    try {
      return JSON.parse(JSON.stringify(value));
    } catch (_err) {
      throw new TypeError(label + " must be JSON-serializable");
    }
  }

  const tools = Object.create(null);
  for (const tool of toolsMetadata) {
    const globalName = String(tool.global_name || "");
    const rawName = String(tool.name || "");
    if (!globalName || !rawName) continue;
    Object.defineProperty(tools, globalName, {
      enumerable: true,
      value(input = {}) {
        let payloadJson = "null";
        try {
          const payload = input === undefined ? null : cloneSerializable(input, rawName + " input");
          payloadJson = JSON.stringify(payload);
        } catch (err) {
          return Promise.reject(err);
        }
        return new Promise((resolve, reject) => {
          bridge.toolCall(
            rawName,
            payloadJson,
            (resultJson) => {
              try {
                resolve(JSON.parse(String(resultJson || "null")));
              } catch (err) {
                reject(err);
              }
            },
            (message) => reject(new Error(String(message || "tool call failed")))
          );
        });
      },
    });
  }
  Object.freeze(tools);

  const allTools = toolsMetadata.map((tool) => Object.freeze({
    name: String(tool.global_name || ""),
    raw_name: String(tool.name || ""),
    description: String(tool.description || ""),
  }));
  define("tools", tools);
  define("ALL_TOOLS", Object.freeze(allTools));
  define("COMMON_WORK_LOOPS", commonWorkLoops);
  function normalizeHelpOptions(options) {
    if (options && typeof options === "object" && !Array.isArray(options)) return options;
    return {};
  }
  function queryTokens(query) {
    const needle = String(query || "").trim().toLowerCase();
    if (!needle) return [];
    const tokens = new Set([needle]);
    for (const part of needle.split(/[^a-z0-9_]+/).filter(Boolean)) tokens.add(part);
    const aliases = helpAliases && Array.isArray(helpAliases[needle]) ? helpAliases[needle] : [];
    for (const alias of aliases) tokens.add(String(alias || "").trim().toLowerCase());
    for (const [key, values] of Object.entries(helpAliases || {})) {
      if (Array.isArray(values) && values.map((item) => String(item || "").trim().toLowerCase()).includes(needle)) {
        tokens.add(String(key || "").trim().toLowerCase());
      }
    }
    return Array.from(tokens).filter(Boolean);
  }
  function canonicalQuery(tokens) {
    for (const token of tokens) {
      if (Array.isArray(helpCuratedTools[token]) || Array.isArray(helpCuratedLoops[token])) return token;
    }
    return tokens.length ? tokens[0] : "";
  }
  function rankByCurated(items, names, getName) {
    if (!Array.isArray(names) || !names.length) return items;
    const rank = new Map(names.map((name, index) => [String(name || ""), index]));
    const selected = [];
    const rest = [];
    for (const item of items) {
      if (rank.has(String(getName(item) || ""))) selected.push(item);
      else rest.push(item);
    }
    selected.sort((a, b) => (rank.get(String(getName(a) || "")) || 0) - (rank.get(String(getName(b) || "")) || 0));
    return selected.concat(rest);
  }
  function toolMatches(tool, tokens) {
    if (!tokens.length) return true;
    const haystack = [tool.name, tool.raw_name, tool.description].join(" ").toLowerCase();
    return tokens.some((token) => haystack.includes(token));
  }
  function loopMatches(loop, tokens) {
    if (!tokens.length) return true;
    const haystack = [loop && loop.name || "", Array.isArray(loop && loop.steps) ? loop.steps.join(" ") : ""].join(" ").toLowerCase();
    return tokens.some((token) => haystack.includes(token));
  }
  function compactTool(tool) {
    return {
      name: tool.name,
      raw_name: tool.raw_name,
      summary: String(tool.description || "").split(/\\n/)[0].slice(0, 320),
    };
  }
  function compactNotes(tokens) {
    const notes = [];
    const seen = new Set();
    for (const token of tokens.length ? tokens : ["code_exec"]) {
      const values = Array.isArray(helpCompactNotes[token]) ? helpCompactNotes[token] : [];
      for (const value of values) {
        const text = String(value || "").trim();
        if (text && !seen.has(text)) {
          notes.push(text);
          seen.add(text);
        }
      }
    }
    return notes.slice(0, 12);
  }
  function matchingTools(query) {
    const tokens = queryTokens(query);
    const canonical = canonicalQuery(tokens);
    const curated = Array.isArray(helpCuratedTools[canonical]) ? helpCuratedTools[canonical] : [];
    const matched = allTools.filter((tool) => toolMatches(tool, tokens));
    if (curated.length) {
      const curatedSet = new Set(curated.map((name) => String(name || "")));
      const exact = allTools.filter((tool) => curatedSet.has(tool.raw_name));
      const selected = exact.length ? exact : matched;
      const seen = new Set();
      return rankByCurated(selected, curated, (tool) => tool.raw_name).filter((tool) => {
        const key = tool.raw_name;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    }
    return matched;
  }
  function matchingLoops(query) {
    const tokens = queryTokens(query);
    const canonical = canonicalQuery(tokens);
    const curated = Array.isArray(helpCuratedLoops[canonical]) ? helpCuratedLoops[canonical] : [];
    const matched = commonWorkLoops.filter((loop) => loopMatches(loop, tokens));
    if (curated.length) {
      const curatedSet = new Set(curated.map((name) => String(name || "")));
      const exact = commonWorkLoops.filter((loop) => curatedSet.has(String(loop && loop.name || "")));
      const selected = exact.length ? exact : matched;
      const seen = new Set();
      return rankByCurated(selected, curated, (loop) => String(loop && loop.name || "")).filter((loop) => {
        const key = String(loop && loop.name || "");
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    }
    return matched;
  }
  define("tool_names", function tool_names(query = "") {
    return matchingTools(query).map((tool) => tool.name);
  });
  define("list_tools", function list_tools(query = "") {
    return matchingTools(query).map(compactTool).slice(0, 24);
  });
  define("tool_help", function tool_help(query = "", options = {}) {
    const opts = normalizeHelpOptions(options);
    const detail = String(opts.detail || opts.mode || "compact").trim().toLowerCase();
    const tokens = queryTokens(query);
    const matches = matchingTools(query).slice(0, 12);
    const loops = matchingLoops(query).slice(0, 6);
    return {
      tools: detail === "schema" || detail === "full" ? matches : matches.map(compactTool),
      common_work_loops: loops,
      notes: compactNotes(tokens),
      usage: "Call nested tools as await tools.<name>({...}). Prefer cccc_code_exec for multi-step read/patch/test/diff/report loops; use direct tools for one-step work.",
    };
  });
  define("text", function text(value) {
    bridge.content(JSON.stringify({ type: "text", text: stringify(value) }));
  });
  define("store", function store(key, value) {
    if (typeof key !== "string" || key.length === 0) {
      throw new TypeError("store key must be a non-empty string");
    }
    storedValues[key] = cloneSerializable(value, "stored value " + key);
  });
  define("load", function load(key) {
    return cloneSerializable(storedValues[String(key)], "stored value " + String(key));
  });
  define("yield_control", function yield_control() {
    bridge.yield(JSON.stringify(storedValues));
  });
  define("exit", function exit() {
    throw new Error(exitSentinel);
  });
  define("setTimeout", function ccccSetTimeout(callback, ms, ...args) {
    if (typeof callback !== "function") {
      throw new TypeError("setTimeout callback must be a function");
    }
    return bridge.setTimeout(callback, Number(ms) || 0, ...args);
  });
  define("clearTimeout", function ccccClearTimeout(id) {
    return bridge.clearTimeout(id);
  });
  define("__cccc_export_stored_values__", function __cccc_export_stored_values__() {
    return JSON.stringify(storedValues);
  });
})();
`;
  new vm.Script(bootstrap, { filename: "cccc_code_exec_bootstrap.mjs" }).runInContext(context, { timeout: 2000 });
  return context;
}

function exportStoredValues(context) {
  if (!context) return storedValues;
  try {
    const raw = vm.runInContext("__cccc_export_stored_values__()", context, { timeout: 1000 });
    return parseJsonObject(raw, {});
  } catch (_err) {
    return {};
  }
}

async function startCell(command) {
  if (started) {
    finish("cell already started");
    return;
  }
  started = true;
  storedValues = command.stored_values && typeof command.stored_values === "object" ? command.stored_values : {};
  let context = null;
  const source = String(command.source || "");
  send({ type: "started" });
  try {
    context = buildContext(
      Array.isArray(command.tools) ? command.tools : [],
      storedValues,
      Array.isArray(command.work_loops) ? command.work_loops : [],
      command.help_aliases && typeof command.help_aliases === "object" ? command.help_aliases : {},
      command.help_compact_notes && typeof command.help_compact_notes === "object" ? command.help_compact_notes : {},
      command.help_curated_tools && typeof command.help_curated_tools === "object" ? command.help_curated_tools : {},
      command.help_curated_loops && typeof command.help_curated_loops === "object" ? command.help_curated_loops : {}
    );
    const script = new vm.Script(`(async () => {\n${source}\n})()`, {
      filename: "cccc_code_exec.mjs",
    });
    await script.runInContext(context, { timeout: 2000 });
    finish("", exportStoredValues(context));
  } catch (err) {
    const message = err && err.message === EXIT_SENTINEL ? "" : (err && (err.stack || err.message)) || String(err);
    finish(message, exportStoredValues(context));
  }
}

function resolveToolResponse(command) {
  const id = String(command.id || "");
  const entry = pending.get(id);
  if (!entry) return;
  pending.delete(id);
  if (command.ok) {
    entry.resolveJson(jsonString(command.result));
  } else {
    entry.rejectMessage(String(command.error || "tool call failed"));
  }
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => {
  let command = null;
  try {
    command = JSON.parse(line);
  } catch (err) {
    finish(`invalid runtime command: ${err.message}`);
    return;
  }
  if (!command || typeof command !== "object") return;
  if (command.type === "start") {
    startCell(command);
  } else if (command.type === "tool_response") {
    resolveToolResponse(command);
  } else if (command.type === "terminate") {
    process.exit(0);
  }
});
"""
