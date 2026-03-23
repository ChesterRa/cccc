import unittest
from argparse import Namespace
from unittest.mock import patch


class TestCliActorSecretsCommands(unittest.TestCase):
    def test_actor_secrets_rejects_keys_with_updates_before_daemon(self) -> None:
        from cccc import cli

        with patch.object(cli, "_resolve_group_id", return_value="g_test"), \
             patch.object(cli, "_ensure_daemon_running", side_effect=AssertionError("daemon check must not run")), \
             patch.object(cli, "call_daemon", side_effect=AssertionError("daemon call must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_secrets(
                Namespace(
                    group="g_test",
                    actor_id="peer-a",
                    by="user",
                    keys=True,
                    set=["OPENAI_API_KEY=secret"],
                    unset=[],
                    clear=False,
                    restart=False,
                )
            )

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_secrets_rejects_keys_with_restart_before_daemon(self) -> None:
        from cccc import cli

        with patch.object(cli, "_resolve_group_id", return_value="g_test"), \
             patch.object(cli, "_ensure_daemon_running", side_effect=AssertionError("daemon check must not run")), \
             patch.object(cli, "call_daemon", side_effect=AssertionError("daemon call must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_secrets(
                Namespace(
                    group="g_test",
                    actor_id="peer-a",
                    by="user",
                    keys=True,
                    set=[],
                    unset=[],
                    clear=False,
                    restart=True,
                )
            )

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_secrets_rejects_empty_update_before_daemon(self) -> None:
        from cccc import cli

        with patch.object(cli, "_resolve_group_id", return_value="g_test"), \
             patch.object(cli, "_ensure_daemon_running", side_effect=AssertionError("daemon check must not run")), \
             patch.object(cli, "call_daemon", side_effect=AssertionError("daemon call must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_secrets(
                Namespace(
                    group="g_test",
                    actor_id="peer-a",
                    by="user",
                    keys=False,
                    set=[],
                    unset=[],
                    clear=False,
                    restart=False,
                )
            )

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "empty_secret_update")


if __name__ == "__main__":
    unittest.main()
