from __future__ import annotations

from dataclasses import dataclass


_EXPECTED_VENDOR_VERSION = "0.8.0"


@dataclass
class NotebookLMCompatStatus:
    compatible: bool
    reason: str


def probe_notebooklm_vendor() -> NotebookLMCompatStatus:
    try:
        from ._vendor.notebooklm import __version__
        from ._vendor.notebooklm.auth import AuthTokens, extract_cookies_from_storage, fetch_tokens
        from ._vendor.notebooklm.client import NotebookLMClient

        if __version__ != _EXPECTED_VENDOR_VERSION:
            return NotebookLMCompatStatus(
                compatible=False,
                reason=(
                    "vendor version mismatch: "
                    f"expected {_EXPECTED_VENDOR_VERSION}, found {__version__}"
                ),
            )

        # Symbol-level checks keep the narrow CCCC adapter boundary explicit.
        _ = AuthTokens, extract_cookies_from_storage, fetch_tokens, NotebookLMClient
        return NotebookLMCompatStatus(compatible=True, reason="ok")
    except Exception as e:
        return NotebookLMCompatStatus(
            compatible=False,
            reason=f"vendor package unavailable: {e}",
        )
