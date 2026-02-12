import unittest
from argparse import Namespace
from unittest.mock import patch


class TestCliGroupSetStateStopped(unittest.TestCase):
    def test_parser_accepts_stopped_choice(self) -> None:
        from cccc import cli

        parser = cli.build_parser()
        args = parser.parse_args(["group", "set-state", "stopped"])
        self.assertEqual(args.state, "stopped")

    def test_set_state_stopped_routes_to_group_stop(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"group_id": "g_test"}}

        args = Namespace(group="g_test", state="stopped", by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_group_set_state(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_stop")
        self.assertEqual(req.get("args", {}).get("group_id"), "g_test")


if __name__ == "__main__":
    unittest.main()
