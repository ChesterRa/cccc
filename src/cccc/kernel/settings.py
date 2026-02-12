"""Global settings management for CCCC.

Settings are stored in ~/.cccc/settings.yaml and include:
- runtime_pool: Prioritized list of agent runtimes with scenarios
- Other global preferences
"""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml  # type: ignore

from ..paths import ensure_home
from ..util.fs import atomic_write_text


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
        runtime="neovate",
        priority=5,
        scenarios=["coding", "general"],
        notes="Neovate Code; MCP support",
    ),
    RuntimePoolEntry(
        runtime="opencode",
        priority=6,
        scenarios=["coding"],
        notes="Solid coding CLI; steady long sessions",
    ),
    RuntimePoolEntry(
        runtime="copilot",
        priority=7,
        scenarios=["coding", "general"],
        notes="GitHub Copilot CLI; MCP support",
    ),
    RuntimePoolEntry(
        runtime="gemini",
        priority=8,
        scenarios=["coding", "general"],
        notes="Gemini CLI; MCP support",
    ),
    RuntimePoolEntry(
        runtime="auggie",
        priority=9,
        scenarios=["coding", "general"],
        notes="Auggie (Augment CLI); MCP support",
    ),
    RuntimePoolEntry(
        runtime="cursor",
        priority=10,
        scenarios=["coding", "general"],
        notes="Cursor CLI (cursor-agent); MCP support",
    ),
    RuntimePoolEntry(
        runtime="kilocode",
        priority=11,
        scenarios=["coding", "general"],
        notes="Kilo Code CLI; MCP support",
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


def _merge_observability(raw: Any) -> Dict[str, Any]:
    """Merge/validate observability settings with defaults."""
    base = dict(DEFAULT_OBSERVABILITY)
    if not isinstance(raw, dict):
        return base

    base["developer_mode"] = _as_bool(raw.get("developer_mode"), bool(base["developer_mode"]))
    base["log_level"] = _as_str(raw.get("log_level"), str(base["log_level"])).upper()

    comps = raw.get("components")
    if isinstance(comps, list) and comps:
        base["components"] = [str(x).strip() for x in comps if str(x).strip()]

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
        merged["log_level"] = _as_str(patch.get("log_level"), str(merged["log_level"])).upper()
    if "components" in patch:
        comps = patch.get("components")
        if isinstance(comps, list):
            merged["components"] = [str(x).strip() for x in comps if str(x).strip()]
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

    settings["observability"] = merged
    save_settings(settings)
    return _merge_observability(merged)


def _settings_path() -> Path:
    return ensure_home() / "settings.yaml"


def load_settings() -> Dict[str, Any]:
    """Load global settings from ~/.cccc/settings.yaml."""
    p = _settings_path()
    if not p.exists():
        return {}
    try:
        doc = yaml.safe_load(p.read_text(encoding="utf-8")) or {}
        return doc if isinstance(doc, dict) else {}
    except Exception:
        return {}


def save_settings(settings: Dict[str, Any]) -> None:
    """Save global settings to ~/.cccc/settings.yaml."""
    p = _settings_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_text(p, yaml.safe_dump(settings, allow_unicode=True, sort_keys=False))


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
