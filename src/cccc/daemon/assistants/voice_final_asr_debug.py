from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

from ...util.time import utc_now_iso
from ...util.fs import atomic_write_text


_ASCII_WORD_RE = re.compile(r"[A-Za-z][A-Za-z'-]{2,}")
_CJK_RE = re.compile(r"[\u3400-\u9fff]")


def voice_final_asr_quality_flags(text: str) -> dict[str, Any]:
    value = str(text or "")
    ascii_words = _ASCII_WORD_RE.findall(value)
    cjk_chars = _CJK_RE.findall(value)
    suspicious = [
        word
        for word in ascii_words
        if len(word) >= 4 and word.lower() not in {
            "agent",
            "agents",
            "adaptive",
            "computing",
            "language",
            "token",
            "tokens",
            "scaffold",
        }
    ]
    total_chars = max(1, len(value.replace(" ", "")))
    return {
        "ascii_word_count": len(ascii_words),
        "cjk_char_count": len(cjk_chars),
        "english_token_ratio": round(len("".join(ascii_words)) / total_chars, 4),
        "suspicious_ascii_fragment_count": len(suspicious),
        "suspicious_ascii_fragments": suspicious[:12],
    }


def write_voice_final_asr_debug_artifact(path: Path, payload: dict[str, Any]) -> str:
    artifact = {
        "schema": 1,
        "created_at": utc_now_iso(),
        **payload,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_text(path, json.dumps(artifact, ensure_ascii=False, sort_keys=True, indent=2), encoding="utf-8")
    return str(path)
