from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_web_shell_assets_use_the_registered_ui_base_path() -> None:
    source = (ROOT / "web/index.html").read_text(encoding="utf-8")

    assert 'href="/ui/logo.svg"' in source
    assert 'href="/ui/manifest.webmanifest"' in source
    assert 'href="/ui/logo.png"' in source
