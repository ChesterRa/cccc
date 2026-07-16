from __future__ import annotations

"""Group Space related CLI command handlers."""

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_space_status",
    "cmd_space_credential_status",
    "cmd_space_credential_set",
    "cmd_space_credential_clear",
    "cmd_space_health",
    "cmd_space_auth_status",
    "cmd_space_auth_start",
    "cmd_space_auth_cancel",
    "cmd_space_auth_disconnect",
    "cmd_space_bind",
    "cmd_space_sync",
    "cmd_space_unbind",
    "cmd_space_ingest",
    "cmd_space_query",
    "cmd_space_jobs_list",
    "cmd_space_jobs_retry",
    "cmd_space_jobs_cancel",
]

def _load_space_auth_json_arg(args: argparse.Namespace) -> tuple[bool, str]:
    raw = str(getattr(args, "auth_json", "") or "").strip()
    file_path = str(getattr(args, "auth_json_file", "") or "").strip()
    if raw and file_path:
        _print_json(
            {
                "ok": False,
                "error": {
                    "code": "invalid_args",
                    "message": "use only one of --auth-json or --auth-json-file",
                },
            }
        )
        return False, ""
    if file_path:
        try:
            raw = Path(file_path).read_text(encoding="utf-8")
        except Exception as e:
            _print_json(
                {
                    "ok": False,
                    "error": {
                        "code": "invalid_auth_json_file",
                        "message": f"failed to read --auth-json-file: {e}",
                    },
                }
            )
            return False, ""
    if not str(raw or "").strip():
        _print_json({"ok": False, "error": {"code": "missing_auth_json", "message": "missing auth_json"}})
        return False, ""
    try:
        obj = _parse_json_object_arg(raw, field="auth_json")
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "invalid_auth_json", "message": str(e)}})
        return False, ""
    return True, json.dumps(obj, ensure_ascii=False, separators=(",", ":"))

def cmd_space_status(args: argparse.Namespace) -> int:
    """Show Group Space status (provider, binding, queue summary)."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon({"op": "group_space_status", "args": {"group_id": group_id, "provider": provider}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_credential_status(args: argparse.Namespace) -> int:
    """Show Group Space provider credential status (masked metadata only)."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_credential_status",
            "args": {"provider": provider, "by": by},
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_credential_set(args: argparse.Namespace) -> int:
    """Set Group Space provider credential (write-only)."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    ok, auth_json = _load_space_auth_json_arg(args)
    if not ok:
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_credential_update",
            "args": {
                "provider": provider,
                "by": by,
                "auth_json": auth_json,
                "clear": False,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_credential_clear(args: argparse.Namespace) -> int:
    """Clear stored Group Space provider credential."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_credential_update",
            "args": {
                "provider": provider,
                "by": by,
                "auth_json": "",
                "clear": True,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_health(args: argparse.Namespace) -> int:
    """Run Group Space provider health check."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_health_check",
            "args": {"provider": provider, "by": by},
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_auth_status(args: argparse.Namespace) -> int:
    """Show Group Space provider auth flow status."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_auth",
            "args": {"provider": provider, "by": by, "action": "status"},
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_auth_start(args: argparse.Namespace) -> int:
    """Start Group Space provider auth flow."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    try:
        timeout_seconds = int(getattr(args, "timeout_seconds", 900) or 900)
    except Exception:
        timeout_seconds = 900
    timeout_seconds = max(60, min(timeout_seconds, 1800))
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_auth",
            "args": {
                "provider": provider,
                "by": by,
                "action": "start",
                "timeout_seconds": timeout_seconds,
                "force_reauth": bool(getattr(args, "force_reauth", False)),
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_auth_cancel(args: argparse.Namespace) -> int:
    """Cancel Group Space provider auth flow."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_auth",
            "args": {"provider": provider, "by": by, "action": "cancel"},
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_auth_disconnect(args: argparse.Namespace) -> int:
    """Disconnect stored Group Space provider auth."""
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_provider_auth",
            "args": {"provider": provider, "by": by, "action": "disconnect"},
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_space_bind(args: argparse.Namespace) -> int:
    """Bind group to a Group Space provider remote space."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    remote_space_id = str(getattr(args, "remote_space_id", "") or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_bind",
            "args": {
                "group_id": group_id,
                "provider": provider,
                "lane": lane,
                "action": "bind",
                "remote_space_id": remote_space_id,
                "by": by,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_sync(args: argparse.Namespace) -> int:
    """Run Group Space file synchronization (repo/space -> provider)."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    force = bool(getattr(args, "force", False))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_sync",
            "args": {
                "group_id": group_id,
                "provider": provider,
                "lane": lane,
                "action": "run",
                "force": force,
                "by": by,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_unbind(args: argparse.Namespace) -> int:
    """Unbind group from a Group Space provider remote space."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_bind",
            "args": {
                "group_id": group_id,
                "provider": provider,
                "lane": lane,
                "action": "unbind",
                "remote_space_id": "",
                "by": by,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_ingest(args: argparse.Namespace) -> int:
    """Submit and execute a Group Space ingest job."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    kind = str(getattr(args, "kind", "") or "context_sync").strip() or "context_sync"
    idempotency_key = str(getattr(args, "idempotency_key", "") or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    try:
        payload = _parse_json_object_arg(getattr(args, "payload", "{}"), field="payload")
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "invalid_payload", "message": str(e)}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_ingest",
            "args": {
                "group_id": group_id,
                "provider": provider,
                "lane": lane,
                "kind": kind,
                "payload": payload,
                "idempotency_key": idempotency_key,
                "by": by,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_query(args: argparse.Namespace) -> int:
    """Query Group Space provider-backed memory."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    query = str(getattr(args, "query", "") or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not query:
        _print_json({"ok": False, "error": {"code": "missing_query", "message": "missing query"}})
        return 2
    try:
        options = _parse_json_object_arg(getattr(args, "options", "{}"), field="options")
        options = _normalize_space_query_options_cli(options)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "invalid_options", "message": str(e)}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_query",
            "args": {
                "group_id": group_id,
                "provider": provider,
                "lane": lane,
                "query": query,
                "options": options,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_jobs_list(args: argparse.Namespace) -> int:
    """List Group Space jobs."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    state = str(getattr(args, "state", "") or "").strip()
    try:
        limit = int(getattr(args, "limit", 50) or 50)
    except Exception:
        limit = 50
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    req_args: dict[str, Any] = {
        "group_id": group_id,
        "provider": provider,
        "lane": lane,
        "action": "list",
        "limit": max(1, min(limit, 500)),
    }
    if state:
        req_args["state"] = state
    resp = call_daemon({"op": "group_space_jobs", "args": req_args})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_jobs_retry(args: argparse.Namespace) -> int:
    """Retry a failed/canceled Group Space job."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    job_id = str(getattr(args, "job_id", "") or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not job_id:
        _print_json({"ok": False, "error": {"code": "missing_job_id", "message": "missing job_id"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_jobs",
            "args": {
                "group_id": group_id,
                "provider": provider,
                "lane": lane,
                "action": "retry",
                "job_id": job_id,
                "by": by,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_space_jobs_cancel(args: argparse.Namespace) -> int:
    """Cancel a pending/running Group Space job."""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    provider = str(getattr(args, "provider", "") or "notebooklm").strip() or "notebooklm"
    lane = str(getattr(args, "lane", "") or "").strip()
    by = str(getattr(args, "by", "") or "user").strip() or "user"
    job_id = str(getattr(args, "job_id", "") or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not job_id:
        _print_json({"ok": False, "error": {"code": "missing_job_id", "message": "missing job_id"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}})
        return 2
    resp = call_daemon(
        {
            "op": "group_space_jobs",
            "args": {
                "group_id": group_id,
                "provider": provider,
                "lane": lane,
                "action": "cancel",
                "job_id": job_id,
                "by": by,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2
