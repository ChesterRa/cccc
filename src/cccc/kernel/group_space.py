from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Optional

from ..paths import ensure_home
from ..util.fs import read_json

_DEFAULT_PROVIDER = "notebooklm"
_SUPPORTED_MODES = {"disabled", "active", "degraded"}


def _space_doc_path(home: Path, name: str) -> Path:
    return home / "state" / "space" / name


def get_group_space_prompt_state(group_id: str, *, provider: str = _DEFAULT_PROVIDER) -> Optional[Dict[str, str]]:
    gid = str(group_id or "").strip()
    pid = str(provider or _DEFAULT_PROVIDER).strip() or _DEFAULT_PROVIDER
    if not gid:
        return None
    home = ensure_home()

    bindings_doc = read_json(_space_doc_path(home, "bindings.json"))
    providers_doc = read_json(_space_doc_path(home, "providers.json"))

    bindings = bindings_doc.get("bindings") if isinstance(bindings_doc, dict) else {}
    per_group = bindings.get(gid) if isinstance(bindings, dict) else {}
    binding = per_group.get(pid) if isinstance(per_group, dict) else {}
    if not isinstance(binding, dict):
        return None
    status = str(binding.get("status") or "").strip()
    remote_space_id = str(binding.get("remote_space_id") or "").strip()
    if status != "bound" or not remote_space_id:
        return None

    providers = providers_doc.get("providers") if isinstance(providers_doc, dict) else {}
    provider_state = providers.get(pid) if isinstance(providers, dict) else {}
    mode = str(provider_state.get("mode") or "disabled").strip() if isinstance(provider_state, dict) else "disabled"
    if mode not in _SUPPORTED_MODES:
        mode = "disabled"

    return {
        "provider": pid,
        "mode": mode,
        "remote_space_id": remote_space_id,
    }
