from pathlib import Path

from fastapi.testclient import TestClient


ROOT = Path(__file__).resolve().parents[1]


def test_manifest_builder_tracks_custom_branding() -> None:
    from cccc.ports.web.branding import build_web_app_manifest

    manifest = build_web_app_manifest(
        {
            "product_name": "Acme Console",
            "logo_icon_asset_path": "state/web_branding/logo.png",
            "updated_at": "2026-08-10T12:00:00Z",
        }
    )

    assert manifest["name"] == "Acme Console"
    assert manifest["short_name"] == "Acme Console"
    assert manifest["icons"][0] == {
        "src": "/pwa-icon.svg?v=2026-08-10T12:00:00Z",
        "sizes": "any",
        "type": "image/svg+xml",
        "purpose": "any",
    }
    assert manifest["icons"][1]["purpose"] == "maskable"


def test_custom_pwa_icon_svg_embeds_the_current_branding_asset(tmp_path, monkeypatch) -> None:
    from cccc.ports.web.branding import build_pwa_icon_svg, store_branding_asset

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    stored = store_branding_asset(
        asset_kind="logo_icon",
        data=b"png-bytes",
        content_type="image/png",
        filename="logo.png",
    )
    raw = {"logo_icon_asset_path": stored["rel_path"]}

    regular = build_pwa_icon_svg(raw, maskable=False).decode("utf-8")
    maskable = build_pwa_icon_svg(raw, maskable=True).decode("utf-8")

    assert "data:image/png;base64,cG5nLWJ5dGVz" in regular
    assert "<rect" not in regular
    assert '<rect width="1024" height="1024"' in maskable
    assert 'x="128" y="128" width="768" height="768"' in maskable


def test_apple_icon_prefers_a_png_favicon_when_the_custom_logo_is_not_png(
    tmp_path, monkeypatch
) -> None:
    from cccc.ports.web import app as web_app
    from cccc.ports.web.branding import store_branding_asset
    from cccc.ports.web.routes import base, branding_icons
    from cccc.ports.web.routes.branding_icons import apple_touch_icon_url

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    logo = store_branding_asset(
        asset_kind="logo_icon",
        data=b"<svg/>",
        content_type="image/svg+xml",
        filename="logo.svg",
    )
    favicon = store_branding_asset(
        asset_kind="favicon",
        data=b"png",
        content_type="image/png",
        filename="favicon.png",
    )
    raw = {
        "logo_icon_asset_path": logo["rel_path"],
        "favicon_asset_path": favicon["rel_path"],
        "updated_at": "v1",
    }

    assert apple_touch_icon_url(raw) == "/api/v1/branding/assets/favicon?v=v1"
    monkeypatch.setattr(branding_icons, "get_web_branding_settings", lambda: raw)
    monkeypatch.setattr(base, "get_web_branding_settings", lambda: raw)
    client = TestClient(web_app.create_app())
    response = client.get(
        "/apple-touch-icon.png", follow_redirects=False
    )
    assert response.status_code == 307
    assert response.headers["location"] == "/api/v1/branding/assets/favicon?v=v1"

    response = client.head(
        "/apple-touch-icon.png",
        headers={"Authorization": "Bearer invalid-token"},
        follow_redirects=True,
    )
    assert response.status_code == 200
    assert response.headers["content-length"] == "3"
    assert response.content == b""


def test_public_pwa_icon_error_does_not_expose_the_branding_path(
    tmp_path, monkeypatch
) -> None:
    from cccc.ports.web.app import create_app
    from cccc.ports.web.routes import branding_icons

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    private_path = tmp_path / "state" / "web_branding" / "private-logo.png"
    monkeypatch.setattr(
        branding_icons,
        "get_web_branding_settings",
        lambda: {"logo_icon_asset_path": "state/web_branding/private-logo.png"},
    )

    response = TestClient(create_app()).get("/pwa-icon.svg")

    assert response.status_code == 404
    assert response.json()["error"] == {
        "code": "branding_icon_unavailable",
        "message": "branding icon unavailable",
        "details": {},
    }
    assert str(private_path) not in response.text


def test_public_pwa_icon_normalizes_related_os_errors(monkeypatch) -> None:
    from cccc.ports.web.app import create_app
    from cccc.ports.web.routes import branding_icons

    def fail_to_read_icon(*_args, **_kwargs):
        raise PermissionError("/private/cccc-home/branding.png")

    monkeypatch.setattr(branding_icons, "build_pwa_icon_svg", fail_to_read_icon)

    response = TestClient(create_app()).get("/pwa-icon.svg")

    assert response.status_code == 404
    assert response.json()["error"]["code"] == "branding_icon_unavailable"
    assert "/private/cccc-home" not in response.text


def test_dynamic_icon_routes_remain_public_with_invalid_credentials(tmp_path, monkeypatch) -> None:
    from cccc.ports.web.app import create_app

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    client = TestClient(create_app())
    expected_statuses = {
        "/apple-touch-icon.png": 307,
        "/pwa-icon-maskable.svg": 404,
        "/pwa-icon.svg": 404,
    }

    for method in ("GET", "HEAD"):
        for credentials in ("header", "cookie"):
            client.cookies.clear()
            headers = {}
            if credentials == "header":
                headers["Authorization"] = "Bearer invalid-token"
            else:
                client.cookies.set("cccc_access_token", "invalid-token")
            for path, status_code in expected_statuses.items():
                response = client.request(
                    method, path, headers=headers, follow_redirects=False
                )
                assert response.status_code == status_code, (
                    method,
                    credentials,
                    path,
                    response.text,
                )


def test_vite_proxies_dynamic_branding_icons_to_the_web_backend() -> None:
    source = (ROOT / "web/vite.config.ts").read_text(encoding="utf-8")

    for path in ("/apple-touch-icon.png", "/pwa-icon-maskable.svg", "/pwa-icon.svg"):
        assert f'"{path}": {{ target: backendTarget, changeOrigin: true }}' in source
