#!/usr/bin/env python3
"""Require native tests by name and publish bounded probe evidence."""
import argparse
import json
from pathlib import Path
import sys

REQUIRED = (
    "native_birth_and_parent_stay_available_until_an_owned_child_is_reaped",
    "actual_child_birth_rejects_a_changed_identity_before_signalling",
    "region_reply_matches_the_published_layout",
    "running_image_keeps_its_identity_after_its_launch_path_is_replaced",
    "lists_a_running_and_an_exited_unreaped_child_but_not_the_caller",
    "empty_darwin_watch_is_a_complete_native_record",
    "the_watchdog_terminates_groups_in_a_fresh_watch_list",
    "the_watchdog_leaves_a_stale_watch_list_to_verified_reconciliation",
    "a_fresh_watch_list_does_not_authorize_a_recycled_group",
    "a_stalled_watchdog_never_blocks_publication_or_forced_exit",
    "a_group_published_while_the_watchdog_is_paused_still_ends_with_its_owner",
    "host_identity_and_native_parent_follow_process_lifetime",
    "mcp_registration_uses_the_native_peer_while_start_and_control_are_pending",
    "long_owner_paths_support_control_and_terminal_connections",
    "an_unreachable_owner_keeps_unknown_and_exited_guidance_separate",
    "an_unstopped_opened_browser_is_recorded_for_the_next_acquire",
    "an_opened_browser_is_stopped_only_by_its_identity",
)
PHASES = (
    "nextest_download", "nextest_verify", "sdk_compile", "sdk_layout",
    "web_dependencies", "web_bundle", "check", "clippy", "native_tests",
    "coverage", "binary", "version", "self_check", "watchdog", "source_unchanged",
)


def emit(name: str, text: str) -> None:
    def encoded(part: int, content: str) -> str:
        message = json.dumps({"file": name, "part": part, "content": content})
        message = message.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        return f"::notice title=macos-native-{name}-{part}::{message}"

    start = part = 0
    while start < len(text) or part == 0:
        low, high = 0, min(2800, len(text) - start)
        while low < high:
            end = (low + high + 1) // 2
            if len(encoded(part, text[start:start + end]).encode()) <= 2800:
                low = end
            else:
                high = end - 1
        if not low and start < len(text):
            raise ValueError("annotation metadata exceeds its bound")
        print(encoded(part, text[start:start + low]))
        start += low
        part += 1
        if start == len(text):
            break


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    log = args.directory / "native_tests.log"
    lines = log.read_text(errors="replace").splitlines() if log.exists() else []
    passed = {name: any("PASS" in line and line.rstrip().endswith("::" + name) for line in lines)
              for name in REQUIRED}
    if args.verify:
        print(json.dumps(passed, indent=2))
        return 0 if all(passed.values()) else 1
    phases = {}
    for name in PHASES:
        path = args.directory / f"{name}.rc"
        phases[name] = path.read_text().strip() if path.exists() else "not_run"
    self_check = args.directory / "self_check.log"
    try:
        gate_closed = json.loads(self_check.read_text())["capabilities"]["preserve_agents"] is False
    except (OSError, ValueError, KeyError, TypeError):
        gate_closed = False
    result = {"phases": phases, "required_native_tests": passed,
              "mac_preservation_gate_closed": gate_closed,
              "passed": gate_closed and all(passed.values())
              and all(rc == "0" for rc in phases.values()),
              "scope": "native Rust process boundaries only; Mac preservation remains disabled"}
    (args.directory / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    emit("result.json", json.dumps(result, indent=2))
    for name in ("check", "clippy"):
        path = args.directory / f"{name}.log"
        if phases[name] != "0" and path.exists():
            lines = path.read_text(errors="replace").splitlines()
            diagnostics = [line for line in lines if line.startswith(("error", "warning"))
                           or line.lstrip().startswith("-->")]
            emit(f"{name}-diagnostics", "\n".join(diagnostics)[:14000])
    for name, status in phases.items():
        path = args.directory / f"{name}.log"
        if status != "0" and path.exists():
            emit(f"failed-{name}", path.read_text(errors="replace")[-14000:])
    for name in ("environment.txt", "sdk_layout.log", "version.log", "self_check.log",
                 "watchdog.json", "target_before.txt", "disk_after.txt", "target_size.txt"):
        path = args.directory / name
        emit(name, path.read_text(errors="replace") if path.exists() else "not produced")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
