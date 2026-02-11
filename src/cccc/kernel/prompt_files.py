from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from .group import Group
from ..util.fs import atomic_write_text


PREAMBLE_FILENAME = "CCCC_PREAMBLE.md"
HELP_FILENAME = "CCCC_HELP.md"
PROMPTS_DIRNAME = "prompts"

_MAX_FILE_BYTES = 512 * 1024  # Safety limit for prompt markdown files.

DEFAULT_PREAMBLE_BODY = """Quick start (recommended):
- Call `cccc_bootstrap` to load PROJECT.md (if present) + Context + inbox + the CCCC help playbook.
- If PROJECT.md is missing, ask the user for goals/constraints/DoD and record a short DoD in Context.
- Read the returned help playbook (or call `cccc_help` anytime to refresh).

Intent: collaborate rigorously (if peers exist) and keep pushing the task forward with evidence-based steps.
Mechanics: use MCP for visible chat; keep commitments/decisions/progress in Context; keep the inbox clean.
Message modes: choose intentionally (normal / attention / task[reply_required]); avoid both overuse and underuse.
"""

def load_builtin_help_markdown() -> str:
    """Load the built-in CCCC help markdown bundled in the package."""
    try:
        import importlib.resources

        files = importlib.resources.files("cccc.resources")
        return (files / "cccc-help.md").read_text(encoding="utf-8")
    except Exception:
        try:
            p = Path(__file__).resolve().parents[1] / "resources" / "cccc-help.md"
            return p.read_text(encoding="utf-8")
        except Exception:
            return ""


@dataclass(frozen=True)
class PromptFile:
    filename: str
    path: Optional[str]
    found: bool
    content: Optional[str]


def resolve_active_scope_root(group: Group) -> Optional[Path]:
    """Resolve the active scope root directory for a group.

    Returns None when the group has no attached scope or the scope URL is missing.
    """
    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list) or not scopes:
        return None

    active_scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if active_scope_key:
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            if str(sc.get("scope_key") or "").strip() != active_scope_key:
                continue
            url = str(sc.get("url") or "").strip()
            if url:
                return Path(url).expanduser().resolve()

    for sc in scopes:
        if not isinstance(sc, dict):
            continue
        url = str(sc.get("url") or "").strip()
        if url:
            return Path(url).expanduser().resolve()

    return None


def _read_text_file(path: Path) -> str:
    raw = path.read_bytes()
    if len(raw) > _MAX_FILE_BYTES:
        raw = raw[:_MAX_FILE_BYTES]
    return raw.decode("utf-8", errors="replace")


def _group_prompts_root(group: Group) -> Path:
    return group.path / PROMPTS_DIRNAME


def read_group_prompt_file(group: Group, filename: str) -> PromptFile:
    """Read a group prompt override from CCCC_HOME.

    Overrides live under:
      CCCC_HOME/groups/<group_id>/prompts/<filename>
    """
    root = _group_prompts_root(group)
    path = (root / filename).expanduser()
    if not path.exists() or not path.is_file():
        return PromptFile(filename=filename, path=str(path), found=False, content=None)
    try:
        content = _read_text_file(path)
    except Exception:
        return PromptFile(filename=filename, path=str(path), found=True, content=None)
    return PromptFile(filename=filename, path=str(path), found=True, content=content)


def delete_group_prompt_file(group: Group, filename: str) -> PromptFile:
    """Delete a group prompt override file if present (reset to built-in defaults)."""
    root = _group_prompts_root(group)
    path = (root / filename).expanduser()
    if not path.exists():
        return PromptFile(filename=filename, path=str(path), found=False, content=None)
    if path.is_file():
        os.unlink(path)
    return PromptFile(filename=filename, path=str(path), found=False, content=None)


def write_group_prompt_file(group: Group, filename: str, content: str) -> PromptFile:
    """Create or update a group prompt override file under CCCC_HOME."""
    root = _group_prompts_root(group)
    root.mkdir(parents=True, exist_ok=True)
    path = (root / filename).expanduser()
    atomic_write_text(path, str(content or ""), encoding="utf-8")
    return PromptFile(filename=filename, path=str(path), found=True, content=_read_text_file(path))
