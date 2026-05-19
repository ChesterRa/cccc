"""Developer-mode chat request timing diagnostics."""

from __future__ import annotations

import logging
import time
import uuid
from dataclasses import dataclass, field
from typing import Callable


def _ms_since(started_at: float) -> int:
    return int(max(0.0, time.monotonic() - started_at) * 1000)


@dataclass
class ChatRequestDiagnostics:
    op: str
    group_id: str
    enabled: bool
    logger: logging.Logger
    client_id: str = ""
    reply_to: str = ""
    request_id: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    _started_at: float = field(default_factory=time.monotonic)
    _last_mark_at: float = field(default_factory=time.monotonic)

    def start(self) -> None:
        if not self.enabled:
            return
        self.logger.info(
            "chat request start op=%s group=%s request_id=%s client_id=%s reply_to=%s",
            self.op,
            self.group_id,
            self.request_id,
            self.client_id,
            self.reply_to,
        )

    def mark(self, phase: str) -> None:
        if not self.enabled:
            return
        now = time.monotonic()
        self.logger.info(
            "chat request phase op=%s group=%s phase=%s request_id=%s total_ms=%d delta_ms=%d",
            self.op,
            self.group_id,
            str(phase or "").strip() or "unknown",
            self.request_id,
            int(max(0.0, now - self._started_at) * 1000),
            int(max(0.0, now - self._last_mark_at) * 1000),
        )
        self._last_mark_at = now

    def done(self, *, ok: bool, error_code: str = "", event_id: str = "") -> None:
        if not self.enabled:
            return
        self.logger.info(
            "chat request done op=%s group=%s request_id=%s ok=%s error=%s event_id=%s total_ms=%d",
            self.op,
            self.group_id,
            self.request_id,
            bool(ok),
            str(error_code or "").strip(),
            str(event_id or "").strip(),
            _ms_since(self._started_at),
        )


def make_chat_diagnostics(
    *,
    op: str,
    group_id: str,
    client_id: str = "",
    reply_to: str = "",
    diagnostics_enabled: Callable[[], bool] | None = None,
    logger: logging.Logger,
) -> ChatRequestDiagnostics:
    enabled = bool(diagnostics_enabled and diagnostics_enabled())
    diag = ChatRequestDiagnostics(
        op=str(op or "").strip() or "unknown",
        group_id=str(group_id or "").strip(),
        client_id=str(client_id or "").strip(),
        reply_to=str(reply_to or "").strip(),
        enabled=enabled,
        logger=logger,
    )
    diag.start()
    return diag
