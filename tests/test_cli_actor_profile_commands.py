import unittest
from argparse import Namespace
from unittest.mock import patch


class TestCliActorProfileCommands(unittest.TestCase):
    def test_parser_accepts_actor_profile_commands(self) -> None:
        from cccc import cli

        parser = cli.build_parser()

        args = parser.parse_args(["actor", "profile", "list", "--view", "my"])
        self.assertEqual(args.cmd, "actor")
        self.assertEqual(args.action, "profile")
        self.assertEqual(args.profile_action, "list")
        self.assertEqual(args.view, "my")

        args = parser.parse_args(["actor", "profile", "get", "shared", "--scope", "user", "--owner-id", "member-user"])
        self.assertEqual(args.profile_action, "get")
        self.assertEqual(args.profile_id, "shared")
        self.assertEqual(args.scope, "user")
        self.assertEqual(args.owner_id, "member-user")

        args = parser.parse_args(
            [
                "actor",
                "profile",
                "upsert",
                "--id",
                "shared",
                "--name",
                "Shared Profile",
                "--runtime",
                "claude",
                "--command",
                "claude --resume",
                "--capability-defaults",
                '{"autoload_capabilities":["pack:space"]}',
            ]
        )
        self.assertEqual(args.profile_action, "upsert")
        self.assertEqual(args.profile_id, "shared")
        self.assertEqual(args.name, "Shared Profile")
        self.assertEqual(args.runtime, "claude")
        self.assertEqual(args.command, "claude --resume")

        args = parser.parse_args(["actor", "profile", "delete", "shared", "--force-detach"])
        self.assertEqual(args.profile_action, "delete")
        self.assertEqual(args.profile_id, "shared")
        self.assertTrue(args.force_detach)

        args = parser.parse_args(["actor", "profile", "secrets", "shared", "--keys"])
        self.assertEqual(args.profile_action, "secrets")
        self.assertEqual(args.profile_id, "shared")
        self.assertTrue(args.keys)

    def test_actor_profile_list_routes_to_daemon(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"profiles": []}}

        args = Namespace(view="all", by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_list(args)

        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_profile_list")
        self.assertEqual(req.get("args", {}).get("view"), "all")
        self.assertEqual(req.get("args", {}).get("by"), "user")

    def test_actor_profile_list_rejects_invalid_view_before_daemon(self) -> None:
        from cccc import cli

        args = Namespace(view="team", by="user")
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_list(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_get_routes_scope_and_owner(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"profile": {"id": "shared"}}}

        args = Namespace(profile_id="shared", scope="user", owner_id="member-user", by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_get(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_profile_get")
        self.assertEqual(req.get("args", {}).get("profile_id"), "shared")
        self.assertEqual(req.get("args", {}).get("profile_scope"), "user")
        self.assertEqual(req.get("args", {}).get("profile_owner"), "member-user")

    def test_actor_profile_get_strips_owner_for_global_scope(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"profile": {"id": "shared"}}}

        args = Namespace(profile_id="shared", scope="global", owner_id="member-user", by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_get(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("args", {}).get("profile_scope"), "global")
        self.assertEqual(req.get("args", {}).get("profile_owner"), "")

    def test_actor_profile_get_rejects_invalid_scope_before_daemon(self) -> None:
        from cccc import cli

        args = Namespace(profile_id="shared", scope="team", owner_id="", by="user")
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_get(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_get_rejects_user_scope_without_owner_before_daemon(self) -> None:
        from cccc import cli

        args = Namespace(profile_id="shared", scope="user", owner_id="", by="user")
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_get(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_upsert_builds_profile_payload(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"profile": {"id": "shared"}}}

        args = Namespace(
            profile_id="shared",
            name="Shared Profile",
            runtime="claude",
            runner="pty",
            command="claude --resume",
            submit="newline",
            scope="user",
            owner_id="member-user",
            expected_revision=3,
            capability_defaults='{"autoload_capabilities":["pack:space"],"default_scope":"session"}',
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_upsert(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_profile_upsert")
        payload = req.get("args", {}).get("profile") or {}
        self.assertEqual(payload.get("id"), "shared")
        self.assertEqual(payload.get("name"), "Shared Profile")
        self.assertEqual(payload.get("runtime"), "claude")
        self.assertEqual(payload.get("runner"), "pty")
        self.assertEqual(payload.get("command"), ["claude", "--resume"])
        self.assertEqual(payload.get("submit"), "newline")
        self.assertEqual(payload.get("scope"), "user")
        self.assertEqual(payload.get("owner_id"), "member-user")
        self.assertEqual(payload.get("capability_defaults"), {"autoload_capabilities": ["pack:space"], "default_scope": "session"})
        self.assertEqual(req.get("args", {}).get("expected_revision"), 3)

    def test_actor_profile_upsert_strips_owner_for_global_scope(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"profile": {"id": "shared"}}}

        args = Namespace(
            profile_id="shared",
            name="Shared Profile",
            runtime="codex",
            runner="pty",
            command="",
            submit="enter",
            scope="global",
            owner_id="member-user",
            expected_revision=None,
            capability_defaults="",
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_upsert(args)

        self.assertEqual(code, 0)
        req = calls[0]
        payload = req.get("args", {}).get("profile") or {}
        self.assertEqual(payload.get("scope"), "global")
        self.assertEqual(payload.get("owner_id"), "")

    def test_actor_profile_delete_routes_force_detach(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"deleted": True}}

        args = Namespace(profile_id="shared", scope="global", owner_id="", force_detach=True, by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_delete(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_profile_delete")
        self.assertEqual(req.get("args", {}).get("profile_id"), "shared")
        self.assertEqual(req.get("args", {}).get("profile_scope"), "global")
        self.assertEqual(req.get("args", {}).get("profile_owner"), "")
        self.assertTrue(req.get("args", {}).get("force_detach"))

    def test_actor_profile_delete_strips_owner_for_global_scope(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"deleted": True}}

        args = Namespace(profile_id="shared", scope="global", owner_id="member-user", force_detach=False, by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_delete(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("args", {}).get("profile_scope"), "global")
        self.assertEqual(req.get("args", {}).get("profile_owner"), "")

    def test_actor_profile_upsert_rejects_invalid_capability_defaults_json(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            name="Shared Profile",
            runtime="codex",
            runner="pty",
            command="",
            submit="enter",
            scope="global",
            owner_id="",
            expected_revision=None,
            capability_defaults="{bad json",
            by="user",
        )
        with patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_upsert(args)

        self.assertEqual(code, 2)
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_capability_defaults")

    def test_actor_profile_upsert_rejects_invalid_expected_revision_before_daemon(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            name="Shared Profile",
            runtime="codex",
            runner="pty",
            command="",
            submit="enter",
            scope="global",
            owner_id="",
            expected_revision="abc",
            capability_defaults="",
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_upsert(args)

        self.assertEqual(code, 2)
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_upsert_rejects_invalid_expected_revision_without_daemon_check(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            name="Shared Profile",
            runtime="codex",
            runner="pty",
            command="",
            submit="enter",
            scope="global",
            owner_id="",
            expected_revision="abc",
            capability_defaults="",
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_upsert(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_upsert_rejects_user_scope_without_owner_before_daemon(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            name="Shared Profile",
            runtime="codex",
            runner="pty",
            command="",
            submit="enter",
            scope="user",
            owner_id="",
            expected_revision=None,
            capability_defaults="",
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_upsert(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_upsert_rejects_invalid_scope_before_daemon(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            name="Shared Profile",
            runtime="codex",
            runner="pty",
            command="",
            submit="enter",
            scope="team",
            owner_id="",
            expected_revision=None,
            capability_defaults="",
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_upsert(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_secrets_keys_routes_to_daemon(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"keys": ["OPENAI_API_KEY"]}}

        args = Namespace(profile_id="shared", scope="user", owner_id="member-user", keys=True, set=[], unset=[], clear=False, by="user")
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_secrets(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_profile_secret_keys")
        self.assertEqual(req.get("args", {}).get("profile_id"), "shared")
        self.assertEqual(req.get("args", {}).get("profile_scope"), "user")
        self.assertEqual(req.get("args", {}).get("profile_owner"), "member-user")

    def test_actor_profile_secrets_rejects_keys_mode_with_update_flags(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            scope="user",
            owner_id="member-user",
            keys=True,
            set=["OPENAI_API_KEY=secret"],
            unset=[],
            clear=False,
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_secrets(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_profile_secrets_update_routes_set_unset_clear(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"updated": True}}

        args = Namespace(
            profile_id="shared",
            scope="global",
            owner_id="",
            keys=False,
            set=["OPENAI_API_KEY=secret", "MODEL=gpt-5"],
            unset=["OLD_KEY"],
            clear=True,
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_profile_secrets(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_profile_secret_update")
        self.assertEqual(req.get("args", {}).get("set"), {"OPENAI_API_KEY": "secret", "MODEL": "gpt-5"})
        self.assertEqual(req.get("args", {}).get("unset"), ["OLD_KEY"])
        self.assertTrue(req.get("args", {}).get("clear"))

    def test_actor_profile_secrets_update_rejects_empty_operation(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            scope="global",
            owner_id="",
            keys=False,
            set=[],
            unset=[],
            clear=False,
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_secrets(args)

        self.assertEqual(code, 2)
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "empty_secret_update")

    def test_actor_profile_secrets_update_rejects_empty_operation_before_daemon_check(self) -> None:
        from cccc import cli

        args = Namespace(
            profile_id="shared",
            scope="global",
            owner_id="",
            keys=False,
            set=["MALFORMED"],
            unset=[],
            clear=False,
            by="user",
        )
        with patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_profile_secrets(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "empty_secret_update")


if __name__ == "__main__":
    unittest.main()
