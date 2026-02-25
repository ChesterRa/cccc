from __future__ import annotations

import json
import os
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


def notebooklm_health_check(
    auth_json_raw: str | None = None,
    *,
    real_enabled: bool | None = None,
) -> Dict[str, Any]:
    enabled = notebooklm_real_enabled() if real_enabled is None else bool(real_enabled)
    if not enabled:
        raise NotebookLMProviderError(
            code="space_provider_not_configured",
            message="NotebookLM real adapter is disabled",
            transient=False,
            degrade_provider=True,
        )
    _ = validate_notebooklm_auth_json(auth_json_raw)
    compat = probe_notebooklm_vendor()
    if not compat.compatible:
        raise NotebookLMProviderError(
            code="space_provider_compat_mismatch",
            message=compat.reason,
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "enabled": True,
        "compatible": True,
        "reason": "ok",
    }
