"""Slash command handling for capability installation requests."""

from __future__ import annotations

import re
import shlex
from typing import Dict, Optional

from ...kernel.install_capability import INSTALL_CAPABILITY_ID


def split_command_args(raw: str) -> list[str]:
    text = str(raw or "").strip()
    if not text:
        return []
    try:
        return shlex.split(text)
    except ValueError:
        return text.split()


def classify_install_target(target: str) -> str:
    value = str(target or "").strip()
    lower = value.lower()
    if not value:
        return "unspecified"
    if lower.startswith(("skill:", "mcp:", "pack:")):
        return "capability_id"
    if lower.startswith(("http://", "https://", "ssh://", "git+", "git@")):
        if "github.com" in lower or lower.startswith("git@github.com:"):
            return "github"
        return "url"
    if lower.startswith(("file://", "./", "../", "/", "~")):
        return "local_path"
    if re.match(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:[/#:@?].*)?$", value):
        return "repo_slug"
    if re.match(r"^[A-Za-z0-9_.-]+$", value):
        return "curated_or_named_skill"
    return "freeform"


def parse_install_slash_command(text: str) -> Optional[Dict[str, str]]:
    raw = str(text or "")
    match = re.match(r"^\s*/install(?:\s+(?P<args>[\s\S]*))?$", raw)
    if not match:
        return None
    args_text = str(match.group("args") or "").strip()
    tokens = split_command_args(args_text)
    target = tokens[0] if tokens else ""
    return {
        "command": "install",
        "args_text": args_text,
        "target": target,
        "target_kind": classify_install_target(target),
    }


def render_install_command_task(command: Dict[str, str]) -> str:
    args_text = str(command.get("args_text") or "").strip()
    target = str(command.get("target") or "").strip()
    target_kind = str(command.get("target_kind") or "unspecified").strip() or "unspecified"
    lines = [
        "[cccc] Slash command: /install",
        f"[cccc] Capability: {INSTALL_CAPABILITY_ID}",
        "",
        "Use the CCCC install skill to route the request through the CCCC capability lifecycle.",
        "Default action: call cccc_capability_install for the target with scope=group.",
        "The install operation must import registry records from capability ids, repos, URLs, or local SKILL.md paths; enable group scope; and return use-ready capability ids.",
        "Any activate, assign, autoload, or use step must operate on the imported CCCC capability record.",
        "Do not bypass the registry by installing into Codex's local skills directory.",
        "",
        "Request:",
        f"- Raw arguments: {args_text or '(none)'}",
        f"- Primary target: {target or '(none)'}",
        f"- Parser target hint: {target_kind}",
        "",
        f"Route this request through {INSTALL_CAPABILITY_ID}. The skill definition is the source of truth for classification, install path, policy checks, and verification.",
        "Treat the parser target hint as non-authoritative; re-classify the request from the skill instructions.",
    ]
    return "\n".join(lines)
