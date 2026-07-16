from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Dict, List

from ..common import MCPError


_CJK_HAN_RE = re.compile(r"[\u4e00-\u9fff]")
_CJK_KANA_RE = re.compile(r"[\u3040-\u30ff]")
_CJK_HANGUL_RE = re.compile(r"[\uac00-\ud7af]")
_SPACE_QUERY_OPTION_KEYS = {"source_ids"}


def _infer_language_from_text(text: str) -> str:
    raw = str(text or "")
    if not raw.strip():
        return ""
    if _CJK_KANA_RE.search(raw):
        return "ja"
    if _CJK_HANGUL_RE.search(raw):
        return "ko"
    if _CJK_HAN_RE.search(raw):
        return "zh-CN"
    return ""


def _infer_artifact_language_from_source(source_hint: str) -> str:
    source = str(source_hint or "").strip()
    if not source:
        return ""
    try:
        p = Path(source).expanduser().resolve()
    except Exception:
        p = None
    if p is not None and p.exists() and p.is_file():
        try:
            blob = p.read_bytes()[:8192]
            text = blob.decode("utf-8", errors="ignore")
            hint = _infer_language_from_text(text)
            if hint:
                return hint
        except Exception:
            return ""
    return _infer_language_from_text(source)


def _normalize_space_query_options_mcp(arguments: Dict[str, Any]) -> Dict[str, Any]:
    allowed_top_level = {"group_id", "provider", "lane", "query", "options", "by"}
    top_keys = {str(k or "").strip() for k in arguments.keys()}
    top_keys.discard("")
    unknown_top = sorted(k for k in top_keys if k not in allowed_top_level)
    if unknown_top:
        if any(k in {"language", "lang"} for k in unknown_top):
            raise MCPError(
                code="invalid_request",
                message=(
                    "cccc_space(action=query) does not support top-level language/lang. "
                    "NotebookLM query API has no language parameter; put language requirements in query text."
                ),
            )
        raise MCPError(
            code="invalid_request",
            message=(
                "cccc_space(action=query) unsupported top-level args: "
                f"{', '.join(unknown_top)}. Supported args: group_id, provider, query, options."
            ),
        )

    options_raw = arguments.get("options")
    if options_raw is None:
        options: Dict[str, Any] = {}
    elif isinstance(options_raw, dict):
        options = dict(options_raw)
    else:
        raise MCPError(code="invalid_request", message="cccc_space(action=query) options must be an object")

    unsupported_options = sorted(k for k in options.keys() if str(k or "").strip() not in _SPACE_QUERY_OPTION_KEYS)
    if unsupported_options:
        if any(str(k or "").strip() in {"language", "lang"} for k in unsupported_options):
            raise MCPError(
                code="invalid_request",
                message=(
                    "cccc_space(action=query) options do not support language/lang. "
                    "NotebookLM query API has no language parameter; put language requirements in query text."
                ),
            )
        raise MCPError(
            code="invalid_request",
            message=(
                "cccc_space(action=query) unsupported options: "
                f"{', '.join(str(k or '').strip() for k in unsupported_options)}. "
                "Supported options: source_ids."
            ),
        )

    if "source_ids" in options:
        raw_source_ids = options.get("source_ids")
        if raw_source_ids is None:
            options["source_ids"] = []
        elif not isinstance(raw_source_ids, list):
            raise MCPError(
                code="invalid_request",
                message="cccc_space(action=query) options.source_ids must be an array of non-empty strings",
            )
        else:
            source_ids: List[str] = []
            for idx, item in enumerate(raw_source_ids):
                sid = str(item or "").strip()
                if not sid:
                    raise MCPError(
                        code="invalid_request",
                        message=f"cccc_space(action=query) options.source_ids[{idx}] must be a non-empty string",
                    )
                source_ids.append(sid)
            options["source_ids"] = source_ids

    return options
