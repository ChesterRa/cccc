from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, Iterable, NamedTuple

from .runtime_session_ops import (
    mark_runtime_session_resume_failed,
    prepare_codex_app_thread_resume,
    record_codex_app_thread_runtime_session,
    runtime_resume_enabled,
)


CodexThreadRequest = Callable[..., Dict[str, Any]]


class CodexThreadStartResult(NamedTuple):
    thread_id: str
    resumed: bool


def _thread_start_params(*, cwd: Path, model: str) -> Dict[str, Any]:
    params: Dict[str, Any] = {
        "cwd": str(cwd),
        "approvalPolicy": "never",
        "sandbox": "danger-full-access",
        "personality": "pragmatic",
    }
    if model:
        params["model"] = model
    return params


def _thread_resume_params(*, thread_id: str, model: str) -> Dict[str, Any]:
    params: Dict[str, Any] = {
        "threadId": str(thread_id or "").strip(),
        "approvalPolicy": "never",
        "sandbox": "danger-full-access",
        "personality": "pragmatic",
    }
    if model:
        params["model"] = model
    return params


def _thread_id_from_response(response: Dict[str, Any]) -> str:
    thread = response.get("thread") if isinstance(response, dict) else {}
    return str((thread or {}).get("id") or "").strip()


def start_codex_app_thread(
    *,
    request: CodexThreadRequest,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    model: str = "",
    runner: str = "headless",
) -> CodexThreadStartResult:
    model = str(model or "").strip()
    command_list = [str(item) for item in list(command or []) if str(item).strip()]
    thread_params = _thread_start_params(cwd=cwd, model=model)
    resume_doc = prepare_codex_app_thread_resume(
        group_id=group_id,
        actor_id=actor_id,
        cwd=cwd,
        command=command_list,
        model=model,
    )

    resumed = False
    if resume_doc:
        resume_thread_id = str(resume_doc.get("provider_thread_id") or "").strip()
        resume_params = _thread_resume_params(thread_id=resume_thread_id, model=model)
        try:
            thread_resp = request("thread/resume", resume_params, timeout=20.0)
            resumed = True
        except Exception as exc:
            mark_runtime_session_resume_failed(group_id=group_id, actor_id=actor_id, error=str(exc))
            thread_resp = request("thread/start", thread_params, timeout=20.0)
            resumed = False
    else:
        thread_resp = request("thread/start", thread_params, timeout=20.0)

    thread_id = _thread_id_from_response(thread_resp)
    if not thread_id:
        raise RuntimeError("codex app-server returned empty thread id")

    if runtime_resume_enabled():
        try:
            record_codex_app_thread_runtime_session(
                group_id=group_id,
                actor_id=actor_id,
                cwd=cwd,
                model=model,
                command=command_list,
                provider_thread_id=thread_id,
                runner=runner,
                status="usable",
                captured_from="app_server_thread_resume" if resumed else "app_server_thread_start",
                resume_eligible=True,
            )
        except Exception:
            pass
    return CodexThreadStartResult(
        thread_id=thread_id,
        resumed=resumed,
    )


def prepare_codex_app_tui_resume(
    *,
    request: CodexThreadRequest,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    model: str = "",
) -> CodexThreadStartResult:
    model = str(model or "").strip()
    command_list = [str(item) for item in list(command or []) if str(item).strip()]
    resume_doc = prepare_codex_app_thread_resume(
        group_id=group_id,
        actor_id=actor_id,
        cwd=cwd,
        command=command_list,
        model=model,
    )
    if resume_doc:
        resume_thread_id = str(resume_doc.get("provider_thread_id") or "").strip()
        try:
            resume_params = _thread_resume_params(thread_id=resume_thread_id, model=model)
            thread_resp = request("thread/resume", resume_params, timeout=20.0)
            thread_id = _thread_id_from_response(thread_resp) or resume_thread_id
            if runtime_resume_enabled():
                try:
                    record_codex_app_thread_runtime_session(
                        group_id=group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        model=model,
                        command=command_list,
                        provider_thread_id=thread_id,
                        runner="pty",
                        status="usable",
                        captured_from="app_server_thread_resume",
                        resume_eligible=True,
                    )
                except Exception:
                    pass
            return CodexThreadStartResult(thread_id=thread_id, resumed=True)
        except Exception as exc:
            mark_runtime_session_resume_failed(group_id=group_id, actor_id=actor_id, error=str(exc))
    return CodexThreadStartResult(thread_id="", resumed=False)
