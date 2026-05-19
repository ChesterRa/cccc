from __future__ import annotations

import logging
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestChatDiagnostics(unittest.TestCase):
    def _fake_group(self) -> SimpleNamespace:
        td = tempfile.TemporaryDirectory()
        self.addCleanup(td.cleanup)
        return SimpleNamespace(
            group_id="g_diag",
            doc={"active_scope_key": ""},
            ledger_path=Path(td.name) / "ledger.jsonl",
        )

    def test_send_logs_phase_diagnostics_when_enabled(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.util.conv import coerce_bool

        group = self._fake_group()
        event = {"id": "evt_1", "ts": "2026-05-19T00:00:00Z", "kind": "chat.message", "data": {}}

        with (
            patch("cccc.daemon.messaging.chat_ops.load_group", return_value=group),
            patch("cccc.daemon.messaging.chat_ops.resolve_recipient_tokens", return_value=["user"]),
            patch("cccc.daemon.messaging.chat_ops.append_event", return_value=event),
            patch("cccc.daemon.messaging.chat_ops.run_group_chat_post_commit"),
            patch("cccc.daemon.messaging.chat_ops.schedule_chat_side_effects"),
            self.assertLogs("cccc.daemon.server", level="INFO") as logs,
        ):
            resp = handle_send(
                {"group_id": "g_diag", "by": "user", "to": ["user"], "text": "hello"},
                coerce_bool=coerce_bool,
                normalize_attachments=lambda _group, _raw: [],
                effective_runner_kind=lambda runner: str(runner or "pty"),
                auto_wake_recipients=lambda _group, _to, _by: [],
                automation_on_resume=lambda _group: None,
                automation_on_new_message=lambda _group: None,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
                diagnostics_enabled=lambda: True,
            )

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        joined = "\n".join(logs.output)
        self.assertIn("chat request start op=send group=g_diag", joined)
        self.assertIn("chat request phase op=send group=g_diag phase=load_group", joined)
        self.assertIn("chat request phase op=send group=g_diag phase=resolve_recipients", joined)
        self.assertIn("chat request phase op=send group=g_diag phase=append_event", joined)
        self.assertIn("chat request phase op=send group=g_diag phase=schedule_delivery", joined)
        self.assertIn("chat request done op=send group=g_diag", joined)

    def test_send_suppresses_phase_diagnostics_when_disabled(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.util.conv import coerce_bool

        group = self._fake_group()
        event = {"id": "evt_1", "ts": "2026-05-19T00:00:00Z", "kind": "chat.message", "data": {}}
        records: list[logging.LogRecord] = []

        class _Capture(logging.Handler):
            def emit(self, record: logging.LogRecord) -> None:
                records.append(record)

        logger = logging.getLogger("cccc.daemon.server")
        handler = _Capture()
        logger.addHandler(handler)
        try:
            with (
                patch("cccc.daemon.messaging.chat_ops.load_group", return_value=group),
                patch("cccc.daemon.messaging.chat_ops.resolve_recipient_tokens", return_value=["user"]),
                patch("cccc.daemon.messaging.chat_ops.append_event", return_value=event),
                patch("cccc.daemon.messaging.chat_ops.run_group_chat_post_commit"),
                patch("cccc.daemon.messaging.chat_ops.schedule_chat_side_effects"),
            ):
                resp = handle_send(
                    {"group_id": "g_diag", "by": "user", "to": ["user"], "text": "hello"},
                    coerce_bool=coerce_bool,
                    normalize_attachments=lambda _group, _raw: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _kinds: None,
                    diagnostics_enabled=lambda: False,
                )
        finally:
            logger.removeHandler(handler)

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        messages = [record.getMessage() for record in records]
        self.assertFalse(any(message.startswith("chat request ") for message in messages))


if __name__ == "__main__":
    unittest.main()
