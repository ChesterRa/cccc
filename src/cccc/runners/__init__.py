from __future__ import annotations

import os

from . import headless

if os.name == "nt":
    # Windows uses ConPTY via pywinpty when available; fall back to stub otherwise.
    try:
        from . import pty_win as pty  # type: ignore
        if not bool(getattr(pty, "PTY_SUPPORTED", False)):
            from . import pty_stub as pty  # type: ignore
    except Exception:
        from . import pty_stub as pty  # type: ignore
else:
    try:
        from . import pty
    except ImportError:
        # Some Python builds lack POSIX PTY dependencies (termios/fcntl). Fall back to stub.
        from . import pty_stub as pty  # type: ignore

__all__ = ["headless", "pty"]
