"""
Daemon operation handlers.

This package splits daemon/server.py handler logic into feature-focused modules:
- group_ops: group operations
- actor_ops: actor operations
- message_ops: messaging operations
- context_ops: context operations
- runner_ops: runner operations (PTY + headless)
"""

from __future__ import annotations
