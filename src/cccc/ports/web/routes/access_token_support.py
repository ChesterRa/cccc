from __future__ import annotations

import hashlib
from typing import Any, Dict, List, NoReturn, Optional

from fastapi import HTTPException

from ....kernel.access_tokens import list_access_tokens


def clean_allowed_groups(raw: Optional[List[str]]) -> List[str]:
    if not isinstance(raw, list):
        return []
    seen: set[str] = set()
    cleaned: List[str] = []
    for item in raw:
        group_id = str(item or "").strip()
        if group_id and group_id not in seen:
            seen.add(group_id)
            cleaned.append(group_id)
    return cleaned


def ensure_scoped_groups_present(allowed_groups: List[str]) -> None:
    if allowed_groups:
        return
    raise HTTPException(
        status_code=400,
        detail={
            "code": "invalid_request",
            "message": "scoped access tokens must include at least one allowed group",
            "details": {},
        },
    )


def last_admin_required(message: str) -> NoReturn:
    raise HTTPException(
        status_code=400,
        detail={"code": "last_admin_required", "message": message, "details": {}},
    )


def token_id(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()[:16]


def resolve_raw_token(value: str) -> str:
    target = str(value or "").strip()
    if len(target) != 16:
        return ""
    for item in list_access_tokens():
        raw = str((item or {}).get("token") or "").strip()
        if raw and token_id(raw) == target:
            return raw
    return target


def mask_entry(item: Dict[str, Any]) -> Dict[str, Any]:
    entry = dict(item)
    raw = str(entry.get("token") or "")
    entry["token_id"] = token_id(raw) if raw else ""
    entry["token_preview"] = raw[:4] + "..." + raw[-4:] if len(raw) > 8 else "****"
    entry.pop("token", None)
    return entry
