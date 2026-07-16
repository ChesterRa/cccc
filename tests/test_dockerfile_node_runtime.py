from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DOCKERFILE = ROOT / "docker" / "Dockerfile"


def test_final_image_reuses_web_builder_node_runtime() -> None:
    text = DOCKERFILE.read_text(encoding="utf-8")
    final_stage = text.split("# Stage 2: Final image", 1)[1]

    assert "FROM node:20-bookworm-slim AS web-builder" in text
    assert "FROM python:3.11-slim-bookworm" in text
    assert "CCCC_BROWSER_DEPS_DEBIAN13" not in text
    assert "deb.nodesource.com" not in final_stage
    assert "setup_20.x" not in final_stage
    assert "apt-get install -y --no-install-recommends nodejs" not in final_stage
    assert "COPY --from=web-builder /usr/local/bin/node /usr/local/bin/node" in final_stage
    assert "COPY --from=web-builder /usr/local/lib/node_modules /usr/local/lib/node_modules" in final_stage
    assert "ln -sf ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm" in final_stage
    assert "ln -sf ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx" in final_stage


def test_final_image_validates_npm_before_global_cli_install() -> None:
    text = DOCKERFILE.read_text(encoding="utf-8")

    npm_check_index = text.index("&& npm --version")
    cli_install_index = text.index("RUN npm install -g")

    assert npm_check_index < cli_install_index
