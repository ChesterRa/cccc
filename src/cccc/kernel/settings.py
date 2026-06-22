"""Global settings management for CCCC.

Settings are stored in ~/.cccc/settings.yaml and include:
- runtime_pool: Prioritized list of agent runtimes with scenarios
- Other global preferences
"""
from __future__ import annotations

import logging
import os
import copy
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml  # type: ignore

from ..paths import ensure_home
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso

_SETTINGS_CACHE_LOCK = threading.Lock()
_SETTINGS_CACHE_KEY: tuple[str, int, int] | None = None
_SETTINGS_CACHE_DOC: Dict[str, Any] | None = None


@dataclass
class RuntimePoolEntry:
    """An entry in the runtime pool."""
    runtime: str
    priority: int
    scenarios: List[str] = field(default_factory=list)
    notes: str = ""
    
    def to_dict(self) -> Dict[str, Any]:
        return {
            "runtime": self.runtime,
            "priority": self.priority,
            "scenarios": self.scenarios,
            "notes": self.notes,
        }
    
    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RuntimePoolEntry":
        try:
            priority = int(d.get("priority") or 999)
        except Exception:
            priority = 999
        scenarios_raw = d.get("scenarios")
        scenarios: List[str] = []
        if isinstance(scenarios_raw, list):
            scenarios = [str(s).strip() for s in scenarios_raw if isinstance(s, str) and str(s).strip()]
        return cls(
            runtime=str(d.get("runtime") or ""),
            priority=priority,
            scenarios=scenarios,
            notes=str(d.get("notes") or ""),
        )


# Default runtime pool (used if no settings.yaml exists)
DEFAULT_RUNTIME_POOL: List[RuntimePoolEntry] = [
    RuntimePoolEntry(
        runtime="claude",
        priority=1,
        scenarios=["coding", "review", "planning", "general"],
        notes="Strong coding; MCP support; good for complex tasks",
    ),
    RuntimePoolEntry(
        runtime="codex",
        priority=2,
        scenarios=["coding", "refactoring", "sandbox"],
        notes="Good sandbox support; suitable for risky operations",
    ),
    RuntimePoolEntry(
        runtime="droid",
        priority=3,
        scenarios=["coding", "long-session"],
        notes="Robust auto mode; good for long tasks",
    ),
    RuntimePoolEntry(
        runtime="amp",
        priority=4,
        scenarios=["coding", "review", "general"],
        notes="Amp CLI; MCP support",
    ),
    RuntimePoolEntry(
        runtime="auggie",
        priority=5,
        scenarios=["coding", "general"],
        notes="Auggie (Augment CLI); MCP support",
    ),
    RuntimePoolEntry(
        runtime="kimi",
        priority=7,
        scenarios=["coding", "general"],
        notes="Kimi CLI; MCP support",
    ),
]


def _copy_runtime_pool(pool: List[RuntimePoolEntry]) -> List[RuntimePoolEntry]:
    return [
        RuntimePoolEntry(
            runtime=str(entry.runtime),
            priority=int(entry.priority),
            scenarios=list(entry.scenarios),
            notes=str(entry.notes),
        )
        for entry in pool
    ]

# ---------------------------------------------------------------------------
# Observability / Developer mode (global)
# ---------------------------------------------------------------------------

DEFAULT_OBSERVABILITY: Dict[str, Any] = {
    "developer_mode": False,
    # Keep log level conservative by default; developer mode may raise it.
    "log_level": "INFO",
    # Components are informational today; filtering can be implemented later.
    "components": ["daemon", "web", "delivery", "im", "pty", "mcp"],
    # Per-logger overrides let us keep local diagnostics without turning on
    # root-level DEBUG for every third-party dependency.
    "logger_levels": {},
    # Terminal transcript is captured in-memory only (no persistence) by default.
    "terminal_transcript": {
        "enabled": False,
        # 10 MiB per actor helps with full-screen TUIs while staying bounded.
        "per_actor_bytes": 10 * 1024 * 1024,
        "persist": False,
        "strip_ansi": True,
    },
    # Web terminal UI preferences (global).
    "terminal_ui": {
        "scrollback_lines": 8000,
    },
    # Runtime surfaces exposed in the standard Web UI.
    "runtime_visibility": {
        "peer_runtime": "visible",
        "assistant_runtime": "hidden",
    },
}


# ---------------------------------------------------------------------------
# Remote access (global)
# ---------------------------------------------------------------------------

DEFAULT_REMOTE_ACCESS: Dict[str, Any] = {
    "provider": "off",          # off | manual | tailscale
    "mode": "tailnet_only",     # reserved for future extension
    "require_access_token": True,  # security-first default
    "enabled": False,           # desired state for provider control
    # Optional Web binding/token overrides (empty means env/default fallback).
    "web_host": "",             # e.g. 192.168.1.20
    "web_port": 8848,            # 1..65535
    "web_public_url": "",       # e.g. https://cccc.example.com/ui/
    "updated_at": "",           # RFC3339 UTC timestamp (best-effort)
}


# ---------------------------------------------------------------------------
# Web branding (global)
# ---------------------------------------------------------------------------

DEFAULT_WEB_BRANDING: Dict[str, Any] = {
    "product_name": "CCCC",
    "logo_icon_asset_path": "",
    "favicon_asset_path": "",
    "updated_at": "",
}


def _as_bool(v: Any, default: bool) -> bool:
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return bool(v)
    if isinstance(v, str):
        s = v.strip().lower()
        if s in ("1", "true", "yes", "y", "on"):
            return True
        if s in ("0", "false", "no", "n", "off"):
            return False
    return default


def _as_int(v: Any, default: int, *, min_value: int = 0, max_value: Optional[int] = None) -> int:
    try:
        n = int(v)
    except Exception:
        n = int(default)
    if n < min_value:
        n = min_value
    if max_value is not None and n > max_value:
        n = max_value
    return n


def _as_str(v: Any, default: str) -> str:
    s = str(v).strip() if v is not None else ""
    return s or default


def _as_log_level_name(v: Any, default: str) -> str:
    s = str(v or "").strip().upper()
    if not s:
        return default
    level = getattr(logging, s, None)
    if isinstance(level, int):
        return s
    return default


def _as_runtime_visibility(v: Any, default: str) -> str:
    s = str(v or "").strip().lower()
    if s in {"hidden", "visible"}:
        return s
    return default


def _merge_observability(raw: Any) -> Dict[str, Any]:
    """Merge/validate observability settings with defaults."""
    base = dict(DEFAULT_OBSERVABILITY)
    if not isinstance(raw, dict):
        return base

    base["developer_mode"] = _as_bool(raw.get("developer_mode"), bool(base["developer_mode"]))
    base["log_level"] = _as_log_level_name(raw.get("log_level"), str(base["log_level"]))

    comps = raw.get("components")
    if isinstance(comps, list) and comps:
        base["components"] = [str(x).strip() for x in comps if str(x).strip()]

    logger_levels_raw = raw.get("logger_levels")
    logger_levels: Dict[str, str] = {}
    if isinstance(logger_levels_raw, dict):
        for name, level in logger_levels_raw.items():
            logger_name = str(name or "").strip()
            if not logger_name:
                continue
            normalized = _as_log_level_name(level, "")
            if normalized:
                logger_levels[logger_name] = normalized
    base["logger_levels"] = logger_levels

    tt = raw.get("terminal_transcript")
    tt_base = dict(DEFAULT_OBSERVABILITY["terminal_transcript"])
    if isinstance(tt, dict):
        tt_base["enabled"] = _as_bool(tt.get("enabled"), bool(tt_base["enabled"]))
        tt_base["per_actor_bytes"] = _as_int(tt.get("per_actor_bytes"), int(tt_base["per_actor_bytes"]), min_value=0)
        tt_base["persist"] = _as_bool(tt.get("persist"), bool(tt_base["persist"]))
        tt_base["strip_ansi"] = _as_bool(tt.get("strip_ansi"), bool(tt_base["strip_ansi"]))
    base["terminal_transcript"] = tt_base

    tui = raw.get("terminal_ui")
    tui_base = dict(DEFAULT_OBSERVABILITY["terminal_ui"])
    if isinstance(tui, dict):
        tui_base["scrollback_lines"] = _as_int(
            tui.get("scrollback_lines"),
            int(tui_base["scrollback_lines"]),
            min_value=1000,
            max_value=200_000,
        )
    base["terminal_ui"] = tui_base

    runtime_visibility = raw.get("runtime_visibility")
    runtime_visibility_base = dict(DEFAULT_OBSERVABILITY["runtime_visibility"])
    if isinstance(runtime_visibility, dict):
        runtime_visibility_base["peer_runtime"] = _as_runtime_visibility(
            runtime_visibility.get("peer_runtime"),
            str(runtime_visibility_base["peer_runtime"]),
        )
        assistant_runtime = runtime_visibility.get("assistant_runtime")
        if assistant_runtime is None:
            # Legacy <= 0.4.25 key; preserve existing built-in assistant visibility preferences.
            assistant_runtime = runtime_visibility.get("pet_runtime")
        runtime_visibility_base["assistant_runtime"] = _as_runtime_visibility(
            assistant_runtime,
            str(runtime_visibility_base["assistant_runtime"]),
        )
    base["runtime_visibility"] = runtime_visibility_base

    return base


def _merge_remote_access(raw: Any) -> Dict[str, Any]:
    """Merge/validate remote access settings with defaults."""
    base = dict(DEFAULT_REMOTE_ACCESS)
    if not isinstance(raw, dict):
        return base

    provider = _as_str(raw.get("provider"), str(base["provider"])).lower()
    if provider not in ("off", "manual", "tailscale"):
        provider = "off"
    base["provider"] = provider

    mode = _as_str(raw.get("mode"), str(base["mode"]))
    base["mode"] = mode or str(DEFAULT_REMOTE_ACCESS["mode"])

    base["require_access_token"] = _as_bool(raw.get("require_access_token"), bool(base["require_access_token"]))
    base["enabled"] = _as_bool(raw.get("enabled"), bool(base["enabled"]))
    base["web_host"] = str(raw.get("web_host") or "").strip()
    base["web_port"] = _as_int(raw.get("web_port"), int(base["web_port"]), min_value=1, max_value=65535)
    base["web_public_url"] = str(raw.get("web_public_url") or "").strip()
    base["updated_at"] = _as_str(raw.get("updated_at"), str(base["updated_at"]))

    if base["provider"] == "off":
        base["enabled"] = False

    return base


def _merge_web_branding(raw: Any) -> Dict[str, Any]:
    """Merge/validate web branding settings with defaults."""
    base = dict(DEFAULT_WEB_BRANDING)
    if not isinstance(raw, dict):
        return base

    product_name = str(raw.get("product_name") or "").strip()
    base["product_name"] = product_name or str(DEFAULT_WEB_BRANDING["product_name"])
    base["logo_icon_asset_path"] = str(raw.get("logo_icon_asset_path") or "").strip()
    base["favicon_asset_path"] = str(raw.get("favicon_asset_path") or "").strip()
    base["updated_at"] = _as_str(raw.get("updated_at"), str(base["updated_at"]))
    return base


def get_observability_settings() -> Dict[str, Any]:
    """Get merged observability settings (global)."""
    settings = load_settings()
    return _merge_observability(settings.get("observability"))


def update_observability_settings(patch: Dict[str, Any]) -> Dict[str, Any]:
    """Update observability settings in ~/.cccc/settings.yaml and return merged result."""
    settings = load_settings()
    current = _merge_observability(settings.get("observability"))
    if not isinstance(patch, dict):
        return current

    merged = dict(current)
    if "developer_mode" in patch:
        merged["developer_mode"] = _as_bool(patch.get("developer_mode"), bool(merged["developer_mode"]))
    if "log_level" in patch:
        merged["log_level"] = _as_log_level_name(patch.get("log_level"), str(merged["log_level"]))
    if "components" in patch:
        comps = patch.get("components")
        if isinstance(comps, list):
            merged["components"] = [str(x).strip() for x in comps if str(x).strip()]
    if "logger_levels" in patch:
        logger_levels_patch = patch.get("logger_levels")
        logger_levels: Dict[str, str] = {}
        if isinstance(logger_levels_patch, dict):
            for name, level in logger_levels_patch.items():
                logger_name = str(name or "").strip()
                if not logger_name:
                    continue
                normalized = _as_log_level_name(level, "")
                if normalized:
                    logger_levels[logger_name] = normalized
        merged["logger_levels"] = logger_levels
    if "terminal_transcript" in patch:
        tt_patch = patch.get("terminal_transcript")
        if isinstance(tt_patch, dict):
            tt = dict(merged.get("terminal_transcript") or {})
            if "enabled" in tt_patch:
                tt["enabled"] = _as_bool(tt_patch.get("enabled"), bool(tt.get("enabled", False)))
            if "per_actor_bytes" in tt_patch:
                tt["per_actor_bytes"] = _as_int(tt_patch.get("per_actor_bytes"), int(tt.get("per_actor_bytes", 0)), min_value=0)
            if "persist" in tt_patch:
                tt["persist"] = _as_bool(tt_patch.get("persist"), bool(tt.get("persist", False)))
            if "strip_ansi" in tt_patch:
                tt["strip_ansi"] = _as_bool(tt_patch.get("strip_ansi"), bool(tt.get("strip_ansi", True)))
            merged["terminal_transcript"] = tt
    if "terminal_ui" in patch:
        tui_patch = patch.get("terminal_ui")
        if isinstance(tui_patch, dict):
            tui = dict(merged.get("terminal_ui") or {})
            if "scrollback_lines" in tui_patch:
                tui["scrollback_lines"] = _as_int(
                    tui_patch.get("scrollback_lines"),
                    int(tui.get("scrollback_lines", 8000)),
                    min_value=1000,
                    max_value=200_000,
                )
            merged["terminal_ui"] = tui
    if "runtime_visibility" in patch:
        runtime_visibility_patch = patch.get("runtime_visibility")
        if isinstance(runtime_visibility_patch, dict):
            runtime_visibility = dict(merged.get("runtime_visibility") or {})
            if "peer_runtime" in runtime_visibility_patch:
                runtime_visibility["peer_runtime"] = _as_runtime_visibility(
                    runtime_visibility_patch.get("peer_runtime"),
                    str(runtime_visibility.get("peer_runtime", "visible")),
                )
            if "assistant_runtime" in runtime_visibility_patch:
                runtime_visibility["assistant_runtime"] = _as_runtime_visibility(
                    runtime_visibility_patch.get("assistant_runtime"),
                    str(runtime_visibility.get("assistant_runtime", "hidden")),
                )
            merged["runtime_visibility"] = runtime_visibility

    settings["observability"] = merged
    save_settings(settings)
    return _merge_observability(merged)


def get_remote_access_settings() -> Dict[str, Any]:
    """Get merged remote access settings (global)."""
    settings = load_settings()
    return _merge_remote_access(settings.get("remote_access"))


def get_web_branding_settings() -> Dict[str, Any]:
    """Get merged web branding settings (global)."""
    settings = load_settings()
    return _merge_web_branding(settings.get("web_branding"))


def resolve_remote_access_web_binding() -> Dict[str, Any]:
    """Resolve effective Web binding with explicit settings/env/default precedence."""
    settings = load_settings()
    raw = settings.get("remote_access") if isinstance(settings.get("remote_access"), dict) else {}

    raw_host = str(raw.get("web_host") or "").strip()
    env_host = str(os.environ.get("CCCC_WEB_HOST") or "").strip()
    host_source = "settings" if raw_host else ("env" if env_host else "default")
    host = raw_host or env_host or "127.0.0.1"

    # Use raw persisted values here so we can still distinguish "unset" from the
    # merged default and preserve env fallback behavior.
    raw_port = raw.get("web_port")
    raw_port_s = str(raw_port or "").strip()
    env_port_raw = os.environ.get("CCCC_WEB_PORT")
    env_port_s = str(env_port_raw or "").strip()
    if raw_port_s:
        port = _as_int(raw_port, int(DEFAULT_REMOTE_ACCESS["web_port"]), min_value=1, max_value=65535)
        port_source = "settings"
    elif env_port_s:
        port = _as_int(env_port_raw, int(DEFAULT_REMOTE_ACCESS["web_port"]), min_value=1, max_value=65535)
        port_source = "env"
    else:
        port = int(DEFAULT_REMOTE_ACCESS["web_port"])
        port_source = "default"

    raw_public_url = str(raw.get("web_public_url") or "").strip()
    env_public_url = str(os.environ.get("CCCC_WEB_PUBLIC_URL") or "").strip()
    public_url_source = "settings" if raw_public_url else ("env" if env_public_url else "none")
    public_url = raw_public_url or env_public_url

    return {
        "web_host": host,
        "web_host_source": host_source,
        "web_port": int(port),
        "web_port_source": port_source,
        "web_public_url": (public_url or None),
        "web_public_url_source": public_url_source,
    }


def update_remote_access_settings(patch: Dict[str, Any]) -> Dict[str, Any]:
    """Update remote access settings in ~/.cccc/settings.yaml and return merged result.

    Only persists fields that are explicitly in the patch (plus fields already
    present in the raw settings).  This avoids writing default values (e.g.
    web_port=8848) that would shadow env-var fallbacks later.
    """
    settings = load_settings()
    current = _merge_remote_access(settings.get("remote_access"))
    if not isinstance(patch, dict) or not patch:
        return current

    # Work on the RAW persisted dict, not the merged-with-defaults version.
    # This ensures we never persist a default value that wasn't explicitly set.
    raw = dict(settings.get("remote_access") if isinstance(settings.get("remote_access"), dict) else {})
    changed = False

    if "provider" in patch:
        provider = _as_str(patch.get("provider"), str(current["provider"])).lower()
        if provider not in ("off", "manual", "tailscale"):
            provider = "off"
        raw["provider"] = provider
        changed = True

    if "mode" in patch:
        mode = _as_str(patch.get("mode"), str(current["mode"]))
        raw["mode"] = mode or str(DEFAULT_REMOTE_ACCESS["mode"])
        changed = True

    if "require_access_token" in patch:
        raw["require_access_token"] = _as_bool(
            patch.get("require_access_token"),
            bool(current["require_access_token"]),
        )
        changed = True

    if "enabled" in patch:
        raw["enabled"] = _as_bool(patch.get("enabled"), bool(current["enabled"]))
        changed = True

    if "web_host" in patch:
        raw["web_host"] = str(patch.get("web_host") or "").strip()
        changed = True

    if "web_port" in patch:
        raw["web_port"] = _as_int(patch.get("web_port"), int(current.get("web_port") or 8848), min_value=1, max_value=65535)
        changed = True

    if "web_public_url" in patch:
        raw["web_public_url"] = str(patch.get("web_public_url") or "").strip()
        changed = True

    if "updated_at" in patch:
        raw["updated_at"] = _as_str(patch.get("updated_at"), str(raw.get("updated_at") or ""))
        changed = True
    elif changed:
        raw["updated_at"] = utc_now_iso()

    # Enforce provider=off => enabled=False on the raw dict before save.
    merged_provider = str(raw.get("provider") or current.get("provider") or "").strip().lower()
    if merged_provider == "off":
        raw["enabled"] = False

    settings["remote_access"] = raw
    save_settings(settings)
    return _merge_remote_access(raw)


def update_web_branding_settings(patch: Dict[str, Any]) -> Dict[str, Any]:
    """Update web branding settings in ~/.cccc/settings.yaml and return merged result."""
    settings = load_settings()
    current = _merge_web_branding(settings.get("web_branding"))
    if not isinstance(patch, dict) or not patch:
        return current

    raw = dict(settings.get("web_branding") if isinstance(settings.get("web_branding"), dict) else {})
    changed = False

    if "product_name" in patch:
        product_name = str(patch.get("product_name") or "").strip()
        raw["product_name"] = product_name or str(DEFAULT_WEB_BRANDING["product_name"])
        changed = True

    if "logo_icon_asset_path" in patch:
        raw["logo_icon_asset_path"] = str(patch.get("logo_icon_asset_path") or "").strip()
        changed = True

    if "favicon_asset_path" in patch:
        raw["favicon_asset_path"] = str(patch.get("favicon_asset_path") or "").strip()
        changed = True

    if "updated_at" in patch:
        raw["updated_at"] = _as_str(patch.get("updated_at"), str(raw.get("updated_at") or ""))
        changed = True
    elif changed:
        raw["updated_at"] = utc_now_iso()

    settings["web_branding"] = raw
    save_settings(settings)
    return _merge_web_branding(raw)


def _settings_path() -> Path:
    return ensure_home() / "settings.yaml"


def _clone_settings(settings: Dict[str, Any]) -> Dict[str, Any]:
    return copy.deepcopy(settings)


def _invalidate_settings_cache() -> None:
    global _SETTINGS_CACHE_KEY, _SETTINGS_CACHE_DOC
    with _SETTINGS_CACHE_LOCK:
        _SETTINGS_CACHE_KEY = None
        _SETTINGS_CACHE_DOC = None


def load_settings() -> Dict[str, Any]:
    """Load global settings from ~/.cccc/settings.yaml."""
    global _SETTINGS_CACHE_KEY, _SETTINGS_CACHE_DOC
    p = _settings_path()
    if not p.exists():
        return {}
    try:
        stat = p.stat()
        cache_key = (str(p), int(stat.st_mtime_ns), int(stat.st_size))
    except Exception:
        cache_key = None
    if cache_key is not None:
        with _SETTINGS_CACHE_LOCK:
            if _SETTINGS_CACHE_KEY == cache_key and _SETTINGS_CACHE_DOC is not None:
                return _clone_settings(_SETTINGS_CACHE_DOC)
    try:
        doc = yaml.safe_load(p.read_text(encoding="utf-8")) or {}
        out = doc if isinstance(doc, dict) else {}
    except Exception:
        return {}
    if cache_key is not None:
        with _SETTINGS_CACHE_LOCK:
            _SETTINGS_CACHE_KEY = cache_key
            _SETTINGS_CACHE_DOC = _clone_settings(out)
    return out


def save_settings(settings: Dict[str, Any]) -> None:
    """Save global settings to ~/.cccc/settings.yaml."""
    p = _settings_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_text(p, yaml.safe_dump(settings, allow_unicode=True, sort_keys=False))
    _invalidate_settings_cache()


def get_runtime_pool() -> List[RuntimePoolEntry]:
    """Get the runtime pool from settings, or default if not configured."""
    settings = load_settings()
    pool_raw = settings.get("runtime_pool")
    
    if not isinstance(pool_raw, list) or not pool_raw:
        return _copy_runtime_pool(DEFAULT_RUNTIME_POOL)
    
    pool: List[RuntimePoolEntry] = []
    for item in pool_raw:
        if isinstance(item, dict) and item.get("runtime"):
            pool.append(RuntimePoolEntry.from_dict(item))
    
    if not pool:
        return _copy_runtime_pool(DEFAULT_RUNTIME_POOL)
    
    # Sort by priority
    pool.sort(key=lambda x: x.priority)
    return pool


def set_runtime_pool(pool: List[RuntimePoolEntry]) -> None:
    """Set the runtime pool in settings."""
    settings = load_settings()
    settings["runtime_pool"] = [e.to_dict() for e in pool]
    save_settings(settings)


def get_recommended_runtime(scenario: str = "general") -> Optional[str]:
    """Get the recommended runtime for a scenario.
    
    Returns the highest priority available runtime that matches the scenario.
    """
    from .runtime import detect_runtime
    
    pool = get_runtime_pool()
    scenario_lower = scenario.lower()
    
    # First pass: find matching scenario
    for entry in pool:
        if scenario_lower in [s.lower() for s in entry.scenarios]:
            rt = detect_runtime(entry.runtime)
            if rt.available:
                return entry.runtime
    
    # Second pass: any available runtime
    for entry in pool:
        rt = detect_runtime(entry.runtime)
        if rt.available:
            return entry.runtime
    
    return None
