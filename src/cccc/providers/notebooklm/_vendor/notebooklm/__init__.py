"""Vendored notebooklm-py package for CCCC.

The upstream `notebooklm-py` package normally performs broad imports and logging
setup at package import time. For CCCC we keep this package init intentionally
minimal and import only specific submodules from the adapter boundary.
"""

from __future__ import annotations

__all__: list[str] = []
