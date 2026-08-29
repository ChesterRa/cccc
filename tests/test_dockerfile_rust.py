from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]
DOCKERFILE = ROOT / "docker" / "Dockerfile"
COMPOSE_FILE = ROOT / "docker" / "docker-compose.yml"


def test_rust_builder_places_web_bundle_in_the_packaged_asset_directory() -> None:
    text = DOCKERFILE.read_text(encoding="utf-8")

    assert "FROM node:24-bookworm-slim AS web-builder" in text
    assert "COPY resources/ ./resources/" in text
    assert (
        "COPY --from=web-builder /src/web/dist "
        "./crates/cccc-web/assets/web-dist"
    ) in text
    assert "cargo build --release --locked -p cccc --bin cccc" in text
    assert text.index("COPY resources/ ./resources/") < text.index(
        "cargo build --release --locked -p cccc --bin cccc"
    )

    for resource in (
        "cccc-help.md",
        "cccc-self-evolution.md",
        "code_mode_metadata.json",
        "code_mode_runtime.js",
        "mcp_tools.json",
    ):
        assert (ROOT / "resources" / resource).is_file()


def test_default_image_contains_only_the_native_cccc_product() -> None:
    text = DOCKERFILE.read_text(encoding="utf-8")

    assert "COPY --from=rust-builder /src/target/release/cccc /usr/local/bin/cccc" in text
    assert "pip install" not in text
    assert "COPY src/ ./src/" not in text
    assert "FROM python:" not in text
    assert "chromium" in text
    assert "xvfb" in text
    assert "x11vnc" in text
    assert "https://claude.ai/install.sh" in text
    assert "ENV CCCC_WEB_ALLOW_UNAUTHENTICATED=1" in text


def test_retired_parallel_rust_docker_entrypoints_are_absent() -> None:
    assert not (ROOT / "docker" / "Dockerfile.rust").exists()
    assert not (ROOT / "docker" / "docker-compose.rust.yml").exists()


def test_default_compose_uses_native_image_and_preserves_upgrade_volume() -> None:
    compose = yaml.safe_load(COMPOSE_FILE.read_text(encoding="utf-8"))
    service = compose["services"]["cccc"]

    assert service["build"]["dockerfile"] == "docker/Dockerfile"
    assert service["ports"] == ["127.0.0.1:${CCCC_PORT:-8848}:8848"]
    assert "cccc-data:/data" in service["volumes"]
    assert compose["volumes"]["cccc-data"]["external"] is True
