from __future__ import annotations

import json
import logging
import os
import queue
import shutil
import subprocess
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional

from ..kernel.actors import find_actor
from ..kernel.blobs import resolve_blob_attachment_path
from ..kernel.headless_events import append_headless_event
from ..kernel.group import load_group
from ..kernel.system_prompt import render_system_prompt
from ..paths import ensure_home
from .actors.actor_exit_ops import persist_actor_process_exit_stopped
from .mcp_install import ensure_mcp_installed
from .messaging.delivery import auto_mark_headless_delivery_started, render_headless_control_text
from .runner_state_ops import headless_state_path, remove_headless_state
from ..util.fs import atomic_write_json
from ..util.node_env import with_node_deprecation_warnings_suppressed
from ..util.process import pid_is_alive
from ..util.time import utc_now_iso
from .voice_secretary_control_turns import (
    control_completion_state as _shared_voice_secretary_control_completion_state,
    control_consumed_input as _voice_secretary_control_consumed_input,
    control_consumption_diagnostics as _voice_secretary_control_consumption_diagnostics,
    control_failure_reason as _voice_secretary_control_failure_reason,
    control_snapshot as _voice_secretary_control_snapshot,
    prepare_control_turn as _shared_voice_secretary_prepare_control_turn,
    prepare_repair_retry as _voice_secretary_prepare_repair_retry,
    retryable_control_failure as _voice_secretary_retryable_control_failure,
)

logger = logging.getLogger(__name__)

_TURN_STALL_SECONDS = 45.0
_TURN_WAIT_POLL_SECONDS = 5.0


def _is_missing_codex_cli_error(exc: BaseException) -> bool:
    if isinstance(exc, FileNotFoundError):
        filename = str(getattr(exc, "filename", "") or "").strip().lower()
        if not filename or Path(filename).name == "codex":
            return True
    message = str(exc or "").strip().lower()
    return "no such file or directory" in message and "codex" in message


def _codex_cli_available(env: Dict[str, str]) -> bool:
    return bool(shutil.which("codex", path=str((env or {}).get("PATH") or os.environ.get("PATH") or "")))


def _is_closed_stream_logging_error(exc: BaseException) -> bool:
    if not isinstance(exc, ValueError):
        return False
    message = str(exc or "").strip().lower()
    return "i/o operation on closed file" in message or "closed stream" in message


def _is_codex_request_timeout(exc: BaseException, *, method: str = "") -> bool:
    message = str(exc or "").strip().lower()
    if "codex request timed out:" not in message:
        return False
    target = str(method or "").strip().lower()
    return not target or message.endswith(target)


def _safe_logger_call(method: str, message: str, *args: Any, **kwargs: Any) -> None:
    log_method = getattr(logger, method, None)
    if not callable(log_method):
        return
    try:
        log_method(message, *args, **kwargs)
    except Exception as exc:
        if _is_closed_stream_logging_error(exc):
            return
        raise


def _jsonrpc_request(request_id: int, method: str, params: Dict[str, Any]) -> Dict[str, Any]:
    return {"jsonrpc": "2.0", "id": int(request_id), "method": method, "params": params}


def _voice_secretary_prepare_control_turn(
    *,
    group_id: str,
    actor_id: str,
    text: str,
    event_id: str,
    control_kind: str,
) -> tuple[str, Dict[str, Any]]:
    return _shared_voice_secretary_prepare_control_turn(
        group_id=group_id,
        actor_id=actor_id,
        text=text,
        event_id=event_id,
        control_kind=control_kind,
        snapshot_fn=_voice_secretary_control_snapshot,
    )


def _voice_secretary_control_completion_state(*, group_id: str, snapshot: Dict[str, Any]) -> tuple[bool, Dict[str, Any]]:
    return _shared_voice_secretary_control_completion_state(
        group_id=group_id,
        snapshot=snapshot,
        diagnostics_fn=_voice_secretary_control_consumption_diagnostics,
    )


@dataclass
class _PendingTurn:
    text: str
    event_id: str
    ts: str = ""
    reply_to: Optional[str] = None
    control_kind: str = ""
    attachments: list[dict[str, Any]] = field(default_factory=list)
    retry_count: int = 0
    validation_snapshot: Dict[str, Any] = field(default_factory=dict)


@dataclass
class CodexSessionState:
    status: str = "idle"
    current_task_id: Optional[str] = None
    updated_at: str = field(default_factory=utc_now_iso)
    thread_id: Optional[str] = None

    def to_headless_state(self, *, group_id: str, actor_id: str) -> Dict[str, Any]:
        return {
            "group_id": group_id,
            "actor_id": actor_id,
            "status": self.status,
            "current_task_id": self.current_task_id,
            "updated_at": self.updated_at,
        }


class CodexAppSession:
    def __init__(self, *, group_id: str, actor_id: str, cwd: Path, env: Dict[str, str], model: str = "") -> None:
        self.group_id = str(group_id or "").strip()
        self.actor_id = str(actor_id or "").strip()
        self.cwd = cwd
        self.env = dict(env or {})
        self.model = str(model or "").strip()
        self._proc: Optional[subprocess.Popen[str]] = None
        self._lock = threading.Lock()
        self._pending: Dict[int, "queue.Queue[Dict[str, Any]]"] = {}
        self._next_request_id = 1
        self._running = False
        self._stop_requested = False
        self._session_state = CodexSessionState(status="idle")
        self._turn_queue: "queue.Queue[Optional[_PendingTurn]]" = queue.Queue()
        self._turn_done = threading.Event()
        self._active_turn_id = ""
        self._active_event_id = ""
        self._stdout_thread: Optional[threading.Thread] = None
        self._stderr_thread: Optional[threading.Thread] = None
        self._turn_thread: Optional[threading.Thread] = None
        self._plan_activity_id = ""
        self._agent_message_phase_by_stream_id: Dict[str, str] = {}
        self._item_snapshots_by_id: Dict[str, Dict[str, Any]] = {}
        self._active_control_kind = ""
        self._active_payload: Optional[_PendingTurn] = None
        self._last_turn_event_monotonic = 0.0
        self._active_stalled_emitted = False

    def _agent_message_phase(self, item_id: str, item: Optional[Dict[str, Any]] = None) -> str:
        stream_id = str(item_id or "").strip()
        if not stream_id:
            return ""
        if isinstance(item, dict):
            phase = str(item.get("phase") or "").strip().lower()
            if phase:
                self._agent_message_phase_by_stream_id[stream_id] = phase
                return phase
        return str(self._agent_message_phase_by_stream_id.get(stream_id) or "").strip().lower()

    def _persist_state(self) -> None:
        with self._lock:
            proc = self._proc
            running = bool(self._running and proc is not None and proc.poll() is None)
            state = self._session_state.to_headless_state(group_id=self.group_id, actor_id=self.actor_id)
            pid = int(proc.pid) if running and proc is not None else 0
        if not running or pid <= 0 or not pid_is_alive(pid):
            remove_headless_state(self.group_id, self.actor_id)
            return
        atomic_write_json(
            headless_state_path(self.group_id, self.actor_id),
            {
                "v": 1,
                "kind": "headless",
                "runtime": "codex",
                "pid": pid,
                **state,
            },
        )

    def _emit_activity(
        self,
        *,
        status: str,
        activity_id: str,
        kind: str,
        summary: str,
        turn_id: str = "",
        stream_id: str = "",
        item_id: str = "",
        detail: Optional[str] = None,
        raw_item_type: str = "",
        tool_name: str = "",
        server_name: str = "",
        command: str = "",
        cwd: str = "",
        file_paths: Optional[list[str]] = None,
        query: str = "",
    ) -> None:
        normalized_status = str(status or "").strip()
        normalized_activity_id = str(activity_id or "").strip()
        normalized_kind = str(kind or "").strip()
        normalized_summary = str(summary or "").strip()
        if not normalized_status or not normalized_activity_id or not normalized_kind or not normalized_summary:
            return
        with self._lock:
            active_event_id = str(self._active_event_id or "").strip()
        self._emit(
            f"headless.activity.{normalized_status}",
            {
                "activity_id": normalized_activity_id,
                "kind": normalized_kind,
                "summary": normalized_summary,
                "detail": str(detail or "").strip() or None,
                "turn_id": str(turn_id or "").strip(),
                "stream_id": str(stream_id or "").strip(),
                "item_id": str(item_id or "").strip(),
                "event_id": active_event_id,
                "raw_item_type": str(raw_item_type or "").strip() or None,
                "tool_name": str(tool_name or "").strip() or None,
                "server_name": str(server_name or "").strip() or None,
                "command": str(command or "").strip() or None,
                "cwd": str(cwd or "").strip() or None,
                "file_paths": [str(path).strip() for path in (file_paths or []) if str(path).strip()] or None,
                "query": str(query or "").strip() or None,
            },
        )

    @staticmethod
    def _trim_single_line(value: Any, *, limit: int = 120) -> str:
        text = " ".join(str(value or "").split())
        if len(text) <= limit:
            return text
        return text[: max(0, limit - 1)].rstrip() + "…"

    def _remember_item_snapshot(self, item: Dict[str, Any]) -> Dict[str, Any]:
        item_id = str(item.get("id") or "").strip()
        if not item_id:
            return item
        snapshot = dict(self._item_snapshots_by_id.get(item_id) or {})
        merged = {**snapshot, **item}
        for key in ("type", "phase", "command", "cwd", "server", "tool", "query", "text", "aggregatedOutput"):
            if key in snapshot and not str(merged.get(key) or "").strip():
                merged[key] = snapshot[key]
        if not isinstance(merged.get("changes"), list) and isinstance(snapshot.get("changes"), list):
            merged["changes"] = snapshot["changes"]
        self._item_snapshots_by_id[item_id] = merged
        return merged

    def _item_snapshot(self, item_id: str) -> Dict[str, Any]:
        snapshot = self._item_snapshots_by_id.get(str(item_id or "").strip())
        return snapshot if isinstance(snapshot, dict) else {}

    def _snapshot_file_paths(self, item: Dict[str, Any]) -> list[str]:
        changes = item.get("changes") if isinstance(item.get("changes"), list) else []
        targets: list[str] = []
        for change in changes[:3]:
            if not isinstance(change, dict):
                continue
            path = self._trim_single_line(change.get("path") or change.get("filePath") or "", limit=80)
            if path:
                targets.append(path)
        return targets

    def _emit_item_activity(self, *, status: str, turn_id: str, item: Dict[str, Any]) -> None:
        item_type = str(item.get("type") or "").strip()
        item_id = str(item.get("id") or "").strip()
        if not item_type or not item_id:
            return

        if item_type == "agentMessage":
            phase = str(item.get("phase") or "").strip().lower()
            if phase and phase != "final_answer":
                return
            summary = "replying" if status != "completed" else "reply ready"
            self._emit_activity(
                status=status,
                activity_id=f"reply:{item_id}",
                kind="reply",
                summary=summary,
                turn_id=turn_id,
                stream_id=item_id,
                item_id=item_id,
            )
            return

        if item_type == "reasoning":
            summary = "thinking" if status != "completed" else "thinking done"
            self._emit_activity(
                status=status,
                activity_id=f"reasoning:{item_id}",
                kind="thinking",
                summary=summary,
                turn_id=turn_id,
                item_id=item_id,
            )
            return

        if item_type == "commandExecution":
            command = self._trim_single_line(item.get("command") or "command")
            detail = self._trim_single_line(item.get("aggregatedOutput") or "", limit=160)
            self._emit_activity(
                status=status,
                activity_id=f"command:{item_id}",
                kind="command",
                summary=command or "command",
                detail=detail or None,
                turn_id=turn_id,
                item_id=item_id,
                raw_item_type=item_type,
                command=command,
                cwd=self._trim_single_line(item.get("cwd") or "", limit=120),
            )
            return

        if item_type == "fileChange":
            targets = self._snapshot_file_paths(item)
            summary = f"patch {', '.join(targets)}" if targets else "patch files"
            self._emit_activity(
                status=status,
                activity_id=f"file:{item_id}",
                kind="patch",
                summary=summary,
                turn_id=turn_id,
                item_id=item_id,
                raw_item_type=item_type,
                file_paths=targets,
            )
            return

        if item_type == "mcpToolCall":
            server = self._trim_single_line(item.get("server") or "", limit=40)
            tool = self._trim_single_line(item.get("tool") or "tool", limit=60)
            summary = f"{server}:{tool}" if server else tool
            self._emit_activity(
                status=status,
                activity_id=f"mcp:{item_id}",
                kind="tool",
                summary=summary,
                turn_id=turn_id,
                item_id=item_id,
                raw_item_type=item_type,
                tool_name=tool,
                server_name=server,
            )
            return

        if item_type == "dynamicToolCall":
            tool = self._trim_single_line(item.get("tool") or "tool", limit=80)
            self._emit_activity(
                status=status,
                activity_id=f"tool:{item_id}",
                kind="tool",
                summary=tool,
                turn_id=turn_id,
                item_id=item_id,
                raw_item_type=item_type,
                tool_name=tool,
            )
            return

        if item_type == "webSearch":
            query = self._trim_single_line(item.get("query") or "web search", limit=100)
            self._emit_activity(
                status=status,
                activity_id=f"search:{item_id}",
                kind="search",
                summary=query,
                turn_id=turn_id,
                item_id=item_id,
                raw_item_type=item_type,
                query=query,
            )
            return

        if item_type == "plan":
            text = self._trim_single_line(item.get("text") or "plan updated", limit=100)
            self._emit_activity(
                status=status,
                activity_id=f"plan:{item_id}",
                kind="plan",
                summary=text,
                turn_id=turn_id,
                item_id=item_id,
            )
            return

    def start(self) -> None:
        with self._lock:
            if self._running:
                return
            self._stop_requested = False
            env = os.environ.copy()
            env.update(self.env)
            env.setdefault("CCCC_HOME", str(ensure_home()))
            env.setdefault("CCCC_GROUP_ID", self.group_id)
            env.setdefault("CCCC_ACTOR_ID", self.actor_id)
            env = with_node_deprecation_warnings_suppressed(env)
            if not ensure_mcp_installed("codex", self.cwd, auto_mcp_runtimes=("codex",), env=env):
                raise RuntimeError("failed to install MCP for runtime: codex")
            self._proc = subprocess.Popen(
                ["codex", "app-server", "--listen", "stdio://"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                cwd=str(self.cwd),
                env=env,
                text=True,
                bufsize=1,
            )
            self._running = True

        self._stdout_thread = threading.Thread(target=self._stdout_loop, name=f"cccc-codex-out:{self.group_id}:{self.actor_id}", daemon=True)
        self._stderr_thread = threading.Thread(target=self._stderr_loop, name=f"cccc-codex-err:{self.group_id}:{self.actor_id}", daemon=True)
        self._turn_thread = threading.Thread(target=self._turn_loop, name=f"cccc-codex-turn:{self.group_id}:{self.actor_id}", daemon=True)
        self._stdout_thread.start()
        self._stderr_thread.start()
        try:
            self._request(
                "initialize",
                {
                    "clientInfo": {"name": "cccc", "version": "1.0"},
                    "capabilities": {"experimentalApi": True},
                },
                timeout=10.0,
            )
            thread_params: Dict[str, Any] = {
                "cwd": str(self.cwd),
                "approvalPolicy": "never",
                "sandbox": "danger-full-access",
                "personality": "pragmatic",
            }
            if self.model:
                thread_params["model"] = self.model
            thread_resp = self._request(
                "thread/start",
                thread_params,
                timeout=20.0,
            )
            thread = thread_resp.get("thread") if isinstance(thread_resp, dict) else {}
            thread_id = str((thread or {}).get("id") or "").strip()
            if not thread_id:
                raise RuntimeError("codex app-server returned empty thread id")
            with self._lock:
                self._session_state.thread_id = thread_id
                self._session_state.status = "idle"
                self._session_state.updated_at = utc_now_iso()
            self._persist_state()
            self._emit("headless.thread.started", {"thread_id": thread_id})
            self._queue_bootstrap_control_turn()
            self._turn_thread.start()
        except Exception:
            self.stop()
            raise

    def stop(self, *, persist_actor_stopped: bool = False) -> None:
        with self._lock:
            proc = self._proc
            self._stop_requested = True
            self._running = False
            self._proc = None
            self._session_state.status = "stopped"
            self._session_state.current_task_id = None
            self._session_state.updated_at = utc_now_iso()
            self._active_control_kind = ""
            self._active_event_id = ""
            self._active_turn_id = ""
        self._persist_state()
        self._turn_done.set()
        try:
            self._turn_queue.put_nowait(None)
        except Exception:
            pass
        if proc is not None:
            try:
                proc.terminate()
            except Exception:
                pass
            try:
                proc.wait(timeout=2.0)
            except Exception:
                try:
                    proc.kill()
                except Exception:
                    pass
        self._emit("headless.session.stopped", {})
        if persist_actor_stopped:
            persist_actor_process_exit_stopped(group_id=self.group_id, actor_id=self.actor_id, runner="headless")

    def is_running(self) -> bool:
        with self._lock:
            proc = self._proc
            return bool(self._running and proc is not None and proc.poll() is None)

    def state(self) -> Dict[str, Any]:
        with self._lock:
            return self._session_state.to_headless_state(group_id=self.group_id, actor_id=self.actor_id)

    def _control_turn_kind(self) -> str:
        with self._lock:
            return str(self._active_control_kind or "").strip().lower()

    def _build_bootstrap_control_text(self) -> str:
        group = load_group(self.group_id)
        if group is None:
            return ""
        actor = find_actor(group, self.actor_id)
        if not isinstance(actor, dict):
            return ""
        prompt = render_system_prompt(group=group, actor=actor)
        if not prompt.strip():
            return ""
        return render_headless_control_text(control_kind="bootstrap", body=prompt)

    def _queue_control_turn(self, *, text: str, control_kind: str, event_id: str = "", ts: str = "") -> bool:
        if not self.is_running():
            return False
        normalized_control_kind = str(control_kind or "").strip().lower()
        normalized_event_id = str(event_id or "").strip()
        payload_text, validation_snapshot = _voice_secretary_prepare_control_turn(
            group_id=self.group_id,
            actor_id=self.actor_id,
            text=text,
            event_id=normalized_event_id,
            control_kind=normalized_control_kind,
        )
        payload = _PendingTurn(
            text=payload_text,
            event_id=normalized_event_id,
            ts=str(ts or "").strip(),
            control_kind=normalized_control_kind,
            validation_snapshot=validation_snapshot,
        )
        if not payload.text.strip() or not payload.control_kind:
            return False
        try:
            self._turn_queue.put_nowait(payload)
            self._emit(
                "headless.control.queued",
                {
                    "control_kind": payload.control_kind,
                    "event_id": payload.event_id,
                },
            )
            return True
        except Exception:
            return False

    def _queue_bootstrap_control_turn(self) -> bool:
        return self._queue_control_turn(
            text=self._build_bootstrap_control_text(),
            control_kind="bootstrap",
        )

    def _thread_id(self) -> str:
        with self._lock:
            return str(self._session_state.thread_id or "").strip()

    def submit_user_message(
        self,
        *,
        text: str,
        event_id: str,
        ts: str = "",
        reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
    ) -> bool:
        if not self.is_running():
            return False
        payload = _PendingTurn(
            text=str(text or ""),
            event_id=str(event_id or "").strip(),
            ts=str(ts or "").strip(),
            reply_to=reply_to,
            attachments=[item for item in (attachments or []) if isinstance(item, dict)],
        )
        try:
            self._turn_queue.put_nowait(payload)
            self._emit(
                "headless.turn.queued",
                {
                    "event_id": payload.event_id,
                    "reply_to": payload.reply_to,
                },
            )
            return True
        except Exception:
            return False

    def submit_control_message(
        self,
        *,
        text: str,
        control_kind: str,
        event_id: str = "",
        ts: str = "",
    ) -> bool:
        return self._queue_control_turn(
            text=text,
            control_kind=control_kind,
            event_id=event_id,
            ts=ts,
        )

    def _request(self, method: str, params: Dict[str, Any], *, timeout: float) -> Dict[str, Any]:
        with self._lock:
            if not self._running or self._proc is None or self._proc.stdin is None:
                raise RuntimeError("codex session is not running")
            request_id = self._next_request_id
            self._next_request_id += 1
            result_q: "queue.Queue[Dict[str, Any]]" = queue.Queue(maxsize=1)
            self._pending[request_id] = result_q
            message = json.dumps(_jsonrpc_request(request_id, method, params), ensure_ascii=False)
            self._proc.stdin.write(message + "\n")
            self._proc.stdin.flush()
        try:
            response = result_q.get(timeout=timeout)
        except queue.Empty as exc:
            with self._lock:
                self._pending.pop(request_id, None)
            raise RuntimeError(f"codex request timed out: {method}") from exc
        if "error" in response:
            raise RuntimeError(str((response.get("error") or {}).get("message") or f"codex request failed: {method}"))
        result = response.get("result")
        return result if isinstance(result, dict) else {}

    def _stdout_loop(self) -> None:
        proc = self._proc
        if proc is None or proc.stdout is None:
            return
        try:
            for raw_line in proc.stdout:
                line = str(raw_line or "").strip()
                if not line:
                    continue
                try:
                    message = json.loads(line)
                except Exception:
                    logger.debug("ignore non-json codex output: %s", line[:200])
                    continue
                if "id" in message:
                    request_id = int(message.get("id") or 0)
                    with self._lock:
                        result_q = self._pending.pop(request_id, None)
                    if result_q is not None:
                        result_q.put_nowait(message)
                    continue
                method = str(message.get("method") or "").strip()
                params = message.get("params")
                if method:
                    self._handle_notification(method, params if isinstance(params, dict) else {})
        except Exception:
            logger.exception("codex stdout loop failed: %s/%s", self.group_id, self.actor_id)
        finally:
            with self._lock:
                persist_actor_stopped = not self._stop_requested
            self.stop(persist_actor_stopped=persist_actor_stopped)

    def _stderr_loop(self) -> None:
        proc = self._proc
        if proc is None or proc.stderr is None:
            return
        try:
            for raw_line in proc.stderr:
                line = str(raw_line or "").rstrip()
                if line:
                    _safe_logger_call("info", "[codex-app %s/%s] %s", self.group_id, self.actor_id, line)
        except Exception as exc:
            if _is_closed_stream_logging_error(exc):
                return
            _safe_logger_call("exception", "codex stderr loop failed: %s/%s", self.group_id, self.actor_id)

    def _build_turn_input_items(self, payload: _PendingTurn, *, text_override: Optional[str] = None) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        text = str(payload.text if text_override is None else text_override or "")
        if text.strip():
            items.append({"type": "text", "text": text})

        group = load_group(self.group_id)
        if group is not None:
            for attachment in payload.attachments:
                if str(attachment.get("kind") or "").strip().lower() != "image":
                    continue
                rel_path = str(attachment.get("path") or "").strip()
                if not rel_path:
                    continue
                try:
                    abs_path = resolve_blob_attachment_path(group, rel_path=rel_path)
                except Exception:
                    continue
                if not abs_path.exists() or not abs_path.is_file():
                    continue
                items.append({"type": "local_image", "path": str(abs_path)})

        if not items:
            items.append({"type": "text", "text": text})
        return items

    def _maybe_emit_turn_stalled(self, *, payload: _PendingTurn, turn_id: str) -> None:
        with self._lock:
            if self._active_stalled_emitted:
                return
            last_event = float(self._last_turn_event_monotonic or 0.0)
            elapsed = time.monotonic() - last_event if last_event > 0 else 0.0
            if elapsed < _TURN_STALL_SECONDS:
                return
            self._active_stalled_emitted = True
            self._session_state.status = "waiting"
            self._session_state.updated_at = utc_now_iso()
        self._persist_state()
        event_type = "headless.control.stalled" if payload.control_kind else "headless.turn.stalled"
        self._emit(
            event_type,
            {
                "turn_id": turn_id,
                "event_id": payload.event_id,
                "control_kind": payload.control_kind or None,
                "seconds_since_last_event": round(elapsed, 3),
                "threshold_seconds": _TURN_STALL_SECONDS,
                "reason": "runtime_no_events_after_turn_started",
            },
        )

    def _turn_loop(self) -> None:
        while self.is_running():
            try:
                payload = self._turn_queue.get(timeout=0.5)
            except queue.Empty:
                continue
            if payload is None:
                return
            thread_id = self._thread_id()
            if not thread_id:
                continue
            self._turn_done.clear()
            turn_id = ""
            with self._lock:
                self._active_control_kind = str(payload.control_kind or "").strip().lower()
                self._active_payload = payload
            turn_text = str(payload.text or "")

            def _handle_turn_start_failed(exc_obj: BaseException) -> None:
                timed_out = _is_codex_request_timeout(exc_obj, method="turn/start")
                logger.warning("codex turn start failed: group=%s actor=%s err=%s", self.group_id, self.actor_id, exc_obj)
                if not timed_out:
                    with self._lock:
                        self._session_state.status = "idle"
                        self._active_event_id = ""
                        self._active_control_kind = ""
                        self._active_payload = None
                        self._session_state.current_task_id = None
                        self._session_state.updated_at = utc_now_iso()
                    self._persist_state()
                self._emit(
                    "headless.control.failed" if payload.control_kind else "headless.turn.failed",
                    {
                        "turn_id": turn_id,
                        "event_id": payload.event_id,
                        "control_kind": payload.control_kind or None,
                        "error": str(exc_obj),
                    },
                )
                if timed_out:
                    self.stop()

            try:
                input_items = self._build_turn_input_items(payload, text_override=turn_text)
                response = self._request(
                    "turn/start",
                    {
                        "threadId": thread_id,
                        "input": input_items,
                    },
                    timeout=30.0,
                )
            except Exception as exc:
                has_local_image = any(str(item.get("type") or "").strip() == "local_image" for item in locals().get("input_items", []))
                if has_local_image:
                    try:
                        response = self._request(
                            "turn/start",
                            {
                                "threadId": thread_id,
                                "input": [{"type": "text", "text": turn_text}],
                            },
                            timeout=30.0,
                        )
                    except Exception as retry_exc:
                        _handle_turn_start_failed(retry_exc)
                        continue
                else:
                    _handle_turn_start_failed(exc)
                    continue
            try:
                turn = response.get("turn") if isinstance(response, dict) else {}
                turn_id = str((turn or {}).get("id") or "").strip()
                with self._lock:
                    self._active_turn_id = turn_id
                    self._active_event_id = payload.event_id
                    self._session_state.status = "working"
                    self._session_state.current_task_id = turn_id or payload.event_id or None
                    self._session_state.updated_at = utc_now_iso()
                    self._last_turn_event_monotonic = time.monotonic()
                    self._active_stalled_emitted = False
                self._persist_state()
                if payload.control_kind:
                    self._emit(
                        "headless.control.started",
                        {
                            "turn_id": turn_id,
                            "event_id": payload.event_id,
                            "control_kind": payload.control_kind,
                        },
                    )
                else:
                    auto_mark_headless_delivery_started(
                        group_id=self.group_id,
                        actor_id=self.actor_id,
                        event_id=payload.event_id,
                        ts=payload.ts,
                    )
                    self._emit(
                        "headless.turn.started",
                        {
                            "turn_id": turn_id,
                            "event_id": payload.event_id,
                            "reply_to": payload.reply_to,
                        },
                    )
            except Exception as exc:
                logger.warning("codex turn start failed: group=%s actor=%s err=%s", self.group_id, self.actor_id, exc)
                with self._lock:
                    self._session_state.status = "idle"
                    self._active_event_id = ""
                    self._active_control_kind = ""
                    self._active_payload = None
                    self._session_state.current_task_id = None
                    self._session_state.updated_at = utc_now_iso()
                self._persist_state()
                self._emit(
                    "headless.control.failed" if payload.control_kind else "headless.turn.failed",
                    {
                        "turn_id": turn_id,
                        "event_id": payload.event_id,
                        "control_kind": payload.control_kind or None,
                        "error": str(exc),
                    },
                )
                continue
            while self.is_running():
                if self._turn_done.wait(timeout=_TURN_WAIT_POLL_SECONDS):
                    break
                self._maybe_emit_turn_stalled(payload=payload, turn_id=turn_id)

    def _handle_notification(self, method: str, params: Dict[str, Any]) -> None:
        now = utc_now_iso()
        with self._lock:
            active_event_id = str(self._active_event_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()
            if self._active_turn_id or active_event_id:
                self._last_turn_event_monotonic = time.monotonic()
                if self._active_stalled_emitted and self._session_state.status == "waiting":
                    self._session_state.status = "working"
                    self._session_state.updated_at = now
        if method == "turn/started":
            turn = params.get("turn") if isinstance(params.get("turn"), dict) else {}
            turn_id = str(turn.get("id") or "").strip()
            with self._lock:
                self._active_turn_id = turn_id
                if not control_kind:
                    self._session_state.status = "working"
                self._session_state.current_task_id = turn_id or None
                self._session_state.updated_at = now
            self._persist_state()
            self._item_snapshots_by_id.clear()
            self._plan_activity_id = ""
            if control_kind:
                return
            self._emit("headless.turn.progress", {"turn_id": turn_id, "event_id": active_event_id, "status": "working"})
            self._emit_activity(
                status="started",
                activity_id=f"turn:{turn_id or active_event_id or 'current'}",
                kind="thinking",
                summary="thinking",
                turn_id=turn_id,
            )
            return

        if method == "turn/completed" and control_kind:
            turn = params.get("turn") if isinstance(params.get("turn"), dict) else {}
            turn_id = str(turn.get("id") or "").strip()
            status = str(turn.get("status") or "completed").strip() or "completed"
            raw_error = turn.get("error")
            error = raw_error if isinstance(raw_error, dict) else ({"message": str(raw_error)} if raw_error else None)
            with self._lock:
                active_payload = self._active_payload
            should_complete, consumption_diagnostics = _voice_secretary_control_completion_state(
                group_id=self.group_id,
                snapshot=(active_payload.validation_snapshot if isinstance(active_payload, _PendingTurn) else {}),
            )
            with self._lock:
                self._active_turn_id = ""
                self._active_event_id = ""
                self._active_control_kind = ""
                self._active_payload = None
                self._session_state.status = "idle"
                self._session_state.current_task_id = None
                self._session_state.updated_at = now
            self._persist_state()
            self._agent_message_phase_by_stream_id.clear()
            self._item_snapshots_by_id.clear()
            self._plan_activity_id = ""
            if not should_complete:
                retry_count = int(active_payload.retry_count or 0) if isinstance(active_payload, _PendingTurn) else 0
                should_retry = _voice_secretary_retryable_control_failure(consumption_diagnostics)
                failure_reason = _voice_secretary_control_failure_reason(consumption_diagnostics)
                if isinstance(active_payload, _PendingTurn) and retry_count < 1 and should_retry:
                    retry_text, retry_diagnostics = _voice_secretary_prepare_repair_retry(
                        text=active_payload.text,
                        diagnostics=consumption_diagnostics,
                    )
                    retry_reason = _voice_secretary_control_failure_reason(retry_diagnostics)
                    retry_payload = _PendingTurn(
                        text=retry_text,
                        event_id=active_payload.event_id,
                        ts=active_payload.ts,
                        reply_to=active_payload.reply_to,
                        control_kind=active_payload.control_kind,
                        attachments=list(active_payload.attachments),
                        retry_count=retry_count + 1,
                        validation_snapshot=active_payload.validation_snapshot,
                    )
                    try:
                        self._turn_queue.put_nowait(retry_payload)
                        self._emit(
                            "headless.control.requeued",
                            {
                                "turn_id": turn_id,
                                "event_id": active_event_id,
                                "control_kind": control_kind,
                                "status": "requeued",
                                "reason": retry_reason,
                                "retry_count": retry_payload.retry_count,
                                "diagnostics": retry_diagnostics,
                            },
                        )
                    except Exception as exc:
                        self._emit(
                            "headless.control.failed",
                            {
                                "turn_id": turn_id,
                                "event_id": active_event_id,
                                "control_kind": control_kind,
                                "status": "failed",
                                "error": {
                                    "message": f"{retry_reason}; requeue failed: {exc}",
                                },
                                "diagnostics": consumption_diagnostics,
                            },
                        )
                    self._turn_done.set()
                    return
                self._emit(
                    "headless.control.failed",
                    {
                        "turn_id": turn_id,
                        "event_id": active_event_id,
                        "control_kind": control_kind,
                        "status": "failed",
                        "error": error or {"message": failure_reason},
                        "diagnostics": consumption_diagnostics,
                    },
                )
                self._turn_done.set()
                return
            if status.lower() in {"failed", "error", "cancelled"} or error:
                self._emit(
                    "headless.control.failed",
                    {
                        "turn_id": turn_id,
                        "event_id": active_event_id,
                        "control_kind": control_kind,
                        "status": status,
                        "error": error or {"message": status},
                    },
                )
                self._turn_done.set()
                return
            self._emit(
                "headless.control.completed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "control_kind": control_kind,
                    "status": status,
                    "error": error,
                },
            )
            self._turn_done.set()
            return

        if method == "turn/plan/updated":
            turn_id = str(params.get("turnId") or "").strip()
            steps = params.get("plan") if isinstance(params.get("plan"), list) else []
            explanation = self._trim_single_line(params.get("explanation") or "", limit=100)
            current = ""
            for step in steps:
                if not isinstance(step, dict):
                    continue
                step_text = self._trim_single_line(step.get("step") or "", limit=100)
                if not step_text:
                    continue
                if str(step.get("status") or "").strip() == "in_progress":
                    current = step_text
                    break
                if not current:
                    current = step_text
            summary = current or explanation or "plan updated"
            activity_id = self._plan_activity_id or f"plan:{turn_id or active_event_id or 'current'}"
            self._plan_activity_id = activity_id
            self._emit_activity(
                status="updated",
                activity_id=activity_id,
                kind="plan",
                summary=summary,
                detail=explanation or None,
                turn_id=turn_id,
            )
            return

        if method == "item/started":
            item = params.get("item") if isinstance(params.get("item"), dict) else {}
            item = self._remember_item_snapshot(item)
            item_type = str(item.get("type") or "").strip()
            item_id = str(item.get("id") or "").strip()
            if item_type == "agentMessage" and item_id:
                phase = self._agent_message_phase(item_id, item)
                payload = {
                    "turn_id": str(params.get("turnId") or ""),
                    "event_id": active_event_id,
                    "stream_id": item_id,
                    "item": item,
                }
                if phase:
                    payload["phase"] = phase
                self._emit("headless.message.started", payload)
            else:
                self._emit("headless.item.started", {"turn_id": str(params.get("turnId") or ""), "event_id": active_event_id, "stream_id": item_id, "item": item})
            self._emit_item_activity(status="started", turn_id=str(params.get("turnId") or ""), item=item)
            return

        if method == "item/agentMessage/delta":
            stream_id = str(params.get("itemId") or "").strip()
            delta = str(params.get("delta") or "")
            if not stream_id or not delta:
                return
            phase = self._agent_message_phase(stream_id)
            payload = {
                "turn_id": str(params.get("turnId") or ""),
                "event_id": active_event_id,
                "stream_id": stream_id,
                "delta": delta,
            }
            if phase:
                payload["phase"] = phase
            self._emit("headless.message.delta", payload)
            return

        if method == "item/reasoning/summaryTextDelta":
            item_id = str(params.get("itemId") or "").strip()
            turn_id = str(params.get("turnId") or "").strip()
            delta = self._trim_single_line(params.get("delta") or "", limit=120)
            if item_id and delta:
                self._emit_activity(
                    status="updated",
                    activity_id=f"reasoning:{item_id}",
                    kind="thinking",
                    summary=delta,
                    turn_id=turn_id,
                    item_id=item_id,
                )
            return

        if method == "item/plan/delta":
            item_id = str(params.get("itemId") or "").strip()
            turn_id = str(params.get("turnId") or "").strip()
            delta = self._trim_single_line(params.get("delta") or "", limit=120)
            if item_id and delta:
                activity_id = self._plan_activity_id or f"plan:{item_id}"
                self._plan_activity_id = activity_id
                self._emit_activity(
                    status="updated",
                    activity_id=activity_id,
                    kind="plan",
                    summary=delta,
                    turn_id=turn_id,
                    item_id=item_id,
                    raw_item_type="plan",
                )
            return

        if method == "item/commandExecution/outputDelta":
            item_id = str(params.get("itemId") or "").strip()
            turn_id = str(params.get("turnId") or "").strip()
            delta = self._trim_single_line(params.get("delta") or "", limit=120)
            snapshot = self._item_snapshot(item_id)
            command = self._trim_single_line(snapshot.get("command") or "", limit=120)
            cwd = self._trim_single_line(snapshot.get("cwd") or "", limit=120)
            if item_id and delta:
                self._emit_activity(
                    status="updated",
                    activity_id=f"command:{item_id}",
                    kind="command",
                    summary=delta,
                    turn_id=turn_id,
                    item_id=item_id,
                    raw_item_type="commandExecution",
                    command=command,
                    cwd=cwd,
                )
            return

        if method == "item/commandExecution/terminalInteraction":
            item_id = str(params.get("itemId") or "").strip()
            turn_id = str(params.get("turnId") or "").strip()
            snapshot = self._item_snapshot(item_id)
            command = self._trim_single_line(snapshot.get("command") or "", limit=120)
            cwd = self._trim_single_line(snapshot.get("cwd") or "", limit=120)
            if item_id:
                self._emit_activity(
                    status="updated",
                    activity_id=f"command:{item_id}",
                    kind="command",
                    summary="terminal input",
                    detail="Sent terminal input to running command",
                    turn_id=turn_id,
                    item_id=item_id,
                    raw_item_type="commandExecution",
                    command=command,
                    cwd=cwd,
                )
            return

        if method == "item/fileChange/outputDelta":
            item_id = str(params.get("itemId") or "").strip()
            turn_id = str(params.get("turnId") or "").strip()
            delta = self._trim_single_line(params.get("delta") or "", limit=120)
            snapshot = self._item_snapshot(item_id)
            targets = self._snapshot_file_paths(snapshot)
            if item_id and delta:
                self._emit_activity(
                    status="updated",
                    activity_id=f"file:{item_id}",
                    kind="patch",
                    summary=delta,
                    turn_id=turn_id,
                    item_id=item_id,
                    raw_item_type="fileChange",
                    file_paths=targets,
                )
            return

        if method == "item/mcpToolCall/progress":
            item_id = str(params.get("itemId") or "").strip()
            turn_id = str(params.get("turnId") or "").strip()
            message = self._trim_single_line(params.get("message") or "", limit=120)
            snapshot = self._item_snapshot(item_id)
            server = self._trim_single_line(snapshot.get("server") or "", limit=40)
            tool = self._trim_single_line(snapshot.get("tool") or "", limit=60)
            if item_id and message:
                self._emit_activity(
                    status="updated",
                    activity_id=f"mcp:{item_id}",
                    kind="tool",
                    summary=message,
                    turn_id=turn_id,
                    item_id=item_id,
                    raw_item_type="mcpToolCall",
                    tool_name=tool,
                    server_name=server,
                )
            return

        if method == "item/reasoning/textDelta":
            item_id = str(params.get("itemId") or "").strip()
            turn_id = str(params.get("turnId") or "").strip()
            delta = self._trim_single_line(params.get("delta") or "", limit=120)
            if item_id and delta:
                self._emit_activity(
                    status="updated",
                    activity_id=f"reasoning:{item_id}",
                    kind="thinking",
                    summary=delta,
                    turn_id=turn_id,
                    item_id=item_id,
                    raw_item_type="reasoning",
                )
            return

        if method == "item/completed":
            item = params.get("item") if isinstance(params.get("item"), dict) else {}
            item = self._remember_item_snapshot(item)
            item_type = str(item.get("type") or "").strip()
            item_id = str(item.get("id") or "").strip()
            if item_type == "agentMessage" and item_id:
                phase = self._agent_message_phase(item_id, item)
                self._agent_message_phase_by_stream_id.pop(item_id, None)
                text = str(item.get("text") or "")
                payload = {
                    "turn_id": str(params.get("turnId") or ""),
                    "event_id": active_event_id,
                    "stream_id": item_id,
                    "text": text,
                }
                if phase:
                    payload["phase"] = phase
                self._emit("headless.message.completed", payload)
                self._emit_item_activity(status="completed", turn_id=str(params.get("turnId") or ""), item=item)
                return
            self._emit_item_activity(status="completed", turn_id=str(params.get("turnId") or ""), item=item)
            self._emit("headless.item.completed", {"turn_id": str(params.get("turnId") or ""), "event_id": active_event_id, "stream_id": item_id, "item": item})
            return

        if method == "turn/completed":
            turn = params.get("turn") if isinstance(params.get("turn"), dict) else {}
            turn_id = str(turn.get("id") or "").strip()
            status = str(turn.get("status") or "completed").strip() or "completed"
            raw_error = turn.get("error")
            error = raw_error if isinstance(raw_error, dict) else ({"message": str(raw_error)} if raw_error else None)
            failed = status.lower() in {"failed", "error", "cancelled"} or error is not None
            with self._lock:
                self._active_turn_id = ""
                self._active_event_id = ""
                self._session_state.status = "idle"
                self._session_state.current_task_id = None
                self._session_state.updated_at = now
            self._persist_state()
            self._agent_message_phase_by_stream_id.clear()
            self._item_snapshots_by_id.clear()
            if self._plan_activity_id:
                self._emit_activity(
                    status="completed",
                    activity_id=self._plan_activity_id,
                    kind="plan",
                    summary="plan finished",
                    turn_id=turn_id,
                )
                self._plan_activity_id = ""
            self._emit(
                "headless.turn.failed" if failed else "headless.turn.completed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "status": status,
                    "error": error,
                },
            )
            self._turn_done.set()
            return

    def _emit(self, event_type: str, data: Dict[str, Any]) -> None:
        group = load_group(self.group_id)
        if group is None:
            return
        try:
            append_headless_event(
                group.path,
                group_id=self.group_id,
                actor_id=self.actor_id,
                event_type=event_type,
                data=data,
            )
        except Exception:
            logger.exception("failed to append headless event: %s/%s %s", self.group_id, self.actor_id, event_type)


class _FallbackCodexAppSession:
    def __init__(self, *, group_id: str, actor_id: str, cwd: Path, env: Dict[str, str], model: str = "", reason: str = "") -> None:
        self.group_id = str(group_id or "").strip()
        self.actor_id = str(actor_id or "").strip()
        self.cwd = cwd
        self.env = dict(env or {})
        self.model = str(model or "").strip()
        self._reason = str(reason or "").strip() or "codex CLI is unavailable"
        self._running = False
        self._session_state = CodexSessionState(status="idle")

    def _persist_state(self) -> None:
        if not self._running:
            remove_headless_state(self.group_id, self.actor_id)
            return
        atomic_write_json(
            headless_state_path(self.group_id, self.actor_id),
            {
                "v": 1,
                "kind": "headless",
                "runtime": "codex",
                "pid": os.getpid(),
                "fallback": True,
                "reason": self._reason,
                **self._session_state.to_headless_state(group_id=self.group_id, actor_id=self.actor_id),
            },
        )

    def _emit(self, event_type: str, data: Dict[str, Any]) -> None:
        group = load_group(self.group_id)
        if group is None:
            return
        try:
            append_headless_event(
                group.path,
                group_id=self.group_id,
                actor_id=self.actor_id,
                event_type=event_type,
                data=data,
            )
        except Exception:
            logger.exception("failed to append fallback headless event: %s/%s %s", self.group_id, self.actor_id, event_type)

    def start(self) -> None:
        self._running = True
        self._session_state.status = "idle"
        self._session_state.current_task_id = None
        self._session_state.updated_at = utc_now_iso()
        self._persist_state()
        _safe_logger_call(
            "warning",
            "codex CLI unavailable; using fallback headless session for %s/%s: %s",
            self.group_id,
            self.actor_id,
            self._reason,
        )

    def stop(self) -> None:
        self._running = False
        self._session_state.status = "stopped"
        self._session_state.current_task_id = None
        self._session_state.updated_at = utc_now_iso()
        self._persist_state()

    def is_running(self) -> bool:
        return bool(self._running)

    def state(self) -> Dict[str, Any]:
        return self._session_state.to_headless_state(group_id=self.group_id, actor_id=self.actor_id)

    def submit_user_message(
        self,
        *,
        text: str,
        event_id: str,
        ts: str = "",
        reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
    ) -> bool:
        if not self._running:
            return False
        self._emit(
            "headless.turn.queued",
            {
                "event_id": str(event_id or "").strip(),
                "reply_to": reply_to,
                "runtime_unavailable": True,
                "reason": self._reason,
            },
        )
        return True

    def submit_control_message(
        self,
        *,
        text: str,
        control_kind: str,
        event_id: str = "",
        ts: str = "",
    ) -> bool:
        if not self._running:
            return False
        self._emit(
            "headless.control.queued",
            {
                "control_kind": str(control_kind or "").strip().lower(),
                "event_id": str(event_id or "").strip(),
                "runtime_unavailable": True,
                "reason": self._reason,
            },
        )
        return True


class CodexAppSessionManager:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._sessions: Dict[tuple[str, str], CodexAppSession] = {}

    def start_actor(self, *, group_id: str, actor_id: str, cwd: Path, env: Dict[str, str], model: str = "") -> CodexAppSession:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            raise ValueError("missing group_id/actor_id")
        with self._lock:
            session = self._sessions.get(key)
            if session is not None and session.is_running():
                return session
            if _codex_cli_available(env):
                session = CodexAppSession(group_id=key[0], actor_id=key[1], cwd=cwd, env=env, model=model)
            else:
                session = _FallbackCodexAppSession(
                    group_id=key[0],
                    actor_id=key[1],
                    cwd=cwd,
                    env=env,
                    model=model,
                    reason="codex CLI not found",
                )
            self._sessions[key] = session
        try:
            session.start()
        except Exception as exc:
            if _is_missing_codex_cli_error(exc):
                fallback = _FallbackCodexAppSession(
                    group_id=key[0],
                    actor_id=key[1],
                    cwd=cwd,
                    env=env,
                    model=model,
                    reason=str(exc),
                )
                fallback.start()
                with self._lock:
                    self._sessions[key] = fallback
                return fallback
            with self._lock:
                if self._sessions.get(key) is session:
                    self._sessions.pop(key, None)
            raise
        return session

    def stop_actor(self, *, group_id: str, actor_id: str) -> None:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            session = self._sessions.pop(key, None)
        if session is not None:
            session.stop()

    def stop_group(self, *, group_id: str) -> None:
        gid = str(group_id or "").strip()
        if not gid:
            return
        with self._lock:
            keys = [key for key in self._sessions if key[0] == gid]
            sessions = [self._sessions.pop(key) for key in keys]
        for session in sessions:
            session.stop()

    def stop_all(self) -> None:
        with self._lock:
            sessions = list(self._sessions.values())
            self._sessions.clear()
        for session in sessions:
            session.stop()

    def actor_running(self, group_id: str, actor_id: str) -> bool:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            session = self._sessions.get(key)
        return bool(session and session.is_running())

    def group_running(self, group_id: str) -> bool:
        gid = str(group_id or "").strip()
        if not gid:
            return False
        with self._lock:
            sessions = [session for (session_gid, _), session in self._sessions.items() if session_gid == gid]
        return any(session.is_running() for session in sessions)

    def get_state(self, *, group_id: str, actor_id: str) -> Optional[Dict[str, Any]]:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            session = self._sessions.get(key)
        return session.state() if session is not None and session.is_running() else None

    def submit_user_message(
        self,
        *,
        group_id: str,
        actor_id: str,
        text: str,
        event_id: str,
        ts: str = "",
        reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
    ) -> bool:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            session = self._sessions.get(key)
        if session is None:
            return False
        return session.submit_user_message(text=text, event_id=event_id, ts=ts, reply_to=reply_to, attachments=attachments)

    def submit_control_message(
        self,
        *,
        group_id: str,
        actor_id: str,
        text: str,
        control_kind: str,
        event_id: str = "",
        ts: str = "",
    ) -> bool:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            session = self._sessions.get(key)
        if session is None:
            return False
        return session.submit_control_message(
            text=text,
            control_kind=control_kind,
            event_id=event_id,
            ts=ts,
        )


SUPERVISOR = CodexAppSessionManager()
