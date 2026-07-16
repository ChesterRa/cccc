from __future__ import annotations

import hashlib
import mimetypes
from pathlib import Path
from typing import Any, Dict, Literal

from ...paths import ensure_home
from ...util.fs import atomic_write_bytes

BrandingAssetKind = Literal["logo_icon", "favicon"]

_BRANDING_MAX_BYTES = 2 * 1024 * 1024
_DEFAULT_PRODUCT_NAME = "CCCC"
_DEFAULT_LOGO_ICON_URL = "/ui/logo.svg"
_DEFAULT_FAVICON_URL = "/ui/logo.svg"

_ALLOWED_MIME_TYPES: dict[str, set[str]] = {
    "logo_icon": {
        "image/svg+xml",
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/gif",
        "image/avif",
        "image/x-icon",
        "image/vnd.microsoft.icon",
    },
    "favicon": {
        "image/svg+xml",
        "image/png",
        "image/x-icon",
        "image/vnd.microsoft.icon",
    },
}

_EXTENSION_BY_MIME: dict[str, str] = {
    "image/svg+xml": ".svg",
    "image/png": ".png",
    "image/jpeg": ".jpg",
    "image/webp": ".webp",
    "image/gif": ".gif",
    "image/avif": ".avif",
    "image/x-icon": ".ico",
    "image/vnd.microsoft.icon": ".ico",
}


def normalize_branding_asset_kind(value: str) -> BrandingAssetKind:
    normalized = str(value or "").strip().lower()
    if normalized in {"logo_icon", "favicon"}:
        return normalized  # type: ignore[return-value]
    raise ValueError("asset kind must be one of: logo_icon, favicon")


def branding_asset_dir() -> Path:
    return ensure_home() / "state" / "web_branding"


def _branding_asset_rel_path(filename: str) -> str:
    return str(Path("state") / "web_branding" / filename).replace("\\", "/")


def resolve_branding_asset_path(rel_path: str) -> Path:
    normalized = str(rel_path or "").strip().replace("\\", "/")
    if not normalized:
        raise FileNotFoundError("branding asset path is empty")
    base = ensure_home().resolve()
    target = (base / Path(*Path(normalized).parts)).resolve()
    try:
        target.relative_to(base)
    except ValueError as exc:
        raise FileNotFoundError("branding asset path is outside CCCC_HOME") from exc
    return target


def store_branding_asset(*, asset_kind: BrandingAssetKind, data: bytes, content_type: str, filename: str = "") -> Dict[str, Any]:
    mime_type = str(content_type or "").strip().lower()
    if not mime_type:
        guessed, _ = mimetypes.guess_type(str(filename or "").strip())
        mime_type = str(guessed or "").strip().lower()
    if mime_type not in _ALLOWED_MIME_TYPES[asset_kind]:
        raise ValueError(f"unsupported {asset_kind} type: {mime_type or 'unknown'}")
    if len(data) > _BRANDING_MAX_BYTES:
        raise ValueError(f"{asset_kind} file too large")

    digest = hashlib.sha256(data).hexdigest()
    ext = _EXTENSION_BY_MIME.get(mime_type) or Path(str(filename or "").strip()).suffix.lower() or ".bin"
    stored_name = f"{asset_kind}_{digest[:16]}{ext}"
    abs_path = branding_asset_dir() / stored_name
    atomic_write_bytes(abs_path, data)
    return {
        "asset_kind": asset_kind,
        "mime_type": mime_type,
        "bytes": len(data),
        "sha256": digest,
        "rel_path": _branding_asset_rel_path(stored_name),
        "public_url": f"/api/v1/branding/assets/{asset_kind}?v={digest[:16]}",
    }


def delete_branding_asset(rel_path: str) -> None:
    try:
        target = resolve_branding_asset_path(rel_path)
        if target.exists():
            target.unlink()
    except Exception:
        pass


def build_branding_payload(raw: Dict[str, Any]) -> Dict[str, Any]:
    product_name = str(raw.get("product_name") or "").strip() or _DEFAULT_PRODUCT_NAME
    logo_rel_path = str(raw.get("logo_icon_asset_path") or "").strip()
    favicon_rel_path = str(raw.get("favicon_asset_path") or "").strip()
    updated_at = str(raw.get("updated_at") or "").strip() or None

    logo_url = (
        f"/api/v1/branding/assets/logo_icon?v={updated_at or 'default'}"
        if logo_rel_path
        else _DEFAULT_LOGO_ICON_URL
    )
    favicon_url = (
        f"/api/v1/branding/assets/favicon?v={updated_at or 'default'}"
        if favicon_rel_path
        else (logo_url if logo_rel_path else _DEFAULT_FAVICON_URL)
    )
    return {
        "product_name": product_name,
        "logo_icon_url": logo_url,
        "favicon_url": favicon_url,
        "has_custom_logo_icon": bool(logo_rel_path),
        "has_custom_favicon": bool(favicon_rel_path),
        "updated_at": updated_at,
    }
