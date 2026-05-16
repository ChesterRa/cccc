from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, Iterable, Tuple

from .runtime_session_ops import (
    mark_runtime_session_resume_failed,
    prepare_headless_runtime_resume,
    record_headless_runtime_session,
    runtime_resume_enabled,
)


CodexThreadRequest = Callable[..., Dict[str, Any]]


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
) -> Tuple[str, bool]:
    model = str(model or "").strip()
    command_list = [str(item) for item in list(command or []) if str(item).strip()]
    thread_params = _thread_start_params(cwd=cwd, model=model)
    resume_doc = prepare_headless_runtime_resume(
        group_id=group_id,
        actor_id=actor_id,
        runtime="codex",
        cwd=cwd,
        command=command_list,
        model=model,
    )

    resumed = False
    if resume_doc:
        resume_params = _thread_resume_params(thread_id=str(resume_doc.get("provider_thread_id") or ""), model=model)
        try:
            thread_resp = request("thread/resume", resume_params, timeout=20.0)
            resumed = True
        except Exception as exc:
            mark_runtime_session_resume_failed(group_id=group_id, actor_id=actor_id, error=str(exc))
            thread_resp = request("thread/start", thread_params, timeout=20.0)
    else:
        thread_resp = request("thread/start", thread_params, timeout=20.0)

    thread_id = _thread_id_from_response(thread_resp)
    if not thread_id:
        raise RuntimeError("codex app-server returned empty thread id")

    if runtime_resume_enabled():
        try:
            record_headless_runtime_session(
                group_id=group_id,
                actor_id=actor_id,
                runtime="codex",
                cwd=cwd,
                model=model,
                command=command_list,
                provider_thread_id=thread_id,
                status="usable",
                captured_from="app_server_thread_resume" if resumed else "app_server_thread_start",
                resume_eligible=True,
            )
        except Exception:
            pass
    return thread_id, resumed
