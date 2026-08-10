from __future__ import annotations

import asyncio
import json
import os
import threading
from typing import Any, Dict

from .compat import probe_notebooklm_vendor
from .errors import NotebookLMProviderError


def _truthy_env(name: str) -> bool:
    value = str(os.environ.get(name) or "").strip().lower()
    return value in {"1", "true", "yes", "y", "on"}


def notebooklm_real_enabled() -> bool:
    return _truthy_env("CCCC_NOTEBOOKLM_REAL")


def parse_notebooklm_auth_json(raw: str, *, label: str = "CCCC_NOTEBOOKLM_AUTH_JSON") -> Dict[str, Any]:
    text = str(raw or "").strip()
    if not text:
        raise NotebookLMProviderError(
            code="space_provider_not_configured",
            message=f"missing {label}",
            transient=False,
            degrade_provider=True,
        )
    try:
        payload = json.loads(text)
    except Exception as e:
        raise NotebookLMProviderError(
            code="space_provider_auth_invalid",
            message=f"invalid {label}: {e}",
            transient=False,
            degrade_provider=True,
        ) from e
    if not isinstance(payload, dict):
        raise NotebookLMProviderError(
            code="space_provider_auth_invalid",
            message=f"{label} must be a JSON object",
            transient=False,
            degrade_provider=True,
        )
    cookies = payload.get("cookies")
    if not isinstance(cookies, list) or not cookies:
        raise NotebookLMProviderError(
            code="space_provider_auth_invalid",
            message=f"{label} missing cookies array",
            transient=False,
            degrade_provider=True,
        )
    return payload


def validate_notebooklm_auth_json(auth_json_raw: str | None = None) -> Dict[str, Any]:
    raw = str(auth_json_raw or "").strip()
    if raw:
        return parse_notebooklm_auth_json(raw, label="NOTEBOOKLM_AUTH_JSON")
    raw = str(os.environ.get("CCCC_NOTEBOOKLM_AUTH_JSON") or "").strip()
    if not raw:
        raise NotebookLMProviderError(
            code="space_provider_not_configured",
            message="missing CCCC_NOTEBOOKLM_AUTH_JSON",
            transient=False,
            degrade_provider=True,
        )
    return parse_notebooklm_auth_json(raw, label="CCCC_NOTEBOOKLM_AUTH_JSON")


def _run_coroutine_sync(coro: Any) -> Any:
    result_holder: Dict[str, Any] = {}
    error_holder: Dict[str, BaseException] = {}

    def _runner() -> None:
        try:
            result_holder["value"] = asyncio.run(coro)
        except BaseException as e:  # pragma: no cover - exercised by live provider failures
            error_holder["error"] = e

    thread = threading.Thread(target=_runner, daemon=True)
    thread.start()
    thread.join()
    if "error" in error_holder:
        raise error_holder["error"]
    return result_holder.get("value")


def verify_notebooklm_storage_state(storage_state: Dict[str, Any]) -> None:
    """Passively verify one in-memory Playwright state without discarding cookie scope."""
    from ._vendor.notebooklm._auth.refresh import _fetch_tokens_with_jar
    from ._vendor.notebooklm.auth import build_cookie_jar, extract_cookies_with_domains

    cookies = extract_cookies_with_domains(storage_state)
    cookie_jar = build_cookie_jar(cookies=cookies)
    authuser_raw = storage_state.get("authuser")
    explicit_authuser = isinstance(authuser_raw, int) and authuser_raw >= 0
    authuser = authuser_raw if explicit_authuser else 0
    _ = _run_coroutine_sync(
        _fetch_tokens_with_jar(
            cookie_jar,
            None,
            authuser=authuser,
            force_authuser_query=explicit_authuser,
            poke=False,
        )
    )


def notebooklm_health_check(
    auth_json_raw: str | None = None,
    *,
    real_enabled: bool | None = None,
    verify_remote: bool = False,
) -> Dict[str, Any]:
    explicit_auth = bool(str(auth_json_raw or "").strip())
    enabled = notebooklm_real_enabled() if real_enabled is None else bool(real_enabled)
    if explicit_auth:
        enabled = True
    if not enabled:
        raise NotebookLMProviderError(
            code="space_provider_not_configured",
            message="NotebookLM real adapter is disabled",
            transient=False,
            degrade_provider=True,
        )
    auth_payload = validate_notebooklm_auth_json(auth_json_raw)
    compat = probe_notebooklm_vendor()
    if not compat.compatible:
        raise NotebookLMProviderError(
            code="space_provider_compat_mismatch",
            message=compat.reason,
            transient=False,
            degrade_provider=True,
        )
    if verify_remote:
        try:
            verify_notebooklm_storage_state(auth_payload)
        except NotebookLMProviderError:
            raise
        except Exception as e:
            raise NotebookLMProviderError(
                code="space_provider_auth_invalid",
                message=str(e) or "NotebookLM session validation failed",
                transient=False,
                degrade_provider=True,
            ) from e
    return {
        "provider": "notebooklm",
        "enabled": True,
        "compatible": True,
        "reason": "ok",
    }
