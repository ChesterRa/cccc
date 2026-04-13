"""Claude Code headless session manager.

Manages Claude Code CLI subprocesses in stream-json mode (bidirectional NDJSON
over stdio), mapping streaming events to headless-compatible events for the
existing frontend streaming pipeline.

Architecture mirrors ``codex_app_sessions.py``: one long-lived subprocess per
actor, stdin for user messages, stdout for NDJSON event stream.
"""
from __future__ import annotations

import json
import logging
import os
import queue
import subprocess
import threading
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

from ..kernel.actors import find_actor
from ..kernel.blobs import resolve_blob_attachment_path
from ..kernel.headless_events import append_headless_event
from ..kernel.group import load_group
from ..kernel.procedural_skills import select_procedural_skills_for_consumption
from ..kernel.system_prompt import render_system_prompt
from .headless_experience import maybe_capture_headless_turn_success_experience
from .messaging.delivery import auto_mark_headless_delivery_started, render_headless_control_text
from .runner_state_ops import headless_state_path, remove_headless_state
from ..util.fs import atomic_write_json
from ..util.process import pid_is_alive
from ..util.time import utc_now_iso

logger = logging.getLogger(__name__)


def _is_closed_stream_logging_error(exc: BaseException) -> bool:
    if not isinstance(exc, ValueError):
        return False
    message = str(exc or "").strip().lower()
    return "i/o operation on closed file" in message or "closed stream" in message


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


@dataclass
class _PendingTurn:
    text: str
    event_id: str
    ts: str = ""
    reply_to: Optional[str] = None
    control_kind: str = ""
    attachments: list[dict[str, Any]] = field(default_factory=list)


@dataclass
class ClaudeSessionState:
    status: str = "idle"
    current_task_id: Optional[str] = None
    updated_at: str = field(default_factory=utc_now_iso)
    session_id: Optional[str] = None

    def to_headless_state(self, *, group_id: str, actor_id: str) -> Dict[str, Any]:
        return {
            "group_id": group_id,
            "actor_id": actor_id,
            "status": self.status,
            "current_task_id": self.current_task_id,
            "updated_at": self.updated_at,
        }


class ClaudeAppSession:
    """Manages a single Claude Code CLI subprocess in stream-json headless mode."""

    def __init__(
        self,
        *,
        group_id: str,
        actor_id: str,
        cwd: Path,
        env: Dict[str, str],
        model: str = "",
    ) -> None:
        self.group_id = str(group_id or "").strip()
        self.actor_id = str(actor_id or "").strip()
        self.cwd = cwd
        self.env = dict(env or {})
        self.model = str(model or "").strip()
        self._proc: Optional[subprocess.Popen[str]] = None
        self._lock = threading.Lock()
        self._running = False
        self._session_state = ClaudeSessionState(status="idle")
        self._turn_queue: "queue.Queue[Optional[_PendingTurn]]" = queue.Queue()
        self._turn_done = threading.Event()
        self._active_turn_id = ""
        self._active_event_id = ""
        self._stdout_thread: Optional[threading.Thread] = None
        self._stderr_thread: Optional[threading.Thread] = None
        self._turn_thread: Optional[threading.Thread] = None

        # Streaming text delta tracking (snapshot diffing)
        self._last_text_snapshot = ""
        self._current_stream_id = ""
        self._current_message_id = ""
        self._message_started = False

        # stream_event end-of-turn tracking (for providers that skip result events)
        self._stream_end_turn_pending = False

        # Activity tracking
        self._active_tool_activities: Dict[str, str] = {}  # tool_use_id → activity_id
        self._tool_activity_context: Dict[str, Dict[str, Any]] = {}
        self._active_control_kind = ""
        self._active_prompt_text = ""

    @staticmethod
    def _usage_error_text(error: Any) -> str:
        if isinstance(error, dict):
            message = str(error.get("message") or error.get("code") or "").strip()
            if message:
                return message
            try:
                return json.dumps(error, ensure_ascii=False, sort_keys=True)
            except Exception:
                return str(error).strip()
        return str(error or "").strip()

    def _maybe_report_turn_skill_usage(
        self,
        *,
        turn_id: str,
        event_id: str,
        status: str,
        error: Any = None,
    ) -> None:
        normalized_turn_id = str(turn_id or "").strip()
        if not normalized_turn_id:
            return
        group = load_group(self.group_id)
        if group is None:
            return
        actor = find_actor(group, self.actor_id)
        raw_skills = actor.get("procedural_skills") if isinstance(actor, dict) and isinstance(actor.get("procedural_skills"), list) else None
        skills = select_procedural_skills_for_consumption(
            group,
            limit=1,
            provided_skills=raw_skills if isinstance(raw_skills, list) else None,
        )
        if not skills:
            return
        skill = skills[0] if isinstance(skills[0], dict) else {}
        skill_id = str(skill.get("skill_id") or "").strip()
        if not skill_id:
            return

        normalized_status = str(status or "").strip().lower() or "completed"
        error_text = self._trim(self._usage_error_text(error), limit=220)
        evidence_payload: Dict[str, Any] = {
            "source": "headless.turn.completed",
            "status": normalized_status,
            "event_id": str(event_id or "").strip(),
            "runtime": "claude",
        }
        if error_text:
            evidence_payload["error"] = error_text

        args: Dict[str, Any] = {
            "group_id": self.group_id,
            "skill_id": skill_id,
            "by": self.actor_id,
            "actor_id": self.actor_id,
            "turn_id": normalized_turn_id,
            "evidence_payload": evidence_payload,
            "generate_patch": False,
        }
        if normalized_status in {"completed", "success"}:
            args["evidence_type"] = "note_only"
            args["outcome"] = "success"
        else:
            failure_signal = error_text or f"Headless turn completed with status={normalized_status}."
            args.update(
                {
                    "evidence_type": "failure_signal_triggered",
                    "outcome": "failed",
                    "reason": f"headless turn completed with status={normalized_status}",
                    "generate_patch": True,
                    "patch_kind": "clarify_failure_signal",
                    "proposed_delta": {"failure_signal": failure_signal},
                }
            )

        try:
            from ..contracts.v1 import DaemonRequest
            from .server import handle_request

            resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "procedural_skill_report_usage",
                        "args": args,
                    }
                )
            )
            if not bool(getattr(resp, "ok", False)):
                logger.warning(
                    "claude turn skill usage report failed: group=%s actor=%s turn=%s skill=%s err=%s",
                    self.group_id,
                    self.actor_id,
                    normalized_turn_id,
                    skill_id,
                    getattr(getattr(resp, "error", None), "message", ""),
                )
        except Exception:
            logger.exception(
                "claude turn skill usage report crashed: group=%s actor=%s turn=%s skill=%s",
                self.group_id,
                self.actor_id,
                normalized_turn_id,
                skill_id,
            )

    def _maybe_capture_turn_success_experience(
        self,
        *,
        turn_id: str,
        event_id: str,
        status: str,
        prompt_text: str,
    ) -> None:
        normalized_status = str(status or "").strip().lower()
        if normalized_status not in {"completed", "success"}:
            return
        maybe_capture_headless_turn_success_experience(
            group_id=self.group_id,
            actor_id=self.actor_id,
            runtime="claude",
            turn_id=turn_id,
            event_id=event_id,
            prompt_text=prompt_text,
        )

    # ── state persistence ───────────────────────────────────────────────

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
                "runtime": "claude",
                "pid": pid,
                **state,
            },
        )

    # ── headless streaming event emission ─────────────────────────────────

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
            logger.exception("failed to append claude event: %s/%s %s", self.group_id, self.actor_id, event_type)

    def _emit_activity(
        self,
        *,
        status: str,
        activity_id: str,
        kind: str,
        summary: str,
        turn_id: str = "",
        stream_id: str = "",
        detail: Optional[str] = None,
        raw_item_type: str = "",
        tool_name: str = "",
        server_name: str = "",
        command: str = "",
        cwd: str = "",
        file_paths: Optional[list[str]] = None,
        query: str = "",
    ) -> None:
        if not status or not activity_id or not kind or not summary:
            return
        with self._lock:
            active_event_id = str(self._active_event_id or "").strip()
        self._emit(
            f"headless.activity.{status}",
            {
                "activity_id": activity_id,
                "kind": kind,
                "summary": summary,
                "detail": detail or None,
                "turn_id": turn_id,
                "stream_id": stream_id,
                "event_id": active_event_id,
                "raw_item_type": raw_item_type or None,
                "tool_name": tool_name or None,
                "server_name": server_name or None,
                "command": command or None,
                "cwd": cwd or None,
                "file_paths": file_paths or None,
                "query": query or None,
            },
        )

    @staticmethod
    def _trim(value: Any, *, limit: int = 120) -> str:
        text = " ".join(str(value or "").split())
        if len(text) <= limit:
            return text
        return text[: max(0, limit - 1)].rstrip() + "…"

    @staticmethod
    def _normalize_string_list(values: list[str]) -> list[str]:
        normalized: list[str] = []
        seen: set[str] = set()
        for value in values:
            text = " ".join(str(value or "").split())
            if not text or text in seen:
                continue
            seen.add(text)
            normalized.append(text)
        return normalized

    def _collect_tool_paths(self, value: Any, out: list[str], *, limit: int = 3) -> None:
        if len(out) >= limit or value is None:
            return
        if isinstance(value, str):
            text = self._trim(value, limit=100)
            if text and text not in out:
                out.append(text)
            return
        if isinstance(value, list):
            for item in value:
                self._collect_tool_paths(item, out, limit=limit)
                if len(out) >= limit:
                    return
            return
        if not isinstance(value, dict):
            return
        for key in ("file_path", "filePath", "path", "paths", "filename", "output_file", "outputFile"):
            if key in value:
                self._collect_tool_paths(value.get(key), out, limit=limit)
                if len(out) >= limit:
                    return

    def _tool_display_name(self, tool_name: str) -> tuple[str, str]:
        name = str(tool_name or "").strip()
        if not name.startswith("mcp__"):
            return name, ""
        parts = name.split("__", 2)
        server_name = parts[1] if len(parts) > 1 else ""
        display_name = parts[2] if len(parts) > 2 else name
        return display_name, server_name

    def _extract_tool_activity_context(
        self,
        tool_name: str,
        tool_input: Any,
        *,
        allow_generic_summary: bool,
    ) -> Dict[str, Any]:
        kind, generic_summary, classified_server_name = self._classify_tool(tool_name)
        display_tool_name, parsed_server_name = self._tool_display_name(tool_name)
        normalized_tool_name = self._trim(display_tool_name or tool_name, limit=80)
        server_name = self._trim(parsed_server_name or classified_server_name, limit=60)
        input_dict = tool_input if isinstance(tool_input, dict) else {}
        lower = normalized_tool_name.lower()
        command = self._trim(
            input_dict.get("command")
            or input_dict.get("cmd")
            or input_dict.get("command_line")
            or "",
            limit=120,
        )
        cwd = self._trim(input_dict.get("cwd") or "", limit=120)
        file_paths: list[str] = []
        self._collect_tool_paths(input_dict, file_paths)
        file_paths = self._normalize_string_list(file_paths)
        query = self._trim(
            input_dict.get("query")
            or input_dict.get("url")
            or input_dict.get("pattern")
            or "",
            limit=120,
        )
        summary = ""
        detail = ""

        if lower == "bash":
            summary = command
            detail = self._trim(input_dict.get("description") or "", limit=120)
        elif lower in ("read", "edit", "write", "notebookedit"):
            summary = ", ".join(file_paths)
            start_line = self._trim(input_dict.get("start_line") or input_dict.get("startLine") or input_dict.get("offset") or "", limit=40)
            end_line = self._trim(input_dict.get("end_line") or input_dict.get("endLine") or input_dict.get("limit") or "", limit=40)
            if start_line and end_line:
                detail = f"lines {start_line}-{end_line}"
            elif start_line:
                detail = f"from {start_line}"
        elif lower == "glob":
            summary = self._trim(input_dict.get("pattern") or "", limit=100)
            base_path = self._trim(input_dict.get("path") or "", limit=100)
            file_paths = []
            if base_path:
                detail = f"in {base_path}"
            query = summary or query
        elif lower == "grep":
            summary = self._trim(input_dict.get("pattern") or "", limit=100)
            search_path = self._trim(input_dict.get("path") or "", limit=100)
            glob_pattern = self._trim(input_dict.get("glob") or "", limit=80)
            file_paths = []
            detail_parts = []
            if search_path:
                detail_parts.append(f"in {search_path}")
            if glob_pattern:
                detail_parts.append(f"glob {glob_pattern}")
            detail = ", ".join(detail_parts)
            query = summary or query
        elif lower == "websearch":
            summary = self._trim(input_dict.get("query") or "", limit=120)
            query = summary or query
        elif lower == "webfetch":
            summary = self._trim(input_dict.get("url") or "", limit=120)
            detail = self._trim(input_dict.get("prompt") or "", limit=140)
            query = summary or query
        elif lower == "task":
            summary = self._trim(input_dict.get("description") or "", limit=120)
            detail = self._trim(input_dict.get("prompt") or "", limit=140)
        elif server_name:
            if file_paths:
                summary = ", ".join(file_paths)
            elif query:
                summary = query

        if not summary and allow_generic_summary:
            summary = generic_summary or normalized_tool_name or tool_name

        context: Dict[str, Any] = {
            "kind": kind,
            "summary": summary,
            "detail": detail,
            "tool_name": normalized_tool_name,
            "server_name": server_name,
            "command": command,
            "cwd": cwd,
            "file_paths": file_paths,
            "query": query,
        }
        return {
            key: value
            for key, value in context.items()
            if value not in ("", [], None)
        }

    def _merge_tool_activity_context(
        self,
        previous: Optional[Dict[str, Any]],
        incoming: Dict[str, Any],
    ) -> Dict[str, Any]:
        merged = dict(previous or {})
        for key in ("kind", "summary", "detail", "tool_name", "server_name", "command", "cwd", "query"):
            value = self._trim(incoming.get(key) or "", limit=160 if key == "detail" else 120)
            if value:
                merged[key] = value
        incoming_paths = incoming.get("file_paths") if isinstance(incoming.get("file_paths"), list) else []
        if incoming_paths:
            merged_paths = self._normalize_string_list([
                *([str(path) for path in (merged.get("file_paths") or [])] if isinstance(merged.get("file_paths"), list) else []),
                *[str(path) for path in incoming_paths],
            ])
            if merged_paths:
                merged["file_paths"] = merged_paths
        return merged

    def _emit_tool_activity(
        self,
        *,
        status: str,
        turn_id: str,
        tool_use_id: str,
        tool_name: str,
        stream_id: str = "",
        tool_input: Any = None,
        detail_override: str = "",
        allow_generic_summary: bool,
    ) -> None:
        if not tool_use_id:
            return
        incoming = self._extract_tool_activity_context(
            tool_name,
            tool_input,
            allow_generic_summary=allow_generic_summary,
        )
        context = self._merge_tool_activity_context(self._tool_activity_context.get(tool_use_id), incoming)
        if detail_override:
            context["detail"] = self._trim(detail_override, limit=160)
        activity_id = self._active_tool_activities.get(tool_use_id) or f"tool:{tool_use_id}"
        summary = self._trim(
            context.get("summary")
            or context.get("tool_name")
            or tool_name
            or tool_use_id,
            limit=120,
        )
        if not summary:
            return
        self._tool_activity_context[tool_use_id] = context
        if status != "completed" and tool_use_id not in self._active_tool_activities:
            self._active_tool_activities[tool_use_id] = activity_id
        self._emit_activity(
            status=status,
            activity_id=activity_id,
            kind=self._trim(context.get("kind") or "tool", limit=40) or "tool",
            summary=summary,
            detail=self._trim(context.get("detail") or "", limit=160) or None,
            turn_id=turn_id,
            stream_id=stream_id,
            raw_item_type="toolUse",
            tool_name=self._trim(context.get("tool_name") or tool_name, limit=80),
            server_name=self._trim(context.get("server_name") or "", limit=60),
            command=self._trim(context.get("command") or "", limit=120),
            cwd=self._trim(context.get("cwd") or "", limit=120),
            file_paths=[str(path).strip() for path in (context.get("file_paths") or []) if str(path).strip()] or None,
            query=self._trim(context.get("query") or "", limit=120),
        )
        if status == "completed":
            self._active_tool_activities.pop(tool_use_id, None)
            self._tool_activity_context.pop(tool_use_id, None)

    def _tool_result_detail(self, content: Any) -> str:
        if isinstance(content, str):
            return self._trim(content, limit=160)
        if not isinstance(content, list):
            return ""
        for block in content:
            if isinstance(block, dict) and str(block.get("type") or "").strip() == "text":
                detail = self._trim(block.get("text") or "", limit=160)
                if detail:
                    return detail
        return ""

    # ── lifecycle ───────────────────────────────────────────────────────

    def start(self) -> None:
        with self._lock:
            if self._running:
                return
            env = os.environ.copy()
            env.update(self.env)

            cmd: List[str] = [
                "claude",
                "-p",
                "--input-format", "stream-json",
                "--output-format", "stream-json",
                "--include-partial-messages",
                "--include-hook-events",
                "--verbose",
                "--dangerously-skip-permissions",
                "--no-session-persistence",
            ]
            if self.model:
                cmd.extend(["--model", self.model])

            self._proc = subprocess.Popen(
                cmd,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                cwd=str(self.cwd),
                env=env,
                text=True,
                bufsize=1,
            )
            self._running = True

        self._stdout_thread = threading.Thread(
            target=self._stdout_loop,
            name=f"cccc-claude-out:{self.group_id}:{self.actor_id}",
            daemon=True,
        )
        self._stderr_thread = threading.Thread(
            target=self._stderr_loop,
            name=f"cccc-claude-err:{self.group_id}:{self.actor_id}",
            daemon=True,
        )
        self._turn_thread = threading.Thread(
            target=self._turn_loop,
            name=f"cccc-claude-turn:{self.group_id}:{self.actor_id}",
            daemon=True,
        )

        self._stdout_thread.start()
        self._stderr_thread.start()

        # Wait briefly for process to prove it's alive (MCP init may take time)
        time.sleep(1.0)
        if not self.is_running():
            raise RuntimeError("claude process exited immediately")

        with self._lock:
            self._session_state.status = "idle"
            self._session_state.updated_at = utc_now_iso()
        self._persist_state()
        self._queue_bootstrap_control_turn()
        self._turn_thread.start()
        logger.info("claude headless started: group=%s actor=%s pid=%s", self.group_id, self.actor_id, self._proc.pid if self._proc else "?")

    def stop(self) -> None:
        with self._lock:
            proc = self._proc
            was_running = self._running
            self._running = False
            self._proc = None
            self._session_state.status = "stopped"
            self._session_state.current_task_id = None
            self._session_state.updated_at = utc_now_iso()
            self._active_control_kind = ""
        if was_running:
            exit_code = proc.poll() if proc else None
            logger.info("claude headless stopping: group=%s actor=%s exit_code=%s", self.group_id, self.actor_id, exit_code)
        self._persist_state()
        self._turn_done.set()
        try:
            self._turn_queue.put_nowait(None)
        except Exception:
            pass
        if proc is not None:
            try:
                if proc.stdin:
                    proc.stdin.close()
            except Exception:
                pass
            try:
                proc.terminate()
            except Exception:
                pass
            try:
                proc.wait(timeout=3.0)
            except Exception:
                try:
                    proc.kill()
                except Exception:
                    pass
        self._emit("headless.session.stopped", {})

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
        payload = _PendingTurn(
            text=str(text or ""),
            event_id=str(event_id or "").strip(),
            ts=str(ts or "").strip(),
            control_kind=str(control_kind or "").strip().lower(),
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

    # ── user message submission ─────────────────────────────────────────

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

    # ── stdin writer ────────────────────────────────────────────────────

    def _write_stdin(self, data: Dict[str, Any]) -> bool:
        with self._lock:
            proc = self._proc
            if not self._running or proc is None or proc.stdin is None:
                return False
            try:
                line = json.dumps(data, ensure_ascii=False)
                proc.stdin.write(line + "\n")
                proc.stdin.flush()
                return True
            except Exception:
                return False

    # ── stdout event loop ───────────────────────────────────────────────

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
                    event = json.loads(line)
                except Exception:
                    logger.debug("ignore non-json claude output: %s", line[:200])
                    continue
                if not isinstance(event, dict):
                    continue
                self._handle_event(event)
        except Exception:
            logger.exception("claude stdout loop failed: %s/%s", self.group_id, self.actor_id)
        finally:
            self.stop()

    def _stderr_loop(self) -> None:
        proc = self._proc
        if proc is None or proc.stderr is None:
            return
        try:
            for raw_line in proc.stderr:
                line = str(raw_line or "").rstrip()
                if line:
                    _safe_logger_call("info", "[claude-app %s/%s] %s", self.group_id, self.actor_id, line)
        except Exception as exc:
            if _is_closed_stream_logging_error(exc):
                return
            _safe_logger_call("exception", "claude stderr loop failed: %s/%s", self.group_id, self.actor_id)

    def _compose_user_content(self, payload: _PendingTurn) -> str:
        text = str(payload.text or "")
        group = load_group(self.group_id)
        image_paths: list[str] = []
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
                if abs_path.exists() and abs_path.is_file():
                    image_paths.append(str(abs_path))

        if not image_paths:
            return text

        lines = [text.rstrip()] if text.strip() else []
        lines.append("[cccc] 图片附件已保存到本地文件。Claude stream-json 当前仅支持文本输入，不能直接内嵌图片。")
        lines.append("[cccc] 请优先基于以下图片文件路径继续处理：")
        for path in image_paths[:8]:
            lines.append(f"- {path}")
        if len(image_paths) > 8:
            lines.append(f"- … ({len(image_paths) - 8} more)")
        return "\n".join([line for line in lines if line]).strip()

    # ── turn loop ───────────────────────────────────────────────────────

    def _turn_loop(self) -> None:
        while self.is_running():
            try:
                payload = self._turn_queue.get(timeout=0.5)
            except queue.Empty:
                continue
            if payload is None:
                return
            self._turn_done.clear()
            turn_id = uuid.uuid4().hex[:12]
            with self._lock:
                self._active_control_kind = str(payload.control_kind or "").strip().lower()

            # Reset streaming state for new turn
            self._last_text_snapshot = ""
            self._current_stream_id = ""
            self._current_message_id = ""
            self._message_started = False
            self._stream_end_turn_pending = False
            self._active_tool_activities.clear()
            self._tool_activity_context.clear()

            with self._lock:
                self._active_turn_id = turn_id
                self._active_event_id = payload.event_id
                self._active_prompt_text = payload.text if not payload.control_kind else ""
                if not payload.control_kind:
                    self._session_state.status = "working"
                self._session_state.current_task_id = turn_id or payload.event_id or None
                self._session_state.updated_at = utc_now_iso()
            self._persist_state()

            # Send user message to claude via stdin
            user_content = self._compose_user_content(payload)
            ok = self._write_stdin({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": user_content,
                },
            })
            if not ok:
                logger.warning("claude stdin write failed: group=%s actor=%s", self.group_id, self.actor_id)
                with self._lock:
                    self._session_state.status = "idle"
                    self._active_event_id = ""
                    self._active_control_kind = ""
                    self._active_prompt_text = ""
                    self._session_state.current_task_id = None
                    self._session_state.updated_at = utc_now_iso()
                self._persist_state()
                self._emit(
                    "headless.control.failed" if payload.control_kind else "headless.turn.failed",
                    {
                        "turn_id": turn_id,
                        "event_id": payload.event_id,
                        "control_kind": payload.control_kind or None,
                        "error": "failed to write to claude stdin",
                    },
                )
                continue

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

            # Wait for turn completion (signaled from _handle_event)
            self._turn_done.wait()

    # ── event handling ──────────────────────────────────────────────────

    def _handle_event(self, event: Dict[str, Any]) -> None:
        event_type = str(event.get("type") or "").strip()
        if not event_type:
            return

        if event_type == "system":
            self._handle_system_event(event)
        elif event_type == "assistant":
            self._handle_assistant_event(event)
        elif event_type == "tool_progress":
            self._handle_tool_progress_event(event)
        elif event_type == "tool_result":
            self._handle_tool_result_event(event)
        elif event_type == "tool_use_summary":
            self._handle_tool_use_summary_event(event)
        elif event_type == "result":
            self._handle_result_event(event)
        elif event_type == "stream_event":
            self._handle_stream_event(event)
        elif event_type == "user":
            pass  # echo of user/tool_result messages sent back — no action needed
        else:
            logger.debug("claude unhandled event type=%s: %s", event_type, str(event)[:300])

    def _handle_system_event(self, event: Dict[str, Any]) -> None:
        subtype = str(event.get("subtype") or "").strip()
        if subtype == "init":
            session_id = str(event.get("session_id") or "").strip()
            with self._lock:
                self._session_state.session_id = session_id or None
            logger.info(
                "claude session init: group=%s actor=%s session=%s model=%s",
                self.group_id, self.actor_id, session_id,
                str(event.get("model") or "").strip(),
            )
            return

        with self._lock:
            turn_id = str(self._active_turn_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()

        if control_kind:
            return

        if subtype == "hook_started":
            hook_id = str(event.get("hook_id") or "").strip()
            hook_name = self._trim(event.get("hook_name") or "hook", limit=80)
            hook_event = self._trim(event.get("hook_event") or "", limit=100)
            if hook_id and hook_name:
                self._emit_activity(
                    status="started",
                    activity_id=f"hook:{hook_id}",
                    kind="tool",
                    summary=hook_name,
                    detail=f"event {hook_event}" if hook_event else None,
                    turn_id=turn_id,
                    raw_item_type="hook_started",
                    tool_name=hook_name,
                )
            return

        if subtype == "hook_progress":
            hook_id = str(event.get("hook_id") or "").strip()
            hook_name = self._trim(event.get("hook_name") or "hook", limit=80)
            detail = self._trim(
                event.get("output") or event.get("stdout") or event.get("stderr") or "",
                limit=160,
            )
            if hook_id and hook_name:
                self._emit_activity(
                    status="updated",
                    activity_id=f"hook:{hook_id}",
                    kind="tool",
                    summary=hook_name,
                    detail=detail or None,
                    turn_id=turn_id,
                    raw_item_type="hook_progress",
                    tool_name=hook_name,
                )
            return

        if subtype == "hook_response":
            hook_id = str(event.get("hook_id") or "").strip()
            hook_name = self._trim(event.get("hook_name") or "hook", limit=80)
            outcome = self._trim(event.get("outcome") or "", limit=40)
            detail = self._trim(
                event.get("output") or event.get("stdout") or event.get("stderr") or outcome,
                limit=160,
            )
            if hook_id and hook_name:
                self._emit_activity(
                    status="completed",
                    activity_id=f"hook:{hook_id}",
                    kind="tool",
                    summary=hook_name,
                    detail=detail or None,
                    turn_id=turn_id,
                    raw_item_type="hook_response",
                    tool_name=hook_name,
                )
            return

        if subtype == "task_started":
            task_id = str(event.get("task_id") or "").strip()
            description = self._trim(event.get("description") or task_id or "sub-task", limit=120)
            detail = self._trim(event.get("prompt") or event.get("workflow_name") or event.get("task_type") or "", limit=160)
            if task_id and description:
                self._emit_activity(
                    status="started",
                    activity_id=f"task:{task_id}",
                    kind="thinking",
                    summary=description,
                    detail=detail or None,
                    turn_id=turn_id,
                    raw_item_type="task_started",
                )
            return

        if subtype == "task_progress":
            task_id = str(event.get("task_id") or "").strip()
            description = self._trim(event.get("description") or "sub-task", limit=120)
            detail = self._trim(event.get("summary") or event.get("last_tool_name") or "", limit=160)
            if task_id and description:
                self._emit_activity(
                    status="updated",
                    activity_id=f"task:{task_id}",
                    kind="thinking",
                    summary=description,
                    detail=detail or None,
                    turn_id=turn_id,
                    raw_item_type="task_progress",
                )
            return

        if subtype == "task_notification":
            task_id = str(event.get("task_id") or "").strip()
            summary = self._trim(event.get("summary") or task_id or "sub-task", limit=120)
            status = self._trim(event.get("status") or "completed", limit=40)
            output_file = self._trim(event.get("output_file") or "", limit=140)
            detail = output_file or (status if status and status != "completed" else "")
            if task_id and summary:
                self._emit_activity(
                    status="completed",
                    activity_id=f"task:{task_id}",
                    kind="thinking",
                    summary=summary,
                    detail=detail or None,
                    turn_id=turn_id,
                    raw_item_type="task_notification",
                )
            return

    def _handle_stream_event(self, event: Dict[str, Any]) -> None:
        """Handle raw Anthropic streaming events from --include-partial-messages."""
        inner = event.get("event") if isinstance(event.get("event"), dict) else {}
        inner_type = str(inner.get("type") or "").strip()
        if not inner_type:
            return

        with self._lock:
            active_event_id = str(self._active_event_id or "").strip()
            turn_id = str(self._active_turn_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()

        if control_kind:
            if inner_type == "message_delta":
                delta_obj = inner.get("delta") if isinstance(inner.get("delta"), dict) else {}
                if str(delta_obj.get("stop_reason") or "").strip() == "end_turn":
                    self._stream_end_turn_pending = True
                return
            if inner_type == "message_stop":
                if self._stream_end_turn_pending:
                    self._complete_turn_from_stream()
                return
            return

        if inner_type == "message_start":
            # Extract message id for stream tracking
            msg = inner.get("message") if isinstance(inner.get("message"), dict) else {}
            message_id = str(msg.get("id") or "").strip()
            effective_id = message_id or (f"turn-{turn_id}" if turn_id else "")
            if effective_id and effective_id != self._current_message_id:
                self._current_message_id = effective_id
                self._current_stream_id = effective_id
                self._last_text_snapshot = ""
                self._message_started = False

        elif inner_type == "content_block_start":
            block = inner.get("content_block") if isinstance(inner.get("content_block"), dict) else {}
            block_type = str(block.get("type") or "").strip()
            if block_type == "tool_use":
                tool_use_id = str(block.get("id") or "").strip()
                tool_name = str(block.get("name") or "").strip()
                if tool_use_id and tool_name and tool_use_id not in self._active_tool_activities:
                    self._emit_tool_activity(
                        status="started",
                        turn_id=turn_id,
                        tool_use_id=tool_use_id,
                        tool_name=tool_name,
                        stream_id=self._current_stream_id,
                        tool_input=block.get("input"),
                        allow_generic_summary=True,
                    )

        elif inner_type == "content_block_delta":
            delta_obj = inner.get("delta") if isinstance(inner.get("delta"), dict) else {}
            delta_type = str(delta_obj.get("type") or "").strip()
            if delta_type == "text_delta":
                text = str(delta_obj.get("text") or "")
                if text:
                    stream_id = self._current_stream_id
                    if not stream_id and turn_id:
                        stream_id = f"turn-{turn_id}"
                        self._current_stream_id = stream_id
                    if stream_id:
                        if not self._message_started:
                            self._message_started = True
                            self._emit(
                                "headless.message.started",
                                {
                                    "turn_id": turn_id,
                                    "event_id": active_event_id,
                                    "stream_id": stream_id,
                                },
                            )
                        self._last_text_snapshot += text
                        self._emit(
                            "headless.message.delta",
                            {
                                "turn_id": turn_id,
                                "event_id": active_event_id,
                                "stream_id": stream_id,
                                "delta": text,
                            },
                        )

        elif inner_type == "message_delta":
            delta_obj = inner.get("delta") if isinstance(inner.get("delta"), dict) else {}
            if str(delta_obj.get("stop_reason") or "").strip() == "end_turn":
                self._stream_end_turn_pending = True

        elif inner_type == "message_stop":
            if self._stream_end_turn_pending:
                self._complete_turn_from_stream()

    def _complete_turn_from_stream(self) -> None:
        """Complete a turn using accumulated stream_event data (for providers that don't send result events)."""
        now = utc_now_iso()

        with self._lock:
            turn_id = str(self._active_turn_id or "").strip()
            active_event_id = str(self._active_event_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()
            # Guard: if turn already completed by _handle_result_event, no-op.
            if not turn_id:
                return
            self._active_turn_id = ""
            self._active_event_id = ""
            self._active_control_kind = ""
            self._session_state.status = "idle"
            self._session_state.current_task_id = None
            self._session_state.updated_at = now
        self._persist_state()

        stream_id = self._current_stream_id or ""
        text = self._last_text_snapshot or ""

        # Complete any remaining tool activities
        for tool_use_id in list(self._active_tool_activities):
            tool_name = str((self._tool_activity_context.get(tool_use_id) or {}).get("tool_name") or "")
            self._emit_tool_activity(
                status="completed",
                turn_id=turn_id,
                tool_use_id=tool_use_id,
                tool_name=tool_name,
                allow_generic_summary=False,
            )
        self._active_tool_activities.clear()
        self._tool_activity_context.clear()

        # Emit message completed if we have text.
        if text and stream_id and not control_kind:
            self._emit(
                "headless.message.completed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "stream_id": stream_id,
                    "text": text,
                },
            )

        # Reset streaming state so _handle_assistant_event won't re-emit for same turn.
        self._message_started = False
        self._last_text_snapshot = ""
        self._current_stream_id = ""
        self._current_message_id = ""

        self._emit(
            "headless.control.completed" if control_kind else "headless.turn.completed",
            {
                "turn_id": turn_id,
                "event_id": active_event_id,
                "control_kind": control_kind or None,
                "status": "completed",
            },
        )
        if not control_kind:
            self._maybe_report_turn_skill_usage(
                turn_id=turn_id,
                event_id=active_event_id,
                status="completed",
            )

        self._turn_done.set()

    def _handle_assistant_event(self, event: Dict[str, Any]) -> None:
        message = event.get("message") if isinstance(event.get("message"), dict) else {}
        content = message.get("content") if isinstance(message.get("content"), list) else []
        message_id = str(message.get("id") or "").strip()
        is_partial = bool(event.get("partial"))

        with self._lock:
            active_event_id = str(self._active_event_id or "").strip()
            turn_id = str(self._active_turn_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()

        if control_kind:
            return

        # Track new message — use message_id if present, else generate a fallback per turn.
        # Don't reset streaming state if we already have an active stream (from stream_events).
        effective_id = message_id or (f"turn-{turn_id}" if turn_id else "")
        if effective_id and effective_id != self._current_message_id and not self._message_started:
            self._current_message_id = effective_id
            self._current_stream_id = effective_id
            self._last_text_snapshot = ""
            self._message_started = False

        stream_id = self._current_stream_id or effective_id

        # Process content blocks
        accumulated_text = ""
        tool_use_blocks: List[Dict[str, Any]] = []
        for block in content:
            if not isinstance(block, dict):
                continue
            block_type = str(block.get("type") or "").strip()
            if block_type == "text":
                accumulated_text += str(block.get("text") or "")
            elif block_type == "tool_use":
                tool_use_blocks.append(block)

        # Emit text streaming events
        if accumulated_text and stream_id:
            if not self._message_started:
                self._message_started = True
                self._emit(
                    "headless.message.started",
                    {
                        "turn_id": turn_id,
                        "event_id": active_event_id,
                        "stream_id": stream_id,
                    },
                )

            # Compute delta from snapshot
            delta = accumulated_text[len(self._last_text_snapshot):]
            if delta:
                self._last_text_snapshot = accumulated_text
                self._emit(
                    "headless.message.delta",
                    {
                        "turn_id": turn_id,
                        "event_id": active_event_id,
                        "stream_id": stream_id,
                        "delta": delta,
                    },
                )

        # Handle tool use blocks — emit activity events
        for block in tool_use_blocks:
            tool_use_id = str(block.get("id") or "").strip()
            tool_name = str(block.get("name") or "").strip()
            if not tool_use_id or not tool_name:
                continue
            self._emit_tool_activity(
                status="started" if tool_use_id not in self._active_tool_activities else "updated",
                turn_id=turn_id,
                tool_use_id=tool_use_id,
                tool_name=tool_name,
                stream_id=stream_id,
                tool_input=block.get("input"),
                allow_generic_summary=tool_use_id not in self._active_tool_activities,
            )

        # If this is a final (non-partial) assistant message with text, emit completed
        if not is_partial and accumulated_text and stream_id and self._message_started:
            self._emit(
                "headless.message.completed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "stream_id": stream_id,
                    "text": accumulated_text,
                },
            )
            # Reset for potential next message in same turn
            self._last_text_snapshot = ""
            self._current_stream_id = ""
            self._current_message_id = ""
            self._message_started = False

    def _handle_tool_result_event(self, event: Dict[str, Any]) -> None:
        tool_use_id = str(event.get("tool_use_id") or "").strip()
        with self._lock:
            turn_id = str(self._active_turn_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()
        if control_kind:
            self._active_tool_activities.pop(tool_use_id, "")
            self._tool_activity_context.pop(tool_use_id, None)
            return
        if tool_use_id:
            tool_name = str((self._tool_activity_context.get(tool_use_id) or {}).get("tool_name") or event.get("tool_name") or "")
            self._emit_tool_activity(
                status="completed",
                turn_id=turn_id,
                tool_use_id=tool_use_id,
                tool_name=tool_name,
                detail_override=self._tool_result_detail(event.get("content")),
                allow_generic_summary=False,
            )

    def _handle_tool_progress_event(self, event: Dict[str, Any]) -> None:
        tool_use_id = str(event.get("tool_use_id") or "").strip()
        tool_name = str(event.get("tool_name") or "").strip()
        with self._lock:
            turn_id = str(self._active_turn_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()
        if control_kind or not tool_use_id or not tool_name:
            return
        elapsed_seconds = event.get("elapsed_time_seconds")
        detail = ""
        if isinstance(elapsed_seconds, (int, float)):
            detail = f"running for {int(elapsed_seconds)}s"
        self._emit_tool_activity(
            status="updated",
            turn_id=turn_id,
            tool_use_id=tool_use_id,
            tool_name=tool_name,
            detail_override=detail,
            allow_generic_summary=False,
        )

    def _handle_tool_use_summary_event(self, event: Dict[str, Any]) -> None:
        summary = self._trim(event.get("summary") or "", limit=140)
        with self._lock:
            turn_id = str(self._active_turn_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()
        if control_kind or not summary:
            return
        preceding = event.get("preceding_tool_use_ids") if isinstance(event.get("preceding_tool_use_ids"), list) else []
        detail = ""
        if preceding:
            detail = f"after {len(preceding)} tool calls"
        activity_id = f"tool-summary:{turn_id or str(event.get('uuid') or '').strip() or 'current'}"
        self._emit_activity(
            status="updated",
            activity_id=activity_id,
            kind="tool",
            summary=summary,
            detail=detail or None,
            turn_id=turn_id,
            raw_item_type="tool_use_summary",
        )

    def _handle_result_event(self, event: Dict[str, Any]) -> None:
        subtype = str(event.get("subtype") or "").strip()
        now = utc_now_iso()

        with self._lock:
            active_event_id = str(self._active_event_id or "").strip()
            turn_id = str(self._active_turn_id or "").strip()
            control_kind = str(self._active_control_kind or "").strip().lower()
            # Guard: if turn already completed by _complete_turn_from_stream, no-op.
            if not turn_id:
                return
            self._active_turn_id = ""
            self._active_event_id = ""
            self._active_control_kind = ""
            self._session_state.status = "idle"
            self._session_state.current_task_id = None
            self._session_state.updated_at = now
        self._persist_state()

        # Complete any remaining tool activities
        for tool_use_id in list(self._active_tool_activities):
            tool_name = str((self._tool_activity_context.get(tool_use_id) or {}).get("tool_name") or "")
            self._emit_tool_activity(
                status="completed",
                turn_id=turn_id,
                tool_use_id=tool_use_id,
                tool_name=tool_name,
                allow_generic_summary=False,
            )
        self._active_tool_activities.clear()
        self._tool_activity_context.clear()

        if control_kind and subtype in ("success", ""):
            self._emit(
                "headless.control.completed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "control_kind": control_kind,
                    "status": "completed",
                },
            )
        elif control_kind:
            error_text = str(event.get("error") or event.get("result") or "unknown error")
            self._emit(
                "headless.control.failed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "control_kind": control_kind,
                    "status": subtype or "completed",
                    "error": {"message": error_text},
                },
            )
        elif subtype in ("success", ""):
            with self._lock:
                active_prompt_text = self._active_prompt_text
                self._active_prompt_text = ""
            self._maybe_report_turn_skill_usage(
                turn_id=turn_id,
                event_id=active_event_id,
                status="completed",
            )
            self._maybe_capture_turn_success_experience(
                turn_id=turn_id,
                event_id=active_event_id,
                status="completed",
                prompt_text=active_prompt_text,
            )
            self._emit(
                "headless.turn.completed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "status": "completed",
                },
            )
        else:
            with self._lock:
                self._active_prompt_text = ""
            error_text = str(event.get("error") or event.get("result") or "unknown error")
            self._maybe_report_turn_skill_usage(
                turn_id=turn_id,
                event_id=active_event_id,
                status=subtype or "completed",
                error={"message": error_text},
            )
            self._emit(
                "headless.turn.failed" if subtype == "error" else "headless.turn.completed",
                {
                    "turn_id": turn_id,
                    "event_id": active_event_id,
                    "status": subtype or "completed",
                    "error": {"message": error_text} if subtype == "error" else None,
                },
            )

        self._turn_done.set()

    # ── tool classification ─────────────────────────────────────────────

    @staticmethod
    def _classify_tool(tool_name: str) -> tuple[str, str, str]:
        """Classify a Claude tool into (kind, summary, server_name)."""
        name = str(tool_name or "").strip()

        # MCP tools: mcp__<server>__<tool>
        if name.startswith("mcp__"):
            parts = name.split("__", 2)
            server = parts[1] if len(parts) > 1 else ""
            tool = parts[2] if len(parts) > 2 else name
            return "tool", f"{server}:{tool}" if server else tool, server

        # Built-in Claude Code tools
        lower = name.lower()
        if lower in ("bash",):
            return "command", name, ""
        if lower in ("edit", "write", "notebookedit"):
            return "patch", name, ""
        if lower in ("read", "glob", "grep"):
            return "search", name, ""
        if lower in ("websearch", "webfetch"):
            return "search", name, ""
        if lower == "task":
            return "thinking", "sub-task", ""

        return "tool", name, ""

    # ── ledger message ──────────────────────────────────────────────────

# ── session manager ────────────────────────────────────────────────────


class ClaudeAppSessionManager:
    """Manages Claude headless sessions across groups/actors."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._sessions: Dict[tuple[str, str], ClaudeAppSession] = {}

    def start_actor(
        self,
        *,
        group_id: str,
        actor_id: str,
        cwd: Path,
        env: Dict[str, str],
        model: str = "",
    ) -> ClaudeAppSession:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            raise ValueError("missing group_id/actor_id")
        with self._lock:
            session = self._sessions.get(key)
            if session is not None and session.is_running():
                return session
            session = ClaudeAppSession(group_id=key[0], actor_id=key[1], cwd=cwd, env=env, model=model)
            self._sessions[key] = session
        try:
            session.start()
        except Exception:
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
            sessions = [s for (g, _), s in self._sessions.items() if g == gid]
        return any(s.is_running() for s in sessions)

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


SUPERVISOR = ClaudeAppSessionManager()
