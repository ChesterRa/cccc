from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import BinaryIO, Mapping

from ..kernel.runtime_hooks.store import record_hook_event

MAX_HOOK_JSON_BYTES = 1024 * 1024
_ACTIONS = {"codex-state": "codex", "claude-state": "claude"}
_REQUIRED_ENV = (
    "CCCC_HOME",
    "CCCC_GROUP_ID",
    "CCCC_ACTOR_ID",
    "CCCC_HOOK_LAUNCH_TOKEN",
)


def run_runtime_hook(
    action: str, stdin: BinaryIO, environ: Mapping[str, str]
) -> int:
    runtime = _ACTIONS.get(str(action))
    if runtime is None:
        raise ValueError("unsupported runtime hook action")
    values: dict[str, str] = {}
    for key in _REQUIRED_ENV:
        value = str(environ.get(key) or "").strip()
        if not value:
            raise ValueError(f"missing required hook env: {key}")
        values[key] = value
    raw = stdin.read(MAX_HOOK_JSON_BYTES + 1)
    if len(raw) > MAX_HOOK_JSON_BYTES:
        raise ValueError("runtime hook JSON is too large")
    try:
        payload = json.loads(raw.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise ValueError("invalid runtime hook JSON") from exc
    if not isinstance(payload, dict):
        raise ValueError("runtime hook JSON must be an object")
    record_hook_event(
        Path(values["CCCC_HOME"]).expanduser().resolve(),
        runtime,
        values["CCCC_GROUP_ID"],
        values["CCCC_ACTOR_ID"],
        values["CCCC_HOOK_LAUNCH_TOKEN"],
        payload,
    )
    return 0


def cmd_runtime_hook(args: argparse.Namespace) -> int:
    try:
        return run_runtime_hook(
            str(args.action), sys.stdin.buffer, os.environ
        )
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 2
