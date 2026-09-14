import os
from pathlib import Path
import shutil
import subprocess

import pytest


ROOT = Path(__file__).resolve().parents[1]


def _checkout(root: Path, name: str, *, packaged: bool) -> Path:
    package = root / name / "crates/cccc-web"
    (package / "src").mkdir(parents=True)
    (package / "Cargo.toml").write_text(
        '[package]\nname="web-assets-probe"\nversion="0.1.0"\nedition="2024"\n'
        "[workspace]\n",
        encoding="utf-8",
    )
    shutil.copyfile(ROOT / "crates/cccc-web/build.rs", package / "build.rs")
    # Resolve relative paths from the package root, as RustEmbed does. Absolute
    # paths also work, so the previous cross-checkout bug fails on wrong content.
    (package / "embedded.rs").write_text(
        'pub const PAGE: &str = include_str!(concat!(env!("CCCC_WEB_DIST_DIR"), '
        '"/index.html"));\n',
        encoding="utf-8",
    )
    (package / "src/main.rs").write_text(
        '#[path="../embedded.rs"] mod embedded;\n'
        f'fn main() {{ print!("backend_{name}:{{}}", embedded::PAGE); }}\n',
        encoding="utf-8",
    )
    if packaged:
        dist = package / "assets/web-dist"
    else:
        web = package.parent.parent / "web"
        for directory in ("src", "public"):
            (web / directory).mkdir(parents=True)
        for file in (
            "index.html",
            "package.json",
            "package-lock.json",
            "components.json",
            "postcss.config.cjs",
            "tailwind.config.ts",
            "tsconfig.json",
            "vite.config.ts",
        ):
            (web / file).write_text("{}", encoding="utf-8")
        dist = web / "dist"
    dist.mkdir(parents=True)
    # A ready frontend avoids invoking npm; this test exercises Cargo's real
    # dependency tracking and embedding, with no registry or provider dependency.
    (dist / "index.html").write_text(f"frontend_{name}", encoding="utf-8")
    return package


@pytest.mark.parametrize("packaged", [False, True], ids=["source", "packaged"])
def test_shared_cargo_cache_embeds_the_current_checkout(
    tmp_path: Path, packaged: bool
) -> None:
    cargo = shutil.which("cargo")
    if cargo is None or shutil.which("rustc") is None:
        pytest.skip("the build-cache regression requires cargo and rustc")
    first, second = [
        _checkout(tmp_path, name, packaged=packaged) for name in ("first", "second")
    ]
    env = {**os.environ, "CARGO_TARGET_DIR": str(tmp_path / "target")}
    env.pop("CCCC_FORCE_WEB_BUILD", None)

    def build(package: Path) -> str:
        result = subprocess.run(
            [
                cargo,
                "run",
                "--offline",
                "--release",
                "--quiet",
                "--manifest-path",
                str(package / "Cargo.toml"),
            ],
            cwd=tmp_path,
            env=env,
            check=True,
            capture_output=True,
            text=True,
            timeout=60,
        )
        return result.stdout

    def frontend(package: Path) -> Path:
        return (
            package / "assets/web-dist"
            if packaged
            else package.parent.parent / "web/dist"
        ) / "index.html"

    assert build(first) == "backend_first:frontend_first"
    # Recompile the backend in B while its already-built Web files predate A's
    # build. A stale build-script result must not import A's frontend into B.
    main = second / "src/main.rs"
    main.write_text(
        main.read_text(encoding="utf-8") + "// Backend changed.\n", encoding="utf-8"
    )
    assert build(second) == "backend_second:frontend_second"

    # Switch back, then rebuild only frontend assets as build_package.sh does.
    frontend(first).write_text("frontend_first_updated", encoding="utf-8")
    assert build(first) == "backend_first:frontend_first_updated"
    frontend(second).write_text("frontend_second_updated", encoding="utf-8")
    assert build(second) == "backend_second:frontend_second_updated"

    outputs = list(
        (tmp_path / "target/release/build").glob("web-assets-probe-*/output")
    )
    before = {path: path.stat().st_mtime_ns for path in outputs}
    assert before
    assert build(second) == "backend_second:frontend_second_updated"
    assert {path: path.stat().st_mtime_ns for path in outputs} == before, (
        "unchanged builds stay cached"
    )
