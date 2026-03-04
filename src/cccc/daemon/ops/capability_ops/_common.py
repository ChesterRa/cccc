"""Common constants, error helpers, path utilities, HTTP/env helpers for capability_ops."""

from __future__ import annotations

import json
import os
import re
import threading
from pathlib import Path
from typing import Any, Dict, Optional
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from ....contracts.v1 import DaemonError, DaemonResponse
from ....kernel.actors import get_effective_role
from ....kernel.group import load_group
from ....paths import ensure_home

_SOURCE_IDS = (
    "manual_import",
    "mcp_registry_official",
    "anthropic_skills",
    "github_skills_curated",
    "skillsmp_remote",
    "clawhub_remote",
    "openclaw_skills_remote",
    "clawskills_remote",
)

_MCP_REGISTRY_BASE = "https://registry.modelcontextprotocol.io"
_MCP_REGISTRY_PAGE_LIMIT = 100
_GITHUB_API_BASE = "https://api.github.com"
_RAW_GITHUB_BASE = "https://raw.githubusercontent.com"
_OPENCLAW_SKILLS_TREE_API = f"{_GITHUB_API_BASE}/repos/openclaw/skills/git/trees/main?recursive=1"
_OPENCLAW_SKILLS_BLOB_BASE = "https://raw.githubusercontent.com/openclaw/skills/main"
_CLAWSKILLS_DATA_URL_DEFAULT = "https://clawskills.co/skills-data.js"
_SKILL_NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
_ARG_TEMPLATE_RE = re.compile(r"\{[a-zA-Z_][a-zA-Z0-9_]*\}")
_ENV_FORWARD_TEMPLATE_RE = re.compile(r"^([A-Z][A-Z0-9_]*)=\{[a-zA-Z_][a-zA-Z0-9_]*\}$")
_CLAWSKILLS_ENTRY_RE = re.compile(r"\{[^{}]*\}")
_STATE_LOCK = threading.RLock()
_CATALOG_LOCK = threading.RLock()
_RUNTIME_LOCK = threading.RLock()
_AUDIT_LOCK = threading.RLock()
_POLICY_LOCK = threading.RLock()
_REMOTE_SOURCE_CACHE_LOCK = threading.RLock()
_OPENCLAW_TREE_CACHE: Dict[str, Any] = {"fetched_at": 0.0, "paths": []}

_LEVEL_INDEXED = "indexed"
_LEVEL_MOUNTED = "mounted"
_LEVEL_ENABLED = "mounted"  # alias: enabled merged into mounted (3→2 level simplification)
_LEVEL_PINNED = "pinned"
_LEVELS = {_LEVEL_INDEXED, _LEVEL_MOUNTED, _LEVEL_PINNED}
_POLICY_CACHE: Dict[str, Any] = {
    "key": "",
    "compiled": None,
    "source": "",
    "error": "",
}
_QUAL_QUALIFIED = "qualified"
_QUAL_BLOCKED = "blocked"
_QUAL_UNAVAILABLE = "unavailable"
_QUAL_STATES = {_QUAL_QUALIFIED, _QUAL_BLOCKED, _QUAL_UNAVAILABLE}


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(
        ok=False,
        error=DaemonError(code=code, message=message, details=(details or {})),
    )

def _capability_root() -> Path:
    return ensure_home() / "state" / "capabilities"

def _state_path() -> Path:
    return _capability_root() / "state.json"

def _catalog_path() -> Path:
    return _capability_root() / "catalog.json"

def _runtime_path() -> Path:
    return _capability_root() / "runtime.json"

def _audit_path() -> Path:
    return _capability_root() / "audit.jsonl"

def _ensure_group(group_id: str):
    gid = str(group_id or "").strip()
    if not gid:
        raise ValueError("missing_group_id")
    group = load_group(gid)
    if group is None:
        raise LookupError(f"group not found: {gid}")
    return group

def _is_foreman(group: Any, actor_id: str) -> bool:
    aid = str(actor_id or "").strip()
    if not aid:
        return False
    try:
        return str(get_effective_role(group, aid) or "") == "foreman"
    except Exception:
        return False

def _normalize_scope(raw_scope: Any) -> str:
    scope = str(raw_scope or "session").strip().lower()
    if scope not in {"group", "actor", "session"}:
        raise ValueError(f"invalid scope: {scope}")
    return scope

def _http_get_json(url: str, *, headers: Optional[Dict[str, str]] = None, timeout: float = 10.0) -> Any:
    req = Request(url, method="GET")
    all_headers = {"Accept": "application/json"}
    if isinstance(headers, dict):
        all_headers.update({str(k): str(v) for k, v in headers.items()})
    for k, v in all_headers.items():
        req.add_header(k, v)
    with urlopen(req, timeout=timeout) as resp:
        payload = resp.read().decode("utf-8", errors="replace")
    return json.loads(payload)

def _http_get_json_obj(url: str, *, headers: Optional[Dict[str, str]] = None, timeout: float = 10.0) -> Dict[str, Any]:
    data = _http_get_json(url, headers=headers, timeout=timeout)
    if not isinstance(data, dict):
        raise ValueError("response is not a JSON object")
    return data

def _http_get_text(url: str, *, headers: Optional[Dict[str, str]] = None, timeout: float = 10.0) -> str:
    req = Request(url, method="GET")
    all_headers = {"Accept": "text/plain, text/markdown, text/html;q=0.9, */*;q=0.8"}
    if isinstance(headers, dict):
        all_headers.update({str(k): str(v) for k, v in headers.items()})
    for k, v in all_headers.items():
        req.add_header(k, v)
    with urlopen(req, timeout=timeout) as resp:
        return resp.read().decode("utf-8", errors="replace")

def _env_int(name: str, default: int) -> int:
    raw = str(os.environ.get(name) or "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except Exception:
        return default

def _env_bool(name: str, default: bool) -> bool:
    raw = str(os.environ.get(name) or "").strip().lower()
    if not raw:
        return default
    if raw in {"1", "true", "yes", "on"}:
        return True
    if raw in {"0", "false", "no", "off"}:
        return False
    return default

def _quota_limit(name: str, default: int, *, minimum: int = 1, maximum: int = 10_000) -> int:
    raw = _env_int(name, default)
    return max(minimum, min(int(raw or default), maximum))
