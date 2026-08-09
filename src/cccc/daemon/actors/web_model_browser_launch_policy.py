"""Launch policy for the shared ChatGPT projected browser."""

from __future__ import annotations

import os
import sys


_HEADLESS_ENV = "CCCC_WEB_MODEL_BROWSER_HEADLESS"
_TRUE_VALUES = {"1", "true", "yes", "on", "enabled"}
_FALSE_VALUES = {"0", "false", "no", "off", "disabled"}


def use_headless_projected_browser(*, platform: str | None = None) -> bool:
    """Keep macOS browser delivery in the embedded projection by default."""
    configured = str(os.environ.get(_HEADLESS_ENV, "") or "").strip().lower()
    if configured in _TRUE_VALUES:
        return True
    if configured in _FALSE_VALUES:
        return False
    return str(platform or sys.platform).strip().lower() == "darwin"
