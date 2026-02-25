import unittest
from argparse import Namespace
from unittest.mock import patch


class TestCliSpaceCommands(unittest.TestCase):
    def test_parser_accepts_space_commands(self) -> None:
        from cccc import cli

        parser = cli.build_parser()

        args = parser.parse_args(["space", "status"])
        self.assertEqual(args.cmd, "space")
        self.assertEqual(args.action, "status")

        args = parser.parse_args(["space", "bind", "nb_123"])
        self.assertEqual(args.action, "bind")
        self.assertEqual(args.remote_space_id, "nb_123")

        args = parser.parse_args(["space", "bind"])
        self.assertEqual(args.action, "bind")
        self.assertEqual(args.remote_space_id, "")

        args = parser.parse_args(["space", "jobs", "retry", "spj_1"])
        self.assertEqual(args.action, "jobs")
        self.assertEqual(args.jobs_action, "retry")
        self.assertEqual(args.job_id, "spj_1")

        args = parser.parse_args(["space", "credential", "status"])
        self.assertEqual(args.action, "credential")
        self.assertEqual(args.credential_action, "status")

        args = parser.parse_args(["space", "health"])
        self.assertEqual(args.action, "health")

        args = parser.parse_args(["space", "auth", "status"])
        self.assertEqual(args.action, "auth")
        self.assertEqual(args.auth_action, "status")

        args = parser.parse_args(["space", "auth", "start", "--timeout-seconds", "120"])
        self.assertEqual(args.action, "auth")
        self.assertEqual(args.auth_action, "start")
        self.assertEqual(args.timeout_seconds, 120)

        args = parser.parse_args(["space", "sync"])
        self.assertEqual(args.action, "sync")

    def test_space_bind_routes_to_group_space_bind(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"binding": {"status": "bound"}}}

        args = Namespace(group="g_test", provider="notebooklm", by="user", remote_space_id="nb_123")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_space_bind(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_space_bind")
        self.assertEqual(req.get("args", {}).get("group_id"), "g_test")
        self.assertEqual(req.get("args", {}).get("action"), "bind")
        self.assertEqual(req.get("args", {}).get("remote_space_id"), "nb_123")

    def test_space_ingest_invalid_payload_rejected_before_daemon_call(self) -> None:
        from cccc import cli

        args = Namespace(
            group="g_test",
            provider="notebooklm",
            by="user",
            kind="context_sync",
            payload="{bad json",
            idempotency_key="",
        )
        with patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_space_ingest(args)

        self.assertEqual(code, 2)
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_payload")

    def test_space_jobs_cancel_routes_to_group_space_jobs(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"job": {"state": "canceled"}}}

        args = Namespace(group="g_test", provider="notebooklm", by="user", job_id="spj_1")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_space_jobs_cancel(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_space_jobs")
        self.assertEqual(req.get("args", {}).get("group_id"), "g_test")
        self.assertEqual(req.get("args", {}).get("action"), "cancel")
        self.assertEqual(req.get("args", {}).get("job_id"), "spj_1")

    def test_space_sync_routes_to_group_space_sync(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"sync_result": {"converged": True}}}

        args = Namespace(group="g_test", provider="notebooklm", by="user", force=True)
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_space_sync(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_space_sync")
        self.assertEqual(req.get("args", {}).get("group_id"), "g_test")
        self.assertEqual(req.get("args", {}).get("action"), "run")
        self.assertEqual(req.get("args", {}).get("force"), True)

    def test_space_credential_set_routes_to_provider_credential_update(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"provider": "notebooklm"}}

        args = Namespace(
            provider="notebooklm",
            by="user",
            auth_json='{"cookies":[{"name":"SID","value":"x","domain":".google.com"}]}',
            auth_json_file="",
        )
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_space_credential_set(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_space_provider_credential_update")
        self.assertEqual(req.get("args", {}).get("provider"), "notebooklm")
        self.assertEqual(req.get("args", {}).get("clear"), False)

    def test_space_health_routes_to_provider_health_check(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"healthy": True}}

        args = Namespace(provider="notebooklm", by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_space_health(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_space_provider_health_check")
        self.assertEqual(req.get("args", {}).get("provider"), "notebooklm")

    def test_space_auth_start_routes_to_provider_auth(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"auth": {"state": "running"}}}

        args = Namespace(provider="notebooklm", by="user", timeout_seconds=120)
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_space_auth_start(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_space_provider_auth")
        self.assertEqual(req.get("args", {}).get("action"), "start")
        self.assertEqual(req.get("args", {}).get("provider"), "notebooklm")
        self.assertEqual(req.get("args", {}).get("timeout_seconds"), 120)

    def test_space_auth_status_routes_to_provider_auth(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"auth": {"state": "idle"}}}

        args = Namespace(provider="notebooklm", by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_space_auth_status(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "group_space_provider_auth")
        self.assertEqual(req.get("args", {}).get("action"), "status")
        self.assertEqual(req.get("args", {}).get("provider"), "notebooklm")


if __name__ == "__main__":
    unittest.main()
