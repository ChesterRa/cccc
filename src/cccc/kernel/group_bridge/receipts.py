"""Remote-send receipt + idempotency store (Stage 1).

Receipts are keyed by ``(registration_id, idempotency_key)``. Recording is
idempotent: replaying the same key returns the already-stored receipt rather
than regenerating a new effect. This is the dedupe primitive the outbox worker
will rely on (worker itself is out of scope for Stage 1).

``safe_error_projection`` produces an externally-safe view that whitelists
known fields and masks anything that looks like a raw access token, so error
surfaces never leak a secret.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

import yaml

from ...paths import ensure_home
from ...util.fs import atomic_write_text

# registration_id is always a fixed "reg_<hex>" shape, so the first separator
# unambiguously delimits it from the (client-supplied) idempotency key.
_KEY_SEP = "::"
# Mask anything resembling an access token (acc_<hex/urlsafe>).
_TOKEN_RE = re.compile(r"acc_[0-9A-Za-z_-]{6,}")
_ERROR_WHITELIST = ("code", "message", "retriable", "transport", "http_status")


def _receipts_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "group_bridge_receipts.yaml"


def _compose_key(registration_id: str, idempotency_key: str) -> str:
    rid = str(registration_id or "").strip()
    ik = str(idempotency_key or "").strip()
    return f"{rid}{_KEY_SEP}{ik}"


def load_receipts(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    path = _receipts_path(home)
    if not path.exists():
        return {}
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception:
        return {}
    receipts = raw.get("receipts") if isinstance(raw, dict) else None
    if not isinstance(receipts, dict):
        return {}
    return {str(k): dict(v) for k, v in receipts.items() if isinstance(v, dict)}


def _save_receipts(receipts: Dict[str, Dict[str, Any]], home: Optional[Path] = None) -> None:
    payload = {"receipts": {str(k): dict(v) for k, v in receipts.items()}}
    atomic_write_text(
        _receipts_path(home),
        yaml.safe_dump(payload, allow_unicode=True, sort_keys=True, default_flow_style=False),
    )


def get_receipt(registration_id: str, idempotency_key: str, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    receipts = load_receipts(home)
    entry = receipts.get(_compose_key(registration_id, idempotency_key))
    return dict(entry) if isinstance(entry, dict) else None


def record_receipt(
    registration_id: str,
    idempotency_key: str,
    receipt: Dict[str, Any],
    home: Optional[Path] = None,
) -> Tuple[Dict[str, Any], bool]:
    """Store a receipt idempotently.

    Returns ``(stored_receipt, created)``. If the key already exists, the
    existing receipt is returned unchanged and ``created`` is ``False``.
    """
    receipts = load_receipts(home)
    key = _compose_key(registration_id, idempotency_key)
    existing = receipts.get(key)
    if isinstance(existing, dict):
        return dict(existing), False
    entry = dict(receipt or {})
    entry["registration_id"] = str(registration_id or "").strip()
    entry["idempotency_key"] = str(idempotency_key or "").strip()
    receipts[key] = entry
    _save_receipts(receipts, home)
    return dict(entry), True


def update_receipt(
    registration_id: str,
    idempotency_key: str,
    home: Optional[Path] = None,
    **fields: Any,
) -> Optional[Dict[str, Any]]:
    """Patch an existing receipt (e.g. queued -> sent). Returns None if absent."""
    receipts = load_receipts(home)
    key = _compose_key(registration_id, idempotency_key)
    existing = receipts.get(key)
    if not isinstance(existing, dict):
        return None
    existing.update(fields)
    receipts[key] = existing
    _save_receipts(receipts, home)
    return dict(existing)


def _mask_secrets(text: str) -> str:
    return _TOKEN_RE.sub(lambda m: f"acc_***{m.group(0)[-4:]}", str(text or ""))


def safe_error_projection(data: Dict[str, Any]) -> Dict[str, Any]:
    """Project an error/receipt dict to an externally-safe view.

    - Drops any non-whitelisted field (no internal state leaks).
    - Masks access-token-shaped substrings in textual fields.
    """
    source = data if isinstance(data, dict) else {}
    out: Dict[str, Any] = {}
    for field in _ERROR_WHITELIST:
        if field not in source:
            continue
        value = source[field]
        out[field] = _mask_secrets(value) if isinstance(value, str) else value
    return out
