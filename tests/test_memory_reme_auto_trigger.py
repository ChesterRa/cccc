from __future__ import annotations

import os
import tempfile
import time
import unittest

from cccc.util.fs import read_json


class TestMemoryRemeAutoTrigger(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_on_new_message_triggers_auto_cycle(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.automation.engine import AutomationManager
            from cccc.kernel.group import load_group
            from cccc.kernel.ledger import append_event

            create_resp, _ = self._call("group_create", {"title": "auto-trigger", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            gid = str((create_resp.result or {}).get("group_id") or "")
            self.assertTrue(gid)

            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc.setdefault("automation", {})
            group.doc["automation"].update(
                {
                    "memory_auto_enabled": True,
                    "memory_auto_min_new_messages": 1,
                    "memory_auto_min_interval_seconds": 0,
                    "memory_auto_max_messages": 240,
                    "memory_auto_context_window_tokens": 3000,
                    "memory_auto_reserve_tokens": 200,
                    "memory_auto_keep_recent_tokens": 500,
                    "memory_auto_signal_pack_token_budget": 100,
                }
            )
            group.save()

            # Seed one active task so signal pack is non-empty.
            self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "AutoTrigger", "goal": "trigger memory lane"}],
                },
            )

            for i in range(120):
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=gid,
                    scope_key="",
                    by=("user" if i % 2 == 0 else "peer1"),
                    data={"text": f"msg {i} " + ("z" * 180), "to": []},
                )

            state_path = group.path / "state" / "automation.json"
            started = time.monotonic()
            AutomationManager().on_new_message(group)
            self.assertLess(time.monotonic() - started, 0.2)

            state = {}
            deadline = time.monotonic() + 5.0
            last_result = {}
            memory_auto = {}
            while time.monotonic() < deadline:
                state = read_json(state_path)
                self.assertIsInstance(state, dict)
                assert isinstance(state, dict)
                memory_auto = state.get("memory_auto") if isinstance(state.get("memory_auto"), dict) else {}
                last_result = memory_auto.get("last_result") if isinstance(memory_auto.get("last_result"), dict) else {}
                if str(memory_auto.get("last_result_at") or "").strip():
                    break
                time.sleep(0.05)

            self.assertIn(str(last_result.get("status") or ""), {"written", "silent"})
            self.assertTrue(str(memory_auto.get("last_result_at") or "").strip())
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
