"""Codex runtime command compatibility helpers."""

from __future__ import annotations

import json
import ntpath
from pathlib import Path
from typing import Any, Dict


_CODEX_PROVIDER_CONFIG_KEYS = (
    "openai_base_url",
    "model_provider",
    "model_providers",
)


def codex_command_stem(command: str) -> str:
    raw = str(command or "").strip()
    if not raw:
        return ""
    try:
        return str(Path(ntpath.basename(raw)).stem or "").strip().lower()
    except Exception:
        return raw.lower()


def codex_command_has_config_key(command: list[str], keys: tuple[str, ...] = _CODEX_PROVIDER_CONFIG_KEYS) -> bool:
    wanted = tuple(str(key or "").strip() for key in keys if str(key or "").strip())
    if not wanted:
        return False
    items = [str(item or "").strip() for item in list(command or [])]
    i = 1
    while i < len(items):
        arg = items[i]
        value = ""
        if arg in {"-c", "--config"}:
            if i + 1 < len(items):
                value = items[i + 1]
            i += 2
        elif arg.startswith("--config="):
            value = arg.split("=", 1)[1]
            i += 1
        else:
            i += 1
            continue
        config_key = value.split("=", 1)[0].strip()
        if any(config_key == key or config_key.startswith(f"{key}.") for key in wanted):
            return True
    return False


def inject_codex_openai_base_url_config(command: list[str], env: Dict[str, Any] | None) -> list[str]:
    cmd = [str(item) for item in list(command or []) if str(item).strip()]
    if not cmd or codex_command_stem(cmd[0]) != "codex":
        return cmd
    base_url = str((env or {}).get("OPENAI_BASE_URL") or "").strip()
    if not base_url:
        return cmd
    if codex_command_has_config_key(cmd):
        return cmd
    config_arg = f"openai_base_url={json.dumps(base_url, ensure_ascii=True)}"
    return [cmd[0], "-c", config_arg, *cmd[1:]]
