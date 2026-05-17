"""Daemon operations for Hermes runtime setup."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.hermes_runtime import hermes_runtime_status, prepare_hermes_runtime, run_hermes_mcp_test


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _cwd_arg(args: Dict[str, Any]) -> Optional[Path]:
    raw = str(args.get("cwd") or "").strip()
    return Path(raw).expanduser().resolve() if raw else None


def try_handle_hermes_runtime_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "runtime_hermes_status":
        return DaemonResponse(ok=True, result=hermes_runtime_status())
    if op == "runtime_hermes_prepare":
        result = prepare_hermes_runtime(
            cwd=_cwd_arg(args),
            auto_enable_tools=bool(args.get("auto_enable_tools") or args.get("yes")),
            force_mcp=bool(args.get("force_mcp") or args.get("force")),
        )
        if result.get("ok"):
            return DaemonResponse(ok=True, result=result)
        error = result.get("error") if isinstance(result.get("error"), dict) else {}
        return _error(
            str(error.get("code") or "hermes_prepare_failed"),
            str(error.get("message") or "failed to prepare Hermes runtime"),
            details=result,
        )
    if op == "runtime_hermes_mcp_test":
        result = run_hermes_mcp_test(
            cwd=_cwd_arg(args),
            group_id=str(args.get("group_id") or "g_probe"),
            actor_id=str(args.get("actor_id") or "hermes-probe"),
        )
        if result.get("ok"):
            return DaemonResponse(ok=True, result=result)
        error = result.get("error") if isinstance(result.get("error"), dict) else {}
        return _error(
            str(error.get("code") or "hermes_mcp_test_failed"),
            str(error.get("message") or "Hermes MCP test failed"),
            details=result,
        )
    return None
