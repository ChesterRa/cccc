"""Canonical IM config normalization helpers.

This module keeps IM config shape consistent across Web/CLI/bridge paths.
"""

from __future__ import annotations

import re
from typing import Any, Dict, Set

from ...util.conv import coerce_bool

SUPPORTED_IM_PLATFORMS: Set[str] = {"telegram", "slack", "discord", "feishu", "dingtalk", "wecom", "weixin"}

_LEGACY_KEYS: Set[str] = {
    "token_env",
    "token",
    "bot_token_env",
    "bot_token",
    "app_token_env",
    "app_token",
    "feishu_domain",
    "feishu_app_id",
    "feishu_app_id_env",
    "feishu_app_secret",
    "feishu_app_secret_env",
    "dingtalk_app_key",
    "dingtalk_app_key_env",
    "dingtalk_app_secret",
    "dingtalk_app_secret_env",
    "dingtalk_robot_code",
    "dingtalk_robot_code_env",
    "wecom_bot_id",
    "wecom_bot_id_env",
    "wecom_secret",
    "wecom_secret_env",
    "weixin_command",
}


def is_env_var_name(value: str) -> bool:
    return bool(re.fullmatch(r"[A-Z_][A-Z0-9_]*", (value or "").strip()))


def normalize_feishu_domain(value: str) -> str:
    raw = str(value or "").strip()
    if not raw:
        return ""
    v = raw.lower().rstrip("/")
    if v.endswith("/open-apis"):
        v = v[: -len("/open-apis")].rstrip("/")
    if v in ("feishu", "cn", "china", "open.feishu.cn", "https://open.feishu.cn"):
        return "https://open.feishu.cn"
    if v in (
        "lark",
        "global",
        "intl",
        "international",
        "open.larkoffice.com",
        "https://open.larkoffice.com",
        "open.larksuite.com",
        "https://open.larksuite.com",
    ):
        return "https://open.larkoffice.com"
    return "https://open.feishu.cn"


def _first_nonempty(raw: Dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = str(raw.get(key) or "").strip()
        if value:
            return value
    return ""


def _set_secret_ref(out: Dict[str, Any], *, env_key: str, value_key: str, raw_value: str) -> None:
    value = str(raw_value or "").strip()
    if not value:
        out.pop(env_key, None)
        out.pop(value_key, None)
        return
    if is_env_var_name(value):
        out[env_key] = value
        out.pop(value_key, None)
    else:
        out[value_key] = value
        out.pop(env_key, None)


def canonicalize_im_config(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return {}
    platform = str(raw.get("platform") or "").strip().lower()
    if platform not in SUPPORTED_IM_PLATFORMS:
        return {}

    out: Dict[str, Any] = {"platform": platform}
    if "enabled" in raw:
        out["enabled"] = coerce_bool(raw.get("enabled"), default=False)
    files = raw.get("files")
    if isinstance(files, dict):
        out["files"] = dict(files)
    if "skip_pending_on_start" in raw:
        out["skip_pending_on_start"] = coerce_bool(raw.get("skip_pending_on_start"), default=True)

    if platform in {"telegram", "discord", "slack"}:
        bot_ref = _first_nonempty(raw, "bot_token_env", "bot_token", "token_env", "token")
        _set_secret_ref(out, env_key="bot_token_env", value_key="bot_token", raw_value=bot_ref)
        if platform == "slack":
            app_ref = _first_nonempty(raw, "app_token_env", "app_token")
            _set_secret_ref(out, env_key="app_token_env", value_key="app_token", raw_value=app_ref)
    elif platform == "feishu":
        domain = normalize_feishu_domain(str(raw.get("feishu_domain") or ""))
        if domain:
            out["feishu_domain"] = domain
        _set_secret_ref(
            out,
            env_key="feishu_app_id_env",
            value_key="feishu_app_id",
            raw_value=_first_nonempty(raw, "feishu_app_id_env", "feishu_app_id"),
        )
        _set_secret_ref(
            out,
            env_key="feishu_app_secret_env",
            value_key="feishu_app_secret",
            raw_value=_first_nonempty(raw, "feishu_app_secret_env", "feishu_app_secret"),
        )
    elif platform == "dingtalk":
        _set_secret_ref(
            out,
            env_key="dingtalk_app_key_env",
            value_key="dingtalk_app_key",
            raw_value=_first_nonempty(raw, "dingtalk_app_key_env", "dingtalk_app_key"),
        )
        _set_secret_ref(
            out,
            env_key="dingtalk_app_secret_env",
            value_key="dingtalk_app_secret",
            raw_value=_first_nonempty(raw, "dingtalk_app_secret_env", "dingtalk_app_secret"),
        )
        _set_secret_ref(
            out,
            env_key="dingtalk_robot_code_env",
            value_key="dingtalk_robot_code",
            raw_value=_first_nonempty(raw, "dingtalk_robot_code_env", "dingtalk_robot_code"),
        )
    elif platform == "wecom":
        _set_secret_ref(
            out,
            env_key="wecom_bot_id_env",
            value_key="wecom_bot_id",
            raw_value=_first_nonempty(raw, "wecom_bot_id_env", "wecom_bot_id"),
        )
        _set_secret_ref(
            out,
            env_key="wecom_secret_env",
            value_key="wecom_secret",
            raw_value=_first_nonempty(raw, "wecom_secret_env", "wecom_secret"),
        )
    # Preserve non-credential extension fields (forward-compatible).
    for key, value in raw.items():
        if key in out:
            continue
        if key in _LEGACY_KEYS:
            continue
        if key in {"platform", "enabled", "files", "skip_pending_on_start", "wecom_agent_id"}:
            continue
        out[key] = value

    return out
