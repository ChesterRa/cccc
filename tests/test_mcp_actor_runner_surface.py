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
            resp = actor_handler.actor_profile_list(group_id="g1", by="peer")
        self.assertEqual(resp, {"profiles": [{"id": "p1", "runner": "pty"}, {"id": "p3"}]})

    def test_actor_profile_list_keeps_headless_profiles_for_headless_caller(self) -> None:
        calls: list[dict] = []

        def fake_call(req: dict) -> dict:
            calls.append(req)
            if req["op"] == "actor_list":
                return {"actors": [{"id": "foreman", "runner": "headless"}]}
            if req["op"] == "actor_profile_list":
                return {
                    "profiles": [
                        {"id": "p1", "runner": "pty"},
                        {"id": "p2", "runner": "headless"},
                    ]
                }
            raise AssertionError(f"unexpected daemon op: {req['op']}")

        with patch.object(actor_handler, "_call_daemon_or_raise", side_effect=fake_call):
            resp = actor_handler.actor_profile_list(group_id="g1", by="foreman")

        self.assertEqual(resp, {"profiles": [{"id": "p1", "runner": "pty"}, {"id": "p2", "runner": "headless"}]})
        self.assertEqual([req["op"] for req in calls], ["actor_profile_list", "actor_list"])

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

        self.assertEqual([req["op"] for req in calls], ["actor_list", "actor_profile_get", "actor_list"])

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
        self.assertEqual(
            calls,
            [
                {"op": "actor_list", "args": {"group_id": "g1", "include_unread": False}},
                {"op": "actor_add", "args": {"group_id": "g1", "actor_id": "a1", "runtime": "codex", "runner": "pty", "title": "", "command": [], "env": {}, "by": "peer"}},
            ],
        )

    def test_actor_add_allows_headless_runner_for_headless_caller(self) -> None:
        calls: list[dict] = []

        def fake_call(req: dict) -> dict:
            calls.append(req)
            if req["op"] == "actor_list":
                return {"actors": [{"id": "foreman", "runner": "headless"}]}
            if req["op"] == "actor_add":
                return {"actor": {"id": "a1", "runner": "headless"}}
            raise AssertionError(f"unexpected daemon op: {req['op']}")

        with patch.object(actor_handler, "_call_daemon_or_raise", side_effect=fake_call):
            resp = actor_handler.actor_add(group_id="g1", by="foreman", actor_id="a1", runner="headless")

        self.assertEqual(resp, {"actor": {"id": "a1", "runner": "headless"}})
        self.assertEqual(
            calls,
            [
                {"op": "actor_list", "args": {"group_id": "g1", "include_unread": False}},
                {"op": "actor_add", "args": {"group_id": "g1", "actor_id": "a1", "runtime": "codex", "runner": "headless", "title": "", "command": [], "env": {}, "by": "foreman"}},
            ],
        )

    def test_actor_add_allows_headless_profile_for_headless_caller(self) -> None:
        calls: list[dict] = []

        def fake_call(req: dict) -> dict:
            calls.append(req)
            if req["op"] == "actor_profile_get":
                return {"profile": {"runner": "headless"}}
            if req["op"] == "actor_list":
                return {"actors": [{"id": "foreman", "runner": "headless"}]}
            if req["op"] == "actor_add":
                return {"actor": {"id": "a1", "runner": "headless"}}
            raise AssertionError(f"unexpected daemon op: {req['op']}")

        with patch.object(actor_handler, "_call_daemon_or_raise", side_effect=fake_call):
            resp = actor_handler.actor_add(group_id="g1", by="foreman", actor_id="a1", profile_id="prof-headless")

        self.assertEqual(resp, {"actor": {"id": "a1", "runner": "headless"}})
        self.assertEqual(
            calls,
            [
                {"op": "actor_list", "args": {"group_id": "g1", "include_unread": False}},
                {"op": "actor_profile_get", "args": {"profile_id": "prof-headless", "by": "user"}},
                {"op": "actor_list", "args": {"group_id": "g1", "include_unread": False}},
                {"op": "actor_add", "args": {"group_id": "g1", "actor_id": "a1", "runtime": "codex", "runner": "headless", "title": "", "command": [], "env": {}, "by": "foreman", "profile_id": "prof-headless"}},
            ],
        )


if __name__ == "__main__":
    unittest.main()
