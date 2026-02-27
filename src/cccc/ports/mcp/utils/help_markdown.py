from __future__ import annotations

import re
from typing import Optional


_HELP_ROLE_HEADER_RE = re.compile(r"^##\s*@role:\s*(\w+)\s*$", re.IGNORECASE)
_HELP_ACTOR_HEADER_RE = re.compile(r"^##\s*@actor:\s*(.+?)\s*$", re.IGNORECASE)
_HELP_H2_RE = re.compile(r"^##(?!#)\s+.*$")


def _select_help_markdown(markdown: str, *, role: Optional[str], actor_id: Optional[str]) -> str:
    """Filter CCCC_HELP markdown by optional conditional blocks.

    Supported markers (level-2 headings):
    - "## @role: foreman|peer"
    - "## @actor: <actor_id>"

    Untagged content is always included. Tagged blocks are filtered only when the selector is known.

    A tagged block starts at its marker heading and ends at the next level-2 heading.
    Within tagged blocks, prefer "###" for subheadings (so "##" can remain a block boundary).
    """
    raw = str(markdown or "")
    if not raw.strip():
        return raw

    role_norm = str(role or "").strip().casefold()
    actor_norm = str(actor_id or "").strip()
    lines = raw.splitlines()
    keep_trailing_newline = raw.endswith("\n")

    out: list[str] = []
    buf: list[str] = []
    tag_kind: Optional[str] = None
    tag_value: str = ""

    def _include_block() -> bool:
        if tag_kind is None:
            return True
        if tag_kind == "role":
            if not role_norm:
                return True
            return role_norm == str(tag_value or "").strip().casefold()
        if tag_kind == "actor":
            if not actor_norm:
                return False
            return actor_norm == str(tag_value or "").strip()
        return True

    def _flush() -> None:
        nonlocal buf
        if buf and _include_block():
            out.extend(buf)
        buf = []

    for ln in lines:
        m_role = _HELP_ROLE_HEADER_RE.match(ln)
        m_actor = _HELP_ACTOR_HEADER_RE.match(ln)
        is_h2 = bool(_HELP_H2_RE.match(ln))

        if m_role or m_actor:
            _flush()
            if m_role:
                tag_kind = "role"
                tag_value = str(m_role.group(1) or "").strip()
                role_label = tag_value.strip().casefold()
                if role_label == "foreman":
                    ln = "## Foreman"
                elif role_label == "peer":
                    ln = "## Peer"
                else:
                    ln = f"## Role: {tag_value}"
            else:
                tag_kind = "actor"
                tag_value = str(m_actor.group(1) or "").strip()
                ln = "## Notes for you"
            buf.append(ln)
            continue

        if is_h2:
            _flush()
            tag_kind = None
            tag_value = ""

        buf.append(ln)
    _flush()

    out_text = "\n".join(out)
    if keep_trailing_newline:
        out_text += "\n"
    return out_text

