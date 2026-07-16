from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Optional

from ..paths import ensure_home
from ..util.fs import read_json

_DEFAULT_PROVIDER = "notebooklm"
_SUPPORTED_MODES = {"disabled", "active", "degraded"}
_SUPPORTED_LANES = ("work", "memory")


def _space_doc_path(home: Path, name: str) -> Path:
    return home / "state" / "space" / name


def _binding_lanes(raw: Any) -> Dict[str, Dict[str, str]]:
    if not isinstance(raw, dict):
        return {}
    if any(key in raw for key in ("remote_space_id", "status")):
        raw = {"work": dict(raw)}
    out: Dict[str, Dict[str, str]] = {}
    for lane in _SUPPORTED_LANES:
        item = raw.get(lane) if isinstance(raw, dict) else None
        if not isinstance(item, dict):
            continue
        status = str(item.get("status") or "").strip()
        remote_space_id = str(item.get("remote_space_id") or "").strip()
        out[lane] = {
            "status": status,
            "remote_space_id": remote_space_id,
        }
    return out


def get_group_space_prompt_state(group_id: str, *, provider: str = _DEFAULT_PROVIDER) -> Optional[Dict[str, Any]]:
    gid = str(group_id or "").strip()
    pid = str(provider or _DEFAULT_PROVIDER).strip() or _DEFAULT_PROVIDER
    if not gid:
        return None
    home = ensure_home()

    bindings_doc = read_json(_space_doc_path(home, "bindings.json"))
    providers_doc = read_json(_space_doc_path(home, "providers.json"))

    bindings = bindings_doc.get("bindings") if isinstance(bindings_doc, dict) else {}
    per_group = bindings.get(gid) if isinstance(bindings, dict) else {}
    raw_provider_bindings = per_group.get(pid) if isinstance(per_group, dict) else {}
    lanes = _binding_lanes(raw_provider_bindings)
    if not lanes:
        return None

    providers = providers_doc.get("providers") if isinstance(providers_doc, dict) else {}
    provider_state = providers.get(pid) if isinstance(providers, dict) else {}
    mode = str(provider_state.get("mode") or "disabled").strip() if isinstance(provider_state, dict) else "disabled"
    if mode not in _SUPPORTED_MODES:
        mode = "disabled"

    bound_lanes = {
        lane: item
        for lane, item in lanes.items()
        if str(item.get("status") or "") == "bound" and str(item.get("remote_space_id") or "").strip()
    }
    if not bound_lanes:
        return None

    return {
        "provider": pid,
        "mode": mode,
        "lanes": bound_lanes,
        "work_bound": "work" in bound_lanes,
        "memory_bound": "memory" in bound_lanes,
    }
