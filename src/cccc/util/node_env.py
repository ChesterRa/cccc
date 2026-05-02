from __future__ import annotations

import os
from collections.abc import Mapping, MutableMapping
from typing import Any

from .conv import coerce_bool

_SUPPRESS_ENV = "CCCC_SUPPRESS_NODE_DEPRECATION_WARNINGS"
_NO_DEPRECATION_FLAG = "--no-deprecation"
_NO_WARNINGS_FLAG = "--no-warnings"
_EXPLICIT_DEPRECATION_FLAGS = (
    _NO_DEPRECATION_FLAG,
    _NO_WARNINGS_FLAG,
    "--trace-deprecation",
    "--throw-deprecation",
    "--pending-deprecation",
)


def _suppression_enabled(env: Mapping[str, Any] | None = None) -> bool:
    source = env if env is not None else os.environ
    return coerce_bool(source.get(_SUPPRESS_ENV), default=True)


def with_node_deprecation_warnings_suppressed(env: Mapping[str, Any] | None = None) -> dict[str, str]:
    """Return a child-process env with noisy Node deprecation warnings disabled.

    CCCC often launches Node-based runtimes, MCP servers, and Playwright drivers. Under newer Node
    releases, dependencies may emit DEP0169 (`url.parse()`) warnings to stderr even though CCCC
    cannot fix the transitive dependency directly. Keep this limited to deprecation warnings and
    preserve user-provided NODE_OPTIONS.
    """
    source = env if env is not None else os.environ
    out = {str(k): str(v) for k, v in source.items() if isinstance(k, str)}
    if not _suppression_enabled(out):
        return out

    existing = str(out.get("NODE_OPTIONS") or "").strip()
    if any(flag in existing for flag in _EXPLICIT_DEPRECATION_FLAGS):
        return out
    out["NODE_OPTIONS"] = f"{existing} {_NO_DEPRECATION_FLAG}".strip()
    return out


def suppress_node_deprecation_warnings_in_process(env: MutableMapping[str, str] | None = None) -> None:
    """Apply the same Node warning policy to the current process environment."""
    target = env if env is not None else os.environ
    updated = with_node_deprecation_warnings_suppressed(target)
    node_options = updated.get("NODE_OPTIONS")
    if node_options is not None:
        target["NODE_OPTIONS"] = node_options
