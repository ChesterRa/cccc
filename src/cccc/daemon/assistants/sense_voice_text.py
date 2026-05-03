from __future__ import annotations

import re


_SENSE_VOICE_TAG_RE = re.compile(r"<\|[^<>|]{1,48}\|>")


def clean_sense_voice_text(text: str) -> str:
    cleaned = _SENSE_VOICE_TAG_RE.sub("", str(text or ""))
    cleaned = re.sub(r"\s+", " ", cleaned).strip()
    return cleaned
