"""Remote access operation handlers for daemon (global scope)."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from typing import Any, Dict, Optional, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.events import publish_event
from ...kernel.settings import get_remote_access_settings, update_remote_access_settings
from ...util.conv import coerce_bool
from ...util.time import utc_now_iso


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _allow_insecure_remote() -> bool:
    v = str(os.environ.get("CCCC_REMOTE_ALLOW_INSECURE") or "").strip().lower()
    return v in ("1", "true", "yes", "y", "on")


def _allow_loopback_remote() -> bool:
    v = str(os.environ.get("CCCC_REMOTE_ALLOW_LOOPBACK") or "").strip().lower()
    return v in ("1", "true", "yes", "y", "on")


def _supported_modes() -> set[str]:
    return {"tailnet_only"}


def _normalize_mode(raw: Any) -> str:
    mode = str(raw or "").strip().lower() or "tailnet_only"
    return mode


def _mode_supported(mode: str) -> bool:
    return mode in _supported_modes()


def _safe_web_port() -> int:
    raw = str(os.environ.get("CCCC_WEB_PORT") or "").strip() or "8848"
    try:
        n = int(raw)
    except Exception:
        n = 8848
    if n <= 0 or n > 65535:
        n = 8848
    return n


def _is_loopback_host(host: str) -> bool:
    v = str(host or "").strip().lower()
    return v in ("127.0.0.1", "localhost", "::1")


def _manual_endpoint() -> Optional[str]:
    public_url = str(os.environ.get("CCCC_WEB_PUBLIC_URL") or "").strip()
    if public_url:
        return public_url
    host = str(os.environ.get("CCCC_WEB_HOST") or "").strip() or "127.0.0.1"
    port = _safe_web_port()
    if _is_loopback_host(host):
        return None
    return f"http://{host}:{port}/ui/"


def _run_command(argv: list[str], *, timeout_s: float) -> Tuple[int, str, str]:
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=float(timeout_s),
            check=False,
            env=dict(os.environ),
        )
        return int(proc.returncode), str(proc.stdout or ""), str(proc.stderr or "")
    except FileNotFoundError:
        return 127, "", "command not found"
    except subprocess.TimeoutExpired as e:
        return 124, str(e.stdout or ""), str(e.stderr or "timeout")
    except Exception as e:
        return 1, "", str(e)


def _tailscale_installed() -> bool:
    return bool(shutil.which("tailscale"))


def _tailscale_status_json() -> Tuple[Optional[Dict[str, Any]], str]:
    code, out, err = _run_command(["tailscale", "status", "--json"], timeout_s=8.0)
    if code != 0:
        return None, str(err or out or "failed to read tailscale status").strip()
    try:
        doc = json.loads(out or "{}")
    except Exception:
        return None, "invalid tailscale status json"
    if not isinstance(doc, dict):
        return None, "invalid tailscale status payload"
    return doc, ""


def _tailscale_backend_state(doc: Dict[str, Any]) -> str:
    return str(doc.get("BackendState") or "").strip().lower()


def _tailscale_endpoint(doc: Dict[str, Any]) -> Optional[str]:
    self_doc = doc.get("Self") if isinstance(doc.get("Self"), dict) else {}
    ips = self_doc.get("TailscaleIPs") if isinstance(self_doc, dict) else []
    tail_ip = ""
    if isinstance(ips, list):
        for item in ips:
            s = str(item or "").strip()
            if "." in s:
                tail_ip = s
                break
    if not tail_ip and isinstance(self_doc, dict):
        s = str(self_doc.get("TailscaleIP") or "").strip()
        if "." in s:
            tail_ip = s
    if not tail_ip:
        return None
    return f"http://{tail_ip}:{_safe_web_port()}/ui/"


def _web_binding_diagnostics(*, provider: str, enforce_web_token: bool, mode: str) -> Dict[str, Any]:
    public_url = str(os.environ.get("CCCC_WEB_PUBLIC_URL") or "").strip()
    host = str(os.environ.get("CCCC_WEB_HOST") or "").strip() or "127.0.0.1"
    port = _safe_web_port()
    web_bind_loopback = _is_loopback_host(host)
    web_bind_reachable = bool(public_url) or (not web_bind_loopback)
    return {
        "web_token_present": bool(str(os.environ.get("CCCC_WEB_TOKEN") or "").strip()) if enforce_web_token else True,
        "web_host": host,
        "web_port": int(port),
        "web_public_url": (public_url or None),
        "web_bind_loopback": bool(web_bind_loopback),
        "web_bind_reachable": bool(web_bind_reachable),
        "mode_supported": bool(_mode_supported(mode)),
        "provider": provider,
    }


def _remote_next_steps(
    *,
    provider: str,
    status: str,
    diagnostics: Dict[str, Any],
) -> list[str]:
    out: list[str] = []
    if provider == "off":
        out.append("Choose provider=manual or provider=tailscale, then click Save.")
        return out
    if not bool(diagnostics.get("mode_supported")):
        out.append("Set remote access mode to tailnet_only.")
    if not bool(diagnostics.get("web_token_present")):
        out.append("Set CCCC_WEB_TOKEN before enabling remote access.")
    if not bool(diagnostics.get("web_bind_reachable")):
        out.append("Set CCCC_WEB_HOST to a non-loopback address (or set CCCC_WEB_PUBLIC_URL).")
    if provider == "tailscale":
        if status == "not_installed":
            out.append("Install Tailscale and make sure 'tailscale' is in PATH.")
        elif status == "not_authenticated":
            out.append("Run 'tailscale up' to authenticate this machine.")
        elif status == "error":
            out.append("Run 'tailscale status --json' and fix reported errors.")
    if status == "stopped":
        out.append("Click Start after configuration checks pass.")
    return out


def _remote_unreachable_error(*, provider: str, diagnostics: Dict[str, Any]) -> DaemonResponse:
    host = str(diagnostics.get("web_host") or "")
    port = int(diagnostics.get("web_port") or 8848)
    public_url = str(diagnostics.get("web_public_url") or "").strip()
    if provider == "tailscale":
        msg = (
            "web server binding is not reachable from tailnet "
            "(set CCCC_WEB_HOST to a non-loopback address or set CCCC_WEB_PUBLIC_URL)"
        )
    else:
        msg = "web server binding is not remotely reachable (set CCCC_WEB_HOST or CCCC_WEB_PUBLIC_URL)"
    return _error(
        "remote_access_unreachable",
        msg,
        details={
            "provider": provider,
            "web_host": host,
            "web_port": port,
            "web_public_url": (public_url or None),
            "allow_loopback_override_env": "CCCC_REMOTE_ALLOW_LOOPBACK=1",
        },
    )


def _remote_access_state_payload(cfg: Dict[str, Any]) -> Dict[str, Any]:
    provider = str(cfg.get("provider") or "off").strip().lower()
    mode = _normalize_mode(cfg.get("mode"))
    enforce_web_token = coerce_bool(cfg.get("enforce_web_token"), default=True)
    enabled = coerce_bool(cfg.get("enabled"), default=False)
    updated_at = str(cfg.get("updated_at") or "").strip()

    status = "stopped"
    endpoint: Optional[str] = None
    diagnostics = _web_binding_diagnostics(provider=provider, enforce_web_token=enforce_web_token, mode=mode)
    tailscale_backend_state: Optional[str] = None
    tailscale_installed: Optional[bool] = None

    if provider == "manual":
        status = "running" if enabled else "stopped"
        if status == "running":
            endpoint = _manual_endpoint()
    elif provider == "tailscale":
        tailscale_installed = _tailscale_installed()
        if not tailscale_installed:
            status = "not_installed"
        else:
            ts_status, _err = _tailscale_status_json()
            if ts_status is None:
                status = "error"
            else:
                backend = _tailscale_backend_state(ts_status)
                tailscale_backend_state = backend
                if backend in ("running",):
                    status = "running"
                    endpoint = _tailscale_endpoint(ts_status)
                elif backend in ("needslogin", "needsmachineauth", "loginrequired", "loggedout"):
                    status = "not_authenticated"
                elif backend in ("starting", "stopped", ""):
                    status = "stopped"
                else:
                    status = "error"
    else:
        provider = "off"
        status = "stopped"
        enabled = False

    if provider in ("manual", "tailscale") and enabled:
        if not bool(diagnostics.get("mode_supported")):
            status = "misconfigured"
        elif not bool(diagnostics.get("web_bind_reachable")) and not _allow_loopback_remote():
            status = "misconfigured"

    diagnostics["tailscale_installed"] = tailscale_installed
    diagnostics["tailscale_backend_state"] = tailscale_backend_state

    return {
        "remote_access": {
            "provider": provider,
            "mode": mode,
            "enforce_web_token": bool(enforce_web_token),
            "enabled": bool(enabled),
            "status": status,
            "endpoint": endpoint,
            "updated_at": (updated_at or None),
            "diagnostics": diagnostics,
            "next_steps": _remote_next_steps(provider=provider, status=status, diagnostics=diagnostics),
        }
    }


def _require_user(by: str) -> Optional[DaemonResponse]:
    if by and by != "user":
        return _error("permission_denied", "only user can manage remote access")
    return None


def _validate_secure_defaults(*, provider: str, enforce_web_token: bool) -> Optional[DaemonResponse]:
    if provider != "off" and not enforce_web_token and not _allow_insecure_remote():
        return _error(
            "remote_access_invalid_config",
            "enforce_web_token=false is blocked by default; set CCCC_REMOTE_ALLOW_INSECURE=1 to override",
        )
    return None


def _validate_mode_or_error(mode: str) -> Optional[DaemonResponse]:
    if _mode_supported(mode):
        return None
    return _error(
        "remote_access_invalid_config",
        "unsupported remote access mode",
        details={"mode": mode, "supported_modes": sorted(_supported_modes())},
    )


def handle_remote_access_state(args: Dict[str, Any]) -> DaemonResponse:
    _ = str(args.get("by") or "user").strip()  # reserved for future policy
    cfg = get_remote_access_settings()
    return DaemonResponse(ok=True, result=_remote_access_state_payload(cfg))


def handle_remote_access_configure(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    denied = _require_user(by)
    if denied is not None:
        return denied

    patch: Dict[str, Any] = {}
    if "provider" in args:
        patch["provider"] = str(args.get("provider") or "").strip().lower()
    if "mode" in args:
        patch["mode"] = _normalize_mode(args.get("mode"))
    if "enforce_web_token" in args:
        patch["enforce_web_token"] = coerce_bool(args.get("enforce_web_token"), default=True)
    if "enabled" in args:
        patch["enabled"] = coerce_bool(args.get("enabled"), default=False)
    if not patch:
        cfg = get_remote_access_settings()
        return DaemonResponse(ok=True, result=_remote_access_state_payload(cfg))

    current = get_remote_access_settings()
    provider = str(patch.get("provider") or current.get("provider") or "off").strip().lower()
    mode = _normalize_mode(patch.get("mode") if "mode" in patch else current.get("mode"))
    enforce_web_token = coerce_bool(
        patch.get("enforce_web_token") if "enforce_web_token" in patch else current.get("enforce_web_token"),
        default=True,
    )
    invalid_mode = _validate_mode_or_error(mode)
    if invalid_mode is not None:
        return invalid_mode
    invalid = _validate_secure_defaults(provider=provider, enforce_web_token=enforce_web_token)
    if invalid is not None:
        return invalid

    patch["mode"] = mode
    patch["updated_at"] = utc_now_iso()
    cfg = update_remote_access_settings(patch)
    publish_event(
        "remote_access.configure",
        {
            "by": by or "user",
            "provider": cfg.get("provider"),
            "mode": cfg.get("mode"),
            "enforce_web_token": bool(cfg.get("enforce_web_token")),
            "enabled": bool(cfg.get("enabled")),
        },
    )
    return DaemonResponse(ok=True, result=_remote_access_state_payload(cfg))


def handle_remote_access_start(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    denied = _require_user(by)
    if denied is not None:
        return denied

    cfg = get_remote_access_settings()
    provider = str(cfg.get("provider") or "off").strip().lower()
    mode = _normalize_mode(cfg.get("mode"))
    enforce_web_token = coerce_bool(cfg.get("enforce_web_token"), default=True)

    invalid_mode = _validate_mode_or_error(mode)
    if invalid_mode is not None:
        return invalid_mode
    invalid = _validate_secure_defaults(provider=provider, enforce_web_token=enforce_web_token)
    if invalid is not None:
        return invalid
    if provider == "off":
        return _error("remote_access_invalid_config", "remote access provider is off")
    if enforce_web_token and not str(os.environ.get("CCCC_WEB_TOKEN") or "").strip():
        return _error("remote_access_invalid_config", "CCCC_WEB_TOKEN is required when enforce_web_token=true")
    diagnostics = _web_binding_diagnostics(provider=provider, enforce_web_token=enforce_web_token, mode=mode)
    if not bool(diagnostics.get("web_bind_reachable")) and not _allow_loopback_remote():
        return _remote_unreachable_error(provider=provider, diagnostics=diagnostics)

    if provider == "tailscale":
        if not _tailscale_installed():
            return _error("remote_access_not_installed", "tailscale is not installed")
        code, out, err = _run_command(["tailscale", "up"], timeout_s=30.0)
        if code != 0:
            msg = str(err or out or "tailscale up failed").strip()
            lower = msg.lower()
            if any(token in lower for token in ("login", "auth", "authenticate", "machine auth")):
                return _error("remote_access_not_authenticated", msg)
            return _error("remote_access_start_failed", msg)
        ts_status, ts_err = _tailscale_status_json()
        if ts_status is None:
            return _error("remote_access_start_failed", ts_err or "failed to verify tailscale status")
        backend = _tailscale_backend_state(ts_status)
        if backend in ("needslogin", "needsmachineauth", "loginrequired", "loggedout"):
            return _error("remote_access_not_authenticated", "tailscale authentication is required")
        if backend not in ("running",):
            return _error(
                "remote_access_start_failed",
                "tailscale backend is not running",
                details={"backend_state": backend},
            )

    cfg = update_remote_access_settings({"enabled": True, "updated_at": utc_now_iso()})
    publish_event(
        "remote_access.start",
        {
            "by": by or "user",
            "provider": cfg.get("provider"),
            "enabled": bool(cfg.get("enabled")),
        },
    )
    return DaemonResponse(ok=True, result=_remote_access_state_payload(cfg))


def handle_remote_access_stop(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    denied = _require_user(by)
    if denied is not None:
        return denied

    cfg = get_remote_access_settings()
    provider = str(cfg.get("provider") or "off").strip().lower()

    if provider == "tailscale" and _tailscale_installed():
        code, out, err = _run_command(["tailscale", "down"], timeout_s=20.0)
        if code != 0:
            return _error("remote_access_stop_failed", str(err or out or "tailscale down failed").strip())

    cfg = update_remote_access_settings({"enabled": False, "updated_at": utc_now_iso()})
    publish_event(
        "remote_access.stop",
        {
            "by": by or "user",
            "provider": cfg.get("provider"),
            "enabled": bool(cfg.get("enabled")),
        },
    )
    return DaemonResponse(ok=True, result=_remote_access_state_payload(cfg))


def try_handle_remote_access_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "remote_access_state":
        return handle_remote_access_state(args)
    if op == "remote_access_configure":
        return handle_remote_access_configure(args)
    if op == "remote_access_start":
        return handle_remote_access_start(args)
    if op == "remote_access_stop":
        return handle_remote_access_stop(args)
    return None
