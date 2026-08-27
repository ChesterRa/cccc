from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_web_shell_assets_use_the_registered_ui_base_path() -> None:
    source = (ROOT / "web/index.html").read_text(encoding="utf-8")

    assert 'href="%BASE_URL%logo.svg"' in source
    assert 'href="%BASE_URL%manifest.webmanifest"' in source
    assert 'href="/apple-touch-icon.png"' in source
