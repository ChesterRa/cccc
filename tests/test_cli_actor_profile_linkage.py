import unittest
from argparse import Namespace
from types import SimpleNamespace
from unittest.mock import patch


class TestCliActorProfileLinkage(unittest.TestCase):
    def test_parser_accepts_actor_add_profile_link_args(self) -> None:
        from cccc import cli

        parser = cli.build_parser()
        args = parser.parse_args(
            [
                "actor",
                "add",
                "peer-a",
                "--profile-id",
                "shared-profile",
                "--profile-scope",
                "user",
                "--profile-owner-id",
                "member-user",
            ]
        )

        self.assertEqual(args.actor_id, "peer-a")
        self.assertEqual(args.profile_id, "shared-profile")
        self.assertEqual(args.profile_scope, "user")
        self.assertEqual(args.profile_owner_id, "member-user")

    def test_parser_accepts_actor_update_profile_link_args(self) -> None:
        from cccc import cli

        parser = cli.build_parser()
        args = parser.parse_args(
            [
                "actor",
                "update",
                "peer-a",
                "--profile-id",
                "shared-profile",
                "--profile-scope",
                "user",
                "--profile-owner-id",
                "member-user",
            ]
        )

        self.assertEqual(args.actor_id, "peer-a")
        self.assertEqual(args.profile_id, "shared-profile")
        self.assertEqual(args.profile_scope, "user")
        self.assertEqual(args.profile_owner_id, "member-user")

    def test_parser_accepts_actor_update_profile_action(self) -> None:
        from cccc import cli

        parser = cli.build_parser()
        args = parser.parse_args(
            [
                "actor",
                "update",
                "peer-a",
                "--profile-action",
                "convert_to_custom",
            ]
        )

        self.assertEqual(args.actor_id, "peer-a")
        self.assertEqual(args.profile_action, "convert_to_custom")

    def test_actor_add_routes_profile_linkage_to_daemon(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"actor": {"id": "peer-a"}}}

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title="",
            runtime="codex",
            command="",
            env=[],
            scope="",
            submit="enter",
            by="user",
            profile_id="shared-profile",
            profile_scope="user",
            profile_owner_id="member-user",
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_add(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_add")
        daemon_args = req.get("args", {})
        self.assertEqual(daemon_args.get("profile_id"), "shared-profile")
        self.assertEqual(daemon_args.get("profile_scope"), "user")
        self.assertEqual(daemon_args.get("profile_owner"), "member-user")

    def test_actor_add_strips_profile_owner_for_global_scope(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"actor": {"id": "peer-a"}}}

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title="",
            runtime="codex",
            command="",
            env=[],
            scope="",
            submit="enter",
            by="user",
            profile_id="shared-profile",
            profile_scope="global",
            profile_owner_id="member-user",
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_add(args)

        self.assertEqual(code, 0)
        daemon_args = calls[0].get("args", {})
        self.assertEqual(daemon_args.get("profile_scope"), "global")
        self.assertEqual(daemon_args.get("profile_owner"), "")

    def test_actor_add_profile_linkage_without_daemon_returns_explicit_error(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title="",
            runtime="codex",
            command="",
            env=[],
            scope="",
            submit="enter",
            by="user",
            profile_id="shared-profile",
            profile_scope="user",
            profile_owner_id="member-user",
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=False), \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_add(args)

        self.assertEqual(code, 2)
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "daemon_unavailable")

    def test_actor_add_rejects_user_scope_without_profile_owner_before_daemon(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title="",
            runtime="codex",
            command="",
            env=[],
            scope="",
            submit="enter",
            by="user",
            profile_id="shared-profile",
            profile_scope="user",
            profile_owner_id="",
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_add(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_add_profile_linkage_preserves_daemon_error(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title="",
            runtime="codex",
            command="",
            env=[],
            scope="",
            submit="enter",
            by="user",
            profile_id="missing-profile",
            profile_scope="user",
            profile_owner_id="member-user",
        )
        daemon_resp = {
            "ok": False,
            "error": {"code": "profile_not_found", "message": "profile not found: missing-profile"},
        }
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_add(args)

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "profile_not_found")

    def test_actor_add_without_profile_preserves_daemon_error(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title="",
            runtime="codex",
            command="",
            env=[],
            scope="",
            submit="enter",
            by="user",
            profile_id="",
            profile_scope="global",
            profile_owner_id="",
        )
        daemon_resp = {
            "ok": False,
            "error": {"code": "permission_denied", "message": "not allowed"},
        }
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "add_actor", side_effect=AssertionError("local fallback must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_add(args)

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "permission_denied")

    def test_actor_update_routes_profile_linkage_without_patch(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"actor": {"id": "peer-a"}}}

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title=None,
            runtime=None,
            command=None,
            env=[],
            scope="",
            submit=None,
            enabled=None,
            by="user",
            profile_id="shared-profile",
            profile_scope="user",
            profile_owner_id="member-user",
            profile_action=None,
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_update(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_update")
        daemon_args = req.get("args", {})
        self.assertEqual(daemon_args.get("profile_id"), "shared-profile")
        self.assertEqual(daemon_args.get("profile_scope"), "user")
        self.assertEqual(daemon_args.get("profile_owner"), "member-user")
        self.assertEqual(daemon_args.get("patch"), {})

    def test_actor_update_strips_profile_owner_for_global_scope(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"actor": {"id": "peer-a"}}}

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title=None,
            runtime=None,
            command=None,
            env=[],
            scope="",
            submit=None,
            enabled=None,
            by="user",
            profile_id="shared-profile",
            profile_scope="global",
            profile_owner_id="member-user",
            profile_action=None,
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_update(args)

        self.assertEqual(code, 0)
        daemon_args = calls[0].get("args", {})
        self.assertEqual(daemon_args.get("profile_scope"), "global")
        self.assertEqual(daemon_args.get("profile_owner"), "")

    def test_actor_update_routes_profile_action_without_patch(self) -> None:
        from cccc import cli

        calls = []

        def _fake_call_daemon(req):
            calls.append(req)
            return {"ok": True, "result": {"actor": {"id": "peer-a"}}}

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title=None,
            runtime=None,
            command=None,
            env=[],
            scope="",
            submit=None,
            enabled=None,
            by="user",
            profile_id="",
            profile_scope="global",
            profile_owner_id="",
            profile_action="convert_to_custom",
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", side_effect=_fake_call_daemon), \
             patch.object(cli, "_print_json"):
            code = cli.cmd_actor_update(args)

        self.assertEqual(code, 0)
        req = calls[0]
        self.assertEqual(req.get("op"), "actor_update")
        daemon_args = req.get("args", {})
        self.assertEqual(daemon_args.get("profile_action"), "convert_to_custom")
        self.assertEqual(daemon_args.get("patch"), {})

    def test_actor_update_profile_linkage_without_daemon_returns_explicit_error(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title=None,
            runtime=None,
            command=None,
            env=[],
            scope="",
            submit=None,
            enabled=None,
            by="user",
            profile_id="shared-profile",
            profile_scope="user",
            profile_owner_id="member-user",
            profile_action=None,
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=False), \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_update(args)

        self.assertEqual(code, 2)
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "daemon_unavailable")

    def test_actor_update_rejects_user_scope_without_profile_owner_before_daemon(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title=None,
            runtime=None,
            command=None,
            env=[],
            scope="",
            submit=None,
            enabled=None,
            by="user",
            profile_id="shared-profile",
            profile_scope="user",
            profile_owner_id="",
            profile_action=None,
        )
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running") as mock_daemon, \
             patch.object(cli, "call_daemon") as mock_call, \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_update(args)

        self.assertEqual(code, 2)
        mock_daemon.assert_not_called()
        mock_call.assert_not_called()
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_update_profile_linkage_preserves_daemon_invalid_request(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title=None,
            runtime=None,
            command=None,
            env=[],
            scope="",
            submit=None,
            enabled=None,
            by="user",
            profile_id="shared-profile",
            profile_scope="user",
            profile_owner_id="member-user",
            profile_action="convert_to_custom",
        )
        daemon_resp = {
            "ok": False,
            "error": {
                "code": "invalid_request",
                "message": "profile_action and profile_id are mutually exclusive",
            },
        }
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_update(args)

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "invalid_request")

    def test_actor_update_without_profile_preserves_daemon_error(self) -> None:
        from cccc import cli

        group = SimpleNamespace(doc={"scopes": []})
        args = Namespace(
            group="g_test",
            actor_id="peer-a",
            title="peer-a",
            runtime=None,
            command=None,
            env=[],
            scope="",
            submit=None,
            enabled=None,
            by="user",
            profile_id="",
            profile_scope="global",
            profile_owner_id="",
            profile_action=None,
        )
        daemon_resp = {
            "ok": False,
            "error": {"code": "permission_denied", "message": "not allowed"},
        }
        with patch.object(cli, "load_group", return_value=group), \
             patch.object(cli, "_ensure_daemon_running", return_value=True), \
             patch.object(cli, "call_daemon", return_value=daemon_resp), \
             patch.object(cli, "update_actor", side_effect=AssertionError("local fallback must not run")), \
             patch.object(cli, "_print_json") as mock_print:
            code = cli.cmd_actor_update(args)

        self.assertEqual(code, 2)
        printed = mock_print.call_args[0][0] if mock_print.call_args else {}
        self.assertEqual(str((printed.get("error") or {}).get("code") or ""), "permission_denied")


if __name__ == "__main__":
    unittest.main()
