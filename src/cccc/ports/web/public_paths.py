from __future__ import annotations

from fastapi import Request


PUBLIC_EXACT_PATHS = frozenset(
    {
        "/api/group-bridge/pairing/requests/remote",
        "/api/group-bridge/pairing/requests/remote/status",
        "/api/group-bridge/session/send",
        "/api/group-bridge/session/ws",
        "/api/v1/branding",
        "/api/v1/health",
        "/api/v1/ping",
        "/api/v1/ready",
        "/api/v1/web_access/exchange",
        "/api/v1/web_access/session",
        "/apple-touch-icon.png",
        "/pwa-icon-maskable.svg",
        "/pwa-icon.svg",
    }
)


def is_public_ui_path(request: Request) -> bool:
    path = str(request.url.path or "")
    return path.startswith("/ui/") or path == "/ui"


def is_public_path(request: Request) -> bool:
    """Routes that bypass token authentication."""
    path = str(request.url.path or "")
    return (
        is_public_ui_path(request)
        or path in PUBLIC_EXACT_PATHS
        or path.startswith("/api/v1/branding/assets/")
        or path.startswith("/mcp/web-model/")
        or path.startswith("/mcp/group-bridge")
        or path.startswith("/nomcp/s/")
    )
