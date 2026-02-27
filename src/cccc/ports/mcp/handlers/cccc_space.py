"""MCP handler functions for space tools."""

from __future__ import annotations

import os
from typing import Any, Dict, Optional

from ..common import MCPError, _call_daemon_or_raise
from ..utils.space_args import _infer_artifact_language_from_source


def space_status(*, group_id: str, provider: str = "notebooklm") -> Dict[str, Any]:
    """Get Group Space status (provider + binding + queue summary)."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_status",
            "args": {"group_id": group_id, "provider": str(provider or "notebooklm")},
        }
    )


def space_capabilities(*, group_id: str, provider: str = "notebooklm") -> Dict[str, Any]:
    """Get Group Space capabilities (local file policy + ingest schema/examples)."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_capabilities",
            "args": {"group_id": group_id, "provider": str(provider or "notebooklm")},
        }
    )


def space_bind(
    *,
    group_id: str,
    by: str,
    provider: str = "notebooklm",
    action: str = "bind",
    remote_space_id: str = "",
) -> Dict[str, Any]:
    """Bind or unbind Group Space provider for a group."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_bind",
            "args": {
                "group_id": group_id,
                "provider": str(provider or "notebooklm"),
                "action": str(action or "bind"),
                "remote_space_id": str(remote_space_id or ""),
                "by": str(by or "user"),
            },
        }
    )


def space_ingest(
    *,
    group_id: str,
    by: str,
    provider: str = "notebooklm",
    kind: str = "context_sync",
    payload: Optional[Dict[str, Any]] = None,
    idempotency_key: str = "",
) -> Dict[str, Any]:
    """Submit a Group Space ingest job."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_ingest",
            "args": {
                "group_id": group_id,
                "provider": str(provider or "notebooklm"),
                "kind": str(kind or "context_sync"),
                "payload": dict(payload or {}),
                "idempotency_key": str(idempotency_key or ""),
                "by": str(by or "user"),
            },
        }
    )


def space_query(
    *,
    group_id: str,
    provider: str = "notebooklm",
    query: str,
    options: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """Query Group Space knowledge provider."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_query",
            "args": {
                "group_id": group_id,
                "provider": str(provider or "notebooklm"),
                "query": str(query or ""),
                "options": dict(options or {}),
            },
        }
    )


def space_sources(
    *,
    group_id: str,
    by: str,
    provider: str = "notebooklm",
    action: str = "list",
    source_id: str = "",
    new_title: str = "",
) -> Dict[str, Any]:
    """List/refresh/rename/delete Group Space provider sources."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_sources",
            "args": {
                "group_id": group_id,
                "provider": str(provider or "notebooklm"),
                "action": str(action or "list"),
                "source_id": str(source_id or ""),
                "new_title": str(new_title or ""),
                "by": str(by or "user"),
            },
        }
    )


def space_artifact(
    *,
    group_id: str,
    by: str,
    provider: str = "notebooklm",
    action: str = "list",
    kind: str = "",
    options: Optional[Dict[str, Any]] = None,
    wait: bool = False,
    save_to_space: bool = True,
    output_path: str = "",
    output_format: str = "",
    artifact_id: str = "",
    timeout_seconds: float = 600.0,
    initial_interval: float = 2.0,
    max_interval: float = 10.0,
) -> Dict[str, Any]:
    """List/generate/download Group Space provider artifacts."""
    action_v = str(action or "list")
    kind_v = str(kind or "")
    wait_v = bool(wait)
    if action_v == "generate" and wait_v and str(kind_v).strip().lower() in {"audio", "video"}:
        wait_v = False
    timeout_v = float(timeout_seconds)
    req = {
        "op": "group_space_artifact",
        "args": {
            "group_id": group_id,
            "provider": str(provider or "notebooklm"),
            "action": action_v,
            "kind": kind_v,
            "options": dict(options or {}),
            "wait": wait_v,
            "save_to_space": bool(save_to_space),
            "output_path": str(output_path or ""),
            "output_format": str(output_format or ""),
            "artifact_id": str(artifact_id or ""),
            "timeout_seconds": timeout_v,
            "initial_interval": float(initial_interval),
            "max_interval": float(max_interval),
            "by": str(by or "user"),
        },
    }
    daemon_timeout = 60.0
    if action_v == "generate":
        if wait_v:
            daemon_timeout = max(180.0, timeout_v + 60.0)
        else:
            daemon_timeout = 120.0
    return _call_daemon_or_raise(req, timeout_s=daemon_timeout)


def space_jobs(
    *,
    group_id: str,
    by: str,
    provider: str = "notebooklm",
    action: str = "list",
    job_id: str = "",
    state: str = "",
    limit: int = 50,
) -> Dict[str, Any]:
    """List/retry/cancel Group Space jobs."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_jobs",
            "args": {
                "group_id": group_id,
                "provider": str(provider or "notebooklm"),
                "action": str(action or "list"),
                "job_id": str(job_id or ""),
                "state": str(state or ""),
                "limit": int(limit or 50),
                "by": str(by or "user"),
            },
        }
    )


def space_sync(
    *,
    group_id: str,
    by: str,
    provider: str = "notebooklm",
    action: str = "run",
    force: bool = False,
) -> Dict[str, Any]:
    """Run Group Space file sync reconcile or read current sync state."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_sync",
            "args": {
                "group_id": group_id,
                "provider": str(provider or "notebooklm"),
                "action": str(action or "run"),
                "force": bool(force),
                "by": str(by or "user"),
            },
        }
    )


def space_provider_auth(
    *,
    provider: str = "notebooklm",
    by: str,
    action: str = "status",
    timeout_seconds: int = 900,
) -> Dict[str, Any]:
    """Control Group Space provider auth flow (status/start/cancel)."""
    req: Dict[str, Any] = {
        "provider": str(provider or "notebooklm"),
        "by": str(by or "user"),
        "action": str(action or "status"),
    }
    if str(action or "status") == "start":
        req["timeout_seconds"] = max(60, min(int(timeout_seconds or 900), 1800))
    return _call_daemon_or_raise({"op": "group_space_provider_auth", "args": req})


def space_provider_credential_status(*, provider: str = "notebooklm", by: str) -> Dict[str, Any]:
    """Read Group Space provider credential status (masked metadata)."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_provider_credential_status",
            "args": {"provider": str(provider or "notebooklm"), "by": str(by or "user")},
        }
    )


def space_provider_credential_update(
    *,
    provider: str = "notebooklm",
    by: str,
    auth_json: str = "",
    clear: bool = False,
) -> Dict[str, Any]:
    """Update/clear Group Space provider credential."""
    return _call_daemon_or_raise(
        {
            "op": "group_space_provider_credential_update",
            "args": {
                "provider": str(provider or "notebooklm"),
                "by": str(by or "user"),
                "auth_json": str(auth_json or ""),
                "clear": bool(clear),
            },
        }
    )


def parse_space_ingest_args(arguments: Dict[str, Any]) -> Dict[str, Any]:
    """Parse and normalize space_ingest MCP arguments into handler kwargs."""
    payload_raw = arguments.get("payload")
    payload = dict(payload_raw) if isinstance(payload_raw, dict) else {}
    if not payload:
        for key in (
            "source_type", "type", "url", "content", "text",
            "file_id", "mime_type", "title", "file_path", "path",
        ):
            if key not in arguments:
                continue
            value = arguments.get(key)
            if value is None:
                continue
            text = str(value).strip()
            if text:
                payload[key] = text
        source_type = str(payload.get("source_type") or payload.get("type") or "").strip().lower()
        if source_type in {"file", "local_file", "path"} and (not str(payload.get("file_path") or "").strip()):
            file_path = str(payload.get("path") or payload.get("url") or "").strip()
            if file_path:
                payload["file_path"] = file_path
    kind = str(arguments.get("kind") or "").strip()
    if not kind:
        resource_hints = {
            "source_type", "type", "url", "content", "text",
            "file_id", "mime_type", "title", "file_path", "path",
        }
        kind = "resource_ingest" if any(k in payload for k in resource_hints) else "context_sync"
    return {"kind": kind, "payload": payload}


def parse_space_artifact_args(arguments: Dict[str, Any]) -> Dict[str, Any]:
    """Parse and normalize space_artifact MCP arguments into handler kwargs."""
    options_raw = arguments.get("options")
    options = dict(options_raw) if isinstance(options_raw, dict) else {}
    source_hint = str(arguments.get("source") or "").strip()
    if source_hint and ("source" not in options):
        options["source"] = source_hint
    language_hint = str(arguments.get("language") or arguments.get("lang") or "").strip()
    if not language_hint:
        language_hint = str(options.get("language") or options.get("lang") or "").strip()
    if not language_hint:
        language_hint = str(os.environ.get("CCCC_SPACE_ARTIFACT_LANGUAGE") or "").strip()
    if not language_hint and source_hint:
        language_hint = _infer_artifact_language_from_source(source_hint)
    if language_hint and ("language" not in options):
        options["language"] = language_hint
    action_raw = str(arguments.get("action") or "").strip()
    if action_raw:
        action = action_raw
    else:
        has_generate_intent = bool(source_hint) or ("wait" in arguments) or ("save_to_space" in arguments) or bool(options)
        has_download_intent = bool(str(arguments.get("artifact_id") or "").strip() or str(arguments.get("output_path") or "").strip())
        if has_generate_intent:
            action = "generate"
        elif has_download_intent:
            action = "download"
        else:
            action = "list"
    timeout_raw = arguments.get("timeout_seconds")
    initial_raw = arguments.get("initial_interval")
    max_raw = arguments.get("max_interval")
    timeout_seconds = 600.0
    initial_interval = 2.0
    max_interval = 10.0
    try:
        if timeout_raw is not None:
            timeout_seconds = float(timeout_raw)
        if initial_raw is not None:
            initial_interval = float(initial_raw)
        if max_raw is not None:
            max_interval = float(max_raw)
    except Exception:
        raise MCPError(
            code="invalid_request",
            message="timeout_seconds/initial_interval/max_interval must be numbers",
        )
    return {
        "action": action,
        "options": options,
        "timeout_seconds": timeout_seconds,
        "initial_interval": initial_interval,
        "max_interval": max_interval,
    }
