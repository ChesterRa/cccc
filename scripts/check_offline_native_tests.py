"""Fail CI if any required offline native-runtime probe is no longer registered."""
from __future__ import annotations

import argparse
from pathlib import Path

REQUIRED_TESTS = (
    "live_codex_empty_actor_and_analyst_resume_with_native_terminal",
    "live_claude_empty_session_resumes_without_a_prompt",
    "live_kilo_shared_actor_and_analyst_when_enabled",
    "live_kilo_submitted_model_reaches_acp_when_enabled",
)


def check_listing(listing: str) -> None:
    registered = {
        line.removesuffix(": test").rsplit("::", 1)[-1]
        for line in listing.splitlines()
        if line.endswith(": test")
    }
    missing = set(REQUIRED_TESTS) - registered
    if missing:
        raise ValueError("Missing offline native-runtime tests: " + ", ".join(sorted(missing)))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("listing", type=Path)
    args = parser.parse_args()
    try:
        check_listing(args.listing.read_text(encoding="utf-8"))
    except ValueError as error:
        parser.exit(1, f"{error}\n")
    print(f"Verified all {len(REQUIRED_TESTS)} required offline native-runtime tests.")


if __name__ == "__main__":
    main()
