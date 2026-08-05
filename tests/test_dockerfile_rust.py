from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DOCKERFILE = ROOT / "docker" / "Dockerfile.rust"


def test_rust_builder_places_web_bundle_in_the_packaged_asset_directory() -> None:
    text = DOCKERFILE.read_text(encoding="utf-8")

    assert (
        "COPY --from=web-builder /src/web/dist "
        "./crates/cccc-web/assets/web-dist"
    ) in text
    assert "cargo build --release --locked -p cccc --bin cccc" in text
