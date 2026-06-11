"""Authorization decision helpers for federation (Stage 1).

Reuses the existing access-token ACL semantics (``kernel.access_tokens`` +
the ``allowed_groups`` / ``is_admin`` model enforced by the web layer) instead
of inventing a new ACL.

Critical semantic — the empty ``allowed_groups`` list is overloaded and must be
disambiguated explicitly:
- ``is_admin = True``  -> empty ``allowed_groups`` means **ALL** groups.
- ``is_admin = False`` -> empty ``allowed_groups`` means **NO** groups.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List, Optional

from ..access_tokens import lookup_access_token


def is_admin_entry(entry: Dict[str, Any]) -> bool:
    return bool(entry.get("is_admin", False)) if isinstance(entry, dict) else False


def allowed_group_ids(entry: Dict[str, Any]) -> List[str]:
    if not isinstance(entry, dict):
        return []
    raw = entry.get("allowed_groups")
    if not isinstance(raw, list):
        return []
    out: List[str] = []
    seen = set()
    for item in raw:
        gid = str(item or "").strip()
        if gid and gid not in seen:
            seen.add(gid)
            out.append(gid)
    return out


def can_access_group(entry: Dict[str, Any], group_id: str) -> bool:
    """Decide whether a token entry may act on ``group_id``.

    Admin => all groups. Non-admin => only explicitly listed groups
    (empty list => none).
    """
    if is_admin_entry(entry):
        return True
    gid = str(group_id or "").strip()
    if not gid:
        return False
    return gid in set(allowed_group_ids(entry))


def authorize_token_group(token: str, group_id: str, home: Optional[Path] = None) -> Dict[str, Any]:
    """Resolve a raw token and decide access to ``group_id``.

    Returns ``{allowed, is_admin, reason}``. Unknown/empty token => denied.
    """
    entry = lookup_access_token(str(token or "").strip(), home)
    if not isinstance(entry, dict):
        return {"allowed": False, "is_admin": False, "reason": "unknown_token"}
    admin = is_admin_entry(entry)
    allowed = can_access_group(entry, group_id)
    if admin:
        reason = "admin"
    elif allowed:
        reason = "group_allowed"
    else:
        reason = "group_denied"
    return {"allowed": allowed, "is_admin": admin, "reason": reason}
