import os
import tempfile
import unittest
from unittest.mock import patch


class TestEnabledFlagCoercion(unittest.TestCase):
    def _create_group(self) -> str:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        resp, _ = handle_request(
            DaemonRequest.model_validate(
                {"op": "group_create", "args": {"title": "coercion", "topic": "", "by": "user"}}
            )
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _add_actor(self, group_id: str, actor_id: str) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        resp, _ = handle_request(
            DaemonRequest.model_validate(
                {
                    "op": "actor_add",
                    "args": {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "runtime": "codex",
                        "runner": "headless",
                        "by": "user",
                    },
                }
            )
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))

    def test_roles_and_recipients_treat_string_false_as_disabled(self) -> None:
        from cccc.kernel.actors import find_actor, find_foreman, get_effective_role
        from cccc.kernel.group import load_group
        from cccc.kernel.messaging import disabled_recipient_actor_ids, enabled_recipient_actor_ids
        from cccc.kernel.system_prompt import render_system_prompt

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                gid = self._create_group()
                self._add_actor(gid, "peer1")
                self._add_actor(gid, "peer2")

                group = load_group(gid)
                self.assertIsNotNone(group)
                assert group is not None

                actor1 = find_actor(group, "peer1")
                actor2 = find_actor(group, "peer2")
                self.assertIsNotNone(actor1)
                self.assertIsNotNone(actor2)
                assert actor1 is not None and actor2 is not None

                actor1["enabled"] = "false"
                group.save()

                reloaded = load_group(gid)
                self.assertIsNotNone(reloaded)
                assert reloaded is not None

                foreman = find_foreman(reloaded)
                self.assertIsNotNone(foreman)
                self.assertEqual(str((foreman or {}).get("id") or ""), "peer2")
                self.assertEqual(get_effective_role(reloaded, "peer1"), "peer")
                self.assertEqual(get_effective_role(reloaded, "peer2"), "foreman")

                enabled_ids = enabled_recipient_actor_ids(reloaded, ["@all"])
                disabled_ids = disabled_recipient_actor_ids(reloaded, ["@all"])
                self.assertEqual(enabled_ids, ["peer2"])
                self.assertEqual(disabled_ids, ["peer1"])

                actor2_now = find_actor(reloaded, "peer2")
                self.assertIsNotNone(actor2_now)
                prompt = render_system_prompt(group=reloaded, actor=actor2_now or {})
                self.assertIn("team: solo", prompt)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_auto_wake_treats_string_false_as_disabled(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon import server as daemon_server
        from cccc.daemon.server import handle_request
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group
        from cccc.util.conv import coerce_bool

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                gid = self._create_group()
                self._add_actor(gid, "peer1")

                group = load_group(gid)
                self.assertIsNotNone(group)
                assert group is not None
                actor = find_actor(group, "peer1")
                self.assertIsNotNone(actor)
                assert actor is not None
                actor["enabled"] = "false"
                group.save()

                with patch.object(
                    daemon_server,
                    "_start_actor_process",
                    return_value={"success": True, "event": None, "effective_runner": "headless", "error": None},
                ) as start_mock:
                    send_resp, _ = handle_request(
                        DaemonRequest.model_validate(
                            {
                                "op": "send",
                                "args": {
                                    "group_id": gid,
                                    "by": "user",
                                    "text": "wake up",
                                    "to": ["peer1"],
                                },
                            }
                        )
                    )

                self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
                self.assertEqual(start_mock.call_count, 1)

                after = load_group(gid)
                self.assertIsNotNone(after)
                assert after is not None
                actor_after = find_actor(after, "peer1")
                self.assertIsNotNone(actor_after)
                self.assertTrue(coerce_bool((actor_after or {}).get("enabled"), default=False))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
