from __future__ import annotations

from dataclasses import dataclass


@dataclass
class NotebookLMCompatStatus:
    compatible: bool
    reason: str


def probe_notebooklm_vendor() -> NotebookLMCompatStatus:
    try:
        from ._vendor.notebooklm.auth import AuthTokens, extract_cookies_from_storage, fetch_tokens
        from ._vendor.notebooklm.client import NotebookLMClient

        # Symbol-level checks keep this guard explicit and deterministic.
        _ = AuthTokens, extract_cookies_from_storage, fetch_tokens, NotebookLMClient
        return NotebookLMCompatStatus(compatible=True, reason="ok")
    except Exception as e:
        return NotebookLMCompatStatus(
            compatible=False,
            reason=f"vendor package unavailable: {e}",
        )
