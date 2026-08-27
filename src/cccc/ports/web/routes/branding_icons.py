from __future__ import annotations

from pathlib import Path
from typing import Any, Dict

from fastapi import APIRouter, HTTPException
from fastapi.responses import RedirectResponse, Response

from ....kernel.settings import get_web_branding_settings
from ..branding import build_pwa_icon_svg, resolve_branding_asset_path


def apple_touch_icon_url(raw: Dict[str, Any]) -> str:
    version = str(raw.get("updated_at") or "default")
    for asset_kind, key in (
        ("logo_icon", "logo_icon_asset_path"),
        ("favicon", "favicon_asset_path"),
    ):
        relative = str(raw.get(key) or "").strip()
        if not relative or Path(relative).suffix.lower() != ".png":
            continue
        try:
            path = resolve_branding_asset_path(relative)
        except (FileNotFoundError, ValueError):
            continue
        if path.is_file():
            return f"/api/v1/branding/assets/{asset_kind}?v={version}"
    return "/ui/logo.png"


def create_router() -> APIRouter:
    router = APIRouter()

    @router.get("/pwa-icon.svg", include_in_schema=False)
    async def pwa_icon() -> Response:
        return _pwa_icon_response(maskable=False)

    @router.get("/pwa-icon-maskable.svg", include_in_schema=False)
    async def pwa_icon_maskable() -> Response:
        return _pwa_icon_response(maskable=True)

    @router.get("/apple-touch-icon.png", include_in_schema=False)
    async def apple_touch_icon() -> RedirectResponse:
        target = apple_touch_icon_url(get_web_branding_settings())
        return RedirectResponse(url=target, status_code=307, headers={"Cache-Control": "no-cache"})

    return router


def _pwa_icon_response(*, maskable: bool) -> Response:
    try:
        content = build_pwa_icon_svg(get_web_branding_settings(), maskable=maskable)
    except (FileNotFoundError, ValueError) as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    return Response(content=content, media_type="image/svg+xml", headers={"Cache-Control": "no-cache"})
