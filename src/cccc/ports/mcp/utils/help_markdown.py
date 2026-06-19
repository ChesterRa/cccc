from __future__ import annotations

import re
from typing import Any, Optional


_HELP_ROLE_HEADER_RE = re.compile(r"^##\s*@role:\s*(\w+)\s*$", re.IGNORECASE)
_HELP_ACTOR_HEADER_RE = re.compile(r"^##\s*@actor:\s*(\S+)(?:\s+(.*\S))?\s*$", re.IGNORECASE)
_HELP_PET_HEADER_RE = re.compile(r"^##\s*@pet\s*:?\s*$", re.IGNORECASE)
_HELP_VOICE_SECRETARY_HEADER_RE = re.compile(r"^##\s*@voice_secretary\s*:?\s*$", re.IGNORECASE)
_HELP_H2_RE = re.compile(r"^##(?!#)\s+.*$")
_HELP_LEGACY_ROLE_SECTION_RE = re.compile(r"^##\s+Role Notes\s*$", re.IGNORECASE)
_HELP_H3_RE = re.compile(r"^###\s+(.+?)\s*$")


def _split_sections(markdown: str) -> list[str]:
    raw = str(markdown or "").replace("\r\n", "\n").replace("\r", "\n")
    if not raw:
        return [""]
    lines = raw.split("\n")
    sections: list[str] = []
    current: list[str] = []
    for line in lines:
        if _HELP_H2_RE.match(line) and current:
            sections.append("\n".join(current))
            current = [line]
            continue
        current.append(line)
    sections.append("\n".join(current))
    return sections


def _trim_block(text: str) -> str:
    return str(text or "").strip()


def _parse_tagged_section(section: str) -> Optional[dict[str, str]]:
    normalized = str(section or "").replace("\r\n", "\n").replace("\r", "\n")
    lines = normalized.split("\n")
    header = str(lines[0] or "")
    role_match = _HELP_ROLE_HEADER_RE.match(header)
    if role_match:
        role = str(role_match.group(1) or "").strip().lower()
        body = _trim_block("\n".join(lines[1:]))
        if role in {"foreman", "peer"}:
            return {"kind": "role", "key": f"role:{role}", "raw": _trim_block(normalized), "body": body}
        return {"kind": "extra", "key": f"role:{role}", "raw": _trim_block(normalized), "body": body}
    actor_match = _HELP_ACTOR_HEADER_RE.match(header)
    if actor_match:
        actor_id = str(actor_match.group(1) or "").strip()
        inline_body = str(actor_match.group(2) or "").strip()
        body_lines = [inline_body] if inline_body else []
        body_lines.extend(lines[1:])
        return {
            "kind": "actor" if actor_id else "extra",
            "key": f"actor:{actor_id}" if actor_id else "actor:",
            "raw": _trim_block(normalized),
            "body": _trim_block("\n".join(body_lines)),
        }
    if _HELP_PET_HEADER_RE.match(header):
        return {
            "kind": "extra",
            "key": "legacy:pet",
            "raw": _trim_block(normalized),
            "body": _trim_block("\n".join(lines[1:])),
        }
    if _HELP_VOICE_SECRETARY_HEADER_RE.match(header):
        return {
            "kind": "voice_secretary",
            "key": "voice_secretary",
            "raw": _trim_block(normalized),
            "body": _trim_block("\n".join(lines[1:])),
        }
    return None


def _try_extract_legacy_role_notes(common: str) -> dict[str, Any]:
    normalized = str(common or "").replace("\r\n", "\n").replace("\r", "\n")
    sections = _split_sections(normalized)
    kept: list[str] = []
    foreman = ""
    peer = ""
    used = False

    for section in sections:
        raw = _trim_block(section)
        if not raw:
            continue
        lines = raw.split("\n")
        if not _HELP_LEGACY_ROLE_SECTION_RE.match(str(lines[0] or "")):
            kept.append(raw)
            continue
        body_lines = lines[1:]
        chunks: list[dict[str, str]] = []
        current_title = ""
        current_body: list[str] = []
        stray = False
        for line in body_lines:
            h3 = _HELP_H3_RE.match(line)
            if h3:
                if current_title:
                    chunks.append({"title": current_title, "body": _trim_block("\n".join(current_body))})
                elif _trim_block("\n".join(current_body)):
                    stray = True
                current_title = str(h3.group(1) or "").strip()
                current_body = []
                continue
            current_body.append(line)
        if current_title:
            chunks.append({"title": current_title, "body": _trim_block("\n".join(current_body))})
        elif _trim_block("\n".join(current_body)):
            stray = True
        if stray or not chunks:
            kept.append(raw)
            continue
        local_foreman = ""
        local_peer = ""
        unknown = False
        for chunk in chunks:
            title = str(chunk.get("title") or "").strip().lower()
            if title == "foreman":
                local_foreman = str(chunk.get("body") or "")
            elif title == "peer":
                local_peer = str(chunk.get("body") or "")
            else:
                unknown = True
                break
        if unknown:
            kept.append(raw)
            continue
        foreman = local_foreman
        peer = local_peer
        used = True

    return {
        "common": "\n\n".join(kept).strip(),
        "foreman": foreman,
        "peer": peer,
        "used_legacy_role_notes": used,
    }


def parse_help_markdown(markdown: str) -> dict[str, Any]:
    sections = _split_sections(markdown)
    common_sections: list[str] = []
    actor_notes: dict[str, str] = {}
    extra_tagged_blocks: list[str] = []
    foreman = ""
    peer = ""
    voice_secretary = ""

    for section in sections:
        raw = _trim_block(section)
        if not raw:
            continue
        tagged = _parse_tagged_section(raw)
        if not tagged:
            common_sections.append(raw)
            continue
        kind = str(tagged.get("kind") or "")
        key = str(tagged.get("key") or "")
        body = str(tagged.get("body") or "")
        if kind == "role":
            if key == "role:foreman":
                foreman = body
            elif key == "role:peer":
                peer = body
            else:
                extra_tagged_blocks.append(str(tagged.get("raw") or ""))
            continue
        if kind == "actor":
            actor_id = key[len("actor:") :]
            if actor_id:
                actor_notes[actor_id] = body
            else:
                extra_tagged_blocks.append(str(tagged.get("raw") or ""))
            continue
        if kind == "voice_secretary":
            voice_secretary = body
            continue
        extra_tagged_blocks.append(str(tagged.get("raw") or ""))

    common = "\n\n".join(common_sections).strip()
    used_legacy_role_notes = False
    if not foreman and not peer:
        legacy = _try_extract_legacy_role_notes(common)
        common = str(legacy.get("common") or "")
        foreman = str(legacy.get("foreman") or "")
        peer = str(legacy.get("peer") or "")
        used_legacy_role_notes = bool(legacy.get("used_legacy_role_notes"))

    return {
        "common": common,
        "foreman": foreman,
        "peer": peer,
        "voice_secretary": voice_secretary,
        "actor_notes": actor_notes,
        "extra_tagged_blocks": extra_tagged_blocks,
        "used_legacy_role_notes": used_legacy_role_notes,
    }


def build_help_markdown(
    *,
    common: str,
    foreman: str,
    peer: str,
    actor_notes: dict[str, str],
    voice_secretary: str = "",
    actor_order: Optional[list[str]] = None,
    extra_tagged_blocks: Optional[list[str]] = None,
) -> str:
    parts: list[str] = []
    common_text = _trim_block(common)
    foreman_text = _trim_block(foreman)
    peer_text = _trim_block(peer)
    voice_secretary_text = _trim_block(voice_secretary)
    actor_notes_map = dict(actor_notes or {})
    extra_blocks = [_trim_block(item) for item in list(extra_tagged_blocks or []) if _trim_block(item)]

    if common_text:
        parts.append(common_text)
    if foreman_text:
        parts.append(f"## @role: foreman\n\n{foreman_text}")
    if peer_text:
        parts.append(f"## @role: peer\n\n{peer_text}")
    if voice_secretary_text:
        parts.append(f"## @voice_secretary\n\n{voice_secretary_text}")

    seen: set[str] = set()
    ordered_actor_ids: list[str] = []
    for actor_id in list(actor_order or []):
        aid = str(actor_id or "").strip()
        if not aid or aid in seen:
            continue
        seen.add(aid)
        ordered_actor_ids.append(aid)
    for actor_id in sorted(actor_notes_map.keys()):
        if actor_id in seen:
            continue
        seen.add(actor_id)
        ordered_actor_ids.append(actor_id)
    for actor_id in ordered_actor_ids:
        body = _trim_block(str(actor_notes_map.get(actor_id) or ""))
        if not body:
            continue
        parts.append(f"## @actor: {actor_id}\n\n{body}")

    parts.extend(extra_blocks)
    out = "\n\n".join([part for part in parts if part]).strip()
    return f"{out}\n" if out else ""


def update_actor_help_note(markdown: str, actor_id: str, note: str, actor_order: Optional[list[str]] = None) -> str:
    parsed = parse_help_markdown(markdown)
    next_actor_notes = dict(parsed.get("actor_notes") or {})
    aid = str(actor_id or "").strip()
    if aid:
        next_actor_notes[aid] = _trim_block(note)
    if aid and not next_actor_notes.get(aid):
        next_actor_notes.pop(aid, None)
    return build_help_markdown(
        common=str(parsed.get("common") or ""),
        foreman=str(parsed.get("foreman") or ""),
        peer=str(parsed.get("peer") or ""),
        voice_secretary=str(parsed.get("voice_secretary") or ""),
        actor_notes=next_actor_notes,
        actor_order=actor_order,
        extra_tagged_blocks=list(parsed.get("extra_tagged_blocks") or []),
    )


def _select_help_markdown(
    markdown: str,
    *,
    role: Optional[str],
    actor_id: Optional[str],
    include_voice_secretary: bool = False,
) -> str:
    """Filter CCCC_HELP markdown by optional conditional blocks.

    Supported markers (level-2 headings):
    - "## @role: foreman|peer"
    - "## @actor: <actor_id>"
    - "## @voice_secretary"

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
        if tag_kind == "legacy_pet":
            return False
        if tag_kind == "voice_secretary":
            return bool(include_voice_secretary)
        return True

    def _flush() -> None:
        nonlocal buf
        if buf and _include_block():
            out.extend(buf)
        buf = []

    for ln in lines:
        m_role = _HELP_ROLE_HEADER_RE.match(ln)
        m_actor = _HELP_ACTOR_HEADER_RE.match(ln)
        m_pet = _HELP_PET_HEADER_RE.match(ln)
        m_voice_secretary = _HELP_VOICE_SECRETARY_HEADER_RE.match(ln)
        is_h2 = bool(_HELP_H2_RE.match(ln))

        if m_role or m_actor or m_pet or m_voice_secretary:
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
                if m_actor:
                    tag_kind = "actor"
                    tag_value = str(m_actor.group(1) or "").strip()
                    ln = "## Notes for you"
                elif m_pet:
                    tag_kind = "legacy_pet"
                    tag_value = ""
                else:
                    tag_kind = "voice_secretary"
                    tag_value = ""
                    ln = "## Voice Secretary Operating Contract"
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
