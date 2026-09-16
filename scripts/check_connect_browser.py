#!/usr/bin/env python3
"""Build and exercise native Connect against the sibling account fixture only."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--journey", choices=("all", "workbench", "groups"), default="all"
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    homepage = root.parent / "cccc-homepage"
    if not (homepage / "account/scripts/connect-fixture.mjs").is_file():
        parser.error(
            "check out cccc-homepage beside cccc, and install both repositories' dependencies"
        )
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(("CCCC_", "GIT_"))
    }

    def run(command, **kwargs):
        return subprocess.run(command, cwd=root, env=env, check=True, **kwargs)

    def revision(repo):
        return {
            "revision": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=repo, env=env, text=True
            ).strip(),
            "dirty": bool(
                subprocess.check_output(
                    ["git", "status", "--porcelain"], cwd=repo, env=env
                )
            ),
        }

    with tempfile.TemporaryDirectory(prefix="cccc-connect-check-") as home:
        env["CCCC_HOME"] = home
        run(["npm", "--prefix", "web", "run", "build"])
        run(
            [
                "cargo",
                "build",
                "--locked",
                "--features",
                "standalone",
                "-p",
                "cccc",
                "--bin",
                "cccc",
            ]
        )
        result = run(
            [
                "cargo",
                "test",
                "--locked",
                "-p",
                "cccc-pair-web",
                "--lib",
                "--no-run",
                "--message-format=json",
            ],
            stdout=subprocess.PIPE,
            text=True,
        )
        artifacts = [
            json.loads(line)
            for line in result.stdout.splitlines()
            if line.startswith("{")
        ]
        web_test = next(
            item["executable"]
            for item in artifacts
            if item.get("reason") == "compiler-artifact"
            and item.get("executable")
            and item.get("target", {}).get("name") == "cccc_web"
        )
        metadata = json.loads(
            subprocess.check_output(
                ["cargo", "metadata", "--no-deps", "--format-version=1"],
                cwd=root,
                env=env,
            )
        )
        cli = (
            Path(metadata["target_directory"])
            / "debug"
            / ("cccc.exe" if os.name == "nt" else "cccc")
        )
        with cli.open("rb") as binary:
            cli_digest = hashlib.file_digest(binary, "sha256").hexdigest()
        print(
            json.dumps(
                {
                    "core": revision(root),
                    "homepage": revision(homepage),
                    "cli_sha256": cli_digest,
                    "web_test": web_test,
                    "journey": args.journey,
                }
            ),
            flush=True,
        )
        env["CCCC_CONNECT_CLI"] = str(cli)
        env["CCCC_CONNECT_WEB_TEST_BIN"] = web_test
        for journey in (
            ("workbench", "groups") if args.journey == "all" else (args.journey,)
        ):
            env["CCCC_CONNECT_GROUPS_PROBE"] = "1" if journey == "groups" else "0"
            run(["node", "web/tests/browser/connect-workbench.mjs"])


if __name__ == "__main__":
    main()
