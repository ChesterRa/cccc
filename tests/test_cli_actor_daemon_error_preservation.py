import unittest
from argparse import Namespace
from types import SimpleNamespace
from unittest.mock import patch


class TestCliActorDaemonErrorPreservation(unittest.TestCase):
    def test_actor_list_preserves_daemon_error(self) -> None:
        from cccc import cli

        daemon_resp = {
            "ok": False,
            "error": {"code": "permission_denied", "message": "not allowed"},
        }
        with patch.object(cli, "_resolve_group_id", return_value="g_test"), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "list_actors", side_effect=AssertionError("local fallback must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_list(Namespace(group="g_test"))

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "permission_denied")

    def test_actor_remove_preserves_daemon_error(self) -> None:
        from cccc import cli

        daemon_resp = {
            "ok": False,
            "error": {"code": "permission_denied", "message": "not allowed"},
        }
        with patch.object(cli, "load_group", return_value=SimpleNamespace(group_id="g_test", doc={"scopes": []})), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "remove_actor", side_effect=AssertionError("local fallback must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_remove(Namespace(group="g_test", actor_id="peer-a", by="user"))

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "permission_denied")

    def test_actor_start_preserves_daemon_error(self) -> None:
        from cccc import cli

        daemon_resp = {
            "ok": False,
            "error": {"code": "permission_denied", "message": "not allowed"},
        }
        with patch.object(cli, "load_group", return_value=SimpleNamespace(group_id="g_test", doc={"scopes": []})), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "update_actor", side_effect=AssertionError("local fallback must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_start(Namespace(group="g_test", actor_id="peer-a", by="user"))

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "permission_denied")

    def test_actor_stop_preserves_daemon_error(self) -> None:
        from cccc import cli

        daemon_resp = {
            "ok": False,
            "error": {"code": "permission_denied", "message": "not allowed"},
        }
        with patch.object(cli, "load_group", return_value=SimpleNamespace(group_id="g_test", doc={"scopes": []})), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "update_actor", side_effect=AssertionError("local fallback must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_stop(Namespace(group="g_test", actor_id="peer-a", by="user"))

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "permission_denied")

    def test_actor_restart_preserves_daemon_error(self) -> None:
        from cccc import cli

        daemon_resp = {
            "ok": False,
            "error": {"code": "permission_denied", "message": "not allowed"},
        }
        with patch.object(cli, "load_group", return_value=SimpleNamespace(group_id="g_test", doc={"scopes": []})), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "update_actor", side_effect=AssertionError("local fallback must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_restart(Namespace(group="g_test", actor_id="peer-a", by="user"))

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "permission_denied")


if __name__ == "__main__":
    unittest.main()
