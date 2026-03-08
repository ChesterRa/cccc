from __future__ import annotations

import unittest
from unittest.mock import patch

from cccc.ports.mcp.handlers import cccc_group_actor as actor_handler


class TestMcpActorRunnerSurface(unittest.TestCase):
    def test_actor_profile_list_filters_headless_profiles(self) -> None:
        with patch.object(
            actor_handler,
            "_call_daemon_or_raise",
            return_value={
                "profiles": [
                    {"id": "p1", "runner": "pty"},
                    {"id": "p2", "runner": "headless"},
                    {"id": "p3"},
                ]
            },
        ):
            resp = actor_handler.actor_profile_list(by="peer")
        self.assertEqual(resp, {"profiles": [{"id": "p1", "runner": "pty"}, {"id": "p3"}]})

    def test_actor_add_rejects_headless_runner(self) -> None:
        with self.assertRaisesRegex(ValueError, "headless runner is internal-only"):
            actor_handler.actor_add(group_id="g1", by="peer", actor_id="a1", runner="headless")

    def test_actor_add_rejects_headless_profile(self) -> None:
        calls: list[dict] = []

        def fake_call(req: dict) -> dict:
            calls.append(req)
            if req["op"] == "actor_profile_get":
                return {"profile": {"runner": "headless"}}
            raise AssertionError(f"unexpected daemon op: {req['op']}")

        with patch.object(actor_handler, "_call_daemon_or_raise", side_effect=fake_call):
            with self.assertRaisesRegex(ValueError, "headless runner is internal-only"):
                actor_handler.actor_add(group_id="g1", by="peer", actor_id="a1", profile_id="prof-headless")

        self.assertEqual([req["op"] for req in calls], ["actor_profile_get"])

    def test_actor_add_forces_pty_runner(self) -> None:
        calls: list[dict] = []

        def fake_call(req: dict) -> dict:
            calls.append(req)
            if req["op"] == "actor_add":
                return {"actor": {"id": "a1"}}
            raise AssertionError(f"unexpected daemon op: {req['op']}")

        with patch.object(actor_handler, "_call_daemon_or_raise", side_effect=fake_call):
            resp = actor_handler.actor_add(group_id="g1", by="peer", actor_id="a1", runner="pty")

        self.assertEqual(resp, {"actor": {"id": "a1"}})
        self.assertEqual(calls, [{"op": "actor_add", "args": {"group_id": "g1", "actor_id": "a1", "runtime": "codex", "runner": "pty", "title": "", "command": [], "env": {}, "by": "peer"}}])


if __name__ == "__main__":
    unittest.main()
