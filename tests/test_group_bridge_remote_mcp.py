import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestGroupBridgeRemoteMcp(unittest.TestCase):
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

        return Path(td), cleanup

    def _create_group_with_scope(self, home: Path, *, title: str = "target"):
        from cccc.kernel.group import attach_scope_to_group, create_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope

        root = home / f"{title}-repo"
        root.mkdir(parents=True, exist_ok=True)
        (root / "README.md").write_text("project alpha\nbeta\n", encoding="utf-8")
        (root / "src").mkdir(exist_ok=True)
        (root / "src" / "app.py").write_text("print('alpha')\n", encoding="utf-8")
        reg = load_registry()
        group = create_group(reg, title=title, topic="")
        return attach_scope_to_group(reg, group, detect_scope(root), set_active=True), root

    def _tool_payload(self, response: dict) -> dict:
        result = response.get("result") if isinstance(response.get("result"), dict) else {}
        content = result.get("content") if isinstance(result.get("content"), list) else []
        text = str((content[0] if content and isinstance(content[0], dict) else {}).get("text") or "{}")
        parsed = json.loads(text)
        return parsed if isinstance(parsed, dict) else {"value": parsed}

    def _call_tool(self, name: str, arguments: dict, context) -> dict:
        from cccc.ports.mcp.group_bridge import handle_group_bridge_request

        return handle_group_bridge_request(
            {
                "jsonrpc": "2.0",
                "id": "test",
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            },
            context,
        )

    def _bridge_context(self, *, target_group_id: str, access_level: str):
        from cccc.ports.mcp.group_bridge import GroupBridgeContext

        return GroupBridgeContext(
            target_group_id=target_group_id,
            remote_group_id="g_remote",
            remote_peer_id="peer_remote",
            trust_id="ptrust_test",
            access_level=access_level,
        )

    def test_group_bridge_tool_specs_are_layered_and_do_not_enter_web_model(self) -> None:
        from cccc.ports.mcp.group_bridge import group_bridge_tool_specs
        from cccc.ports.mcp.server import _WEB_MODEL_FOREMAN_ADVERTISED_TOOL_NAMES, _WEB_MODEL_PEER_ADVERTISED_TOOL_NAMES

        self.assertEqual([item["name"] for item in group_bridge_tool_specs("messages")], ["cccc_remote_access"])
        read_names = {item["name"] for item in group_bridge_tool_specs("read")}
        self.assertIn("cccc_remote_repo", read_names)
        self.assertNotIn("cccc_remote_shell", read_names)
        full_names = {item["name"] for item in group_bridge_tool_specs("full")}
        self.assertIn("cccc_remote_exec_command", full_names)
        self.assertIn("cccc_remote_write_stdin", full_names)

        for name in full_names:
            self.assertNotIn(name, _WEB_MODEL_PEER_ADVERTISED_TOOL_NAMES)
            self.assertNotIn(name, _WEB_MODEL_FOREMAN_ADVERTISED_TOOL_NAMES)

        git_spec = next(item for item in group_bridge_tool_specs("read") if item["name"] == "cccc_remote_git")
        git_actions = git_spec["inputSchema"]["properties"]["action"]["enum"]
        self.assertEqual(git_actions, ["status", "diff", "log"])
        full_git_spec = next(item for item in group_bridge_tool_specs("full") if item["name"] == "cccc_remote_git")
        full_git_actions = full_git_spec["inputSchema"]["properties"]["action"]["enum"]
        self.assertEqual(full_git_actions, ["status", "diff", "log", "add", "commit"])
        repo_spec = next(item for item in group_bridge_tool_specs("read") if item["name"] == "cccc_remote_repo")
        repo_props = repo_spec["inputSchema"]["properties"]
        self.assertIn("remote_group_id", repo_props)
        self.assertNotIn("group_id", repo_props)

    def test_messages_only_denies_read_tools_without_chat_message(self) -> None:
        home, cleanup = self._with_home()
        try:
            group, _root = self._create_group_with_scope(home)
            context = self._bridge_context(target_group_id=group.group_id, access_level="messages")

            response = self._call_tool(
                "cccc_remote_repo",
                {"remote_group_id": group.group_id, "action": "read", "path": "README.md"},
                context,
            )

            self.assertTrue((response.get("result") or {}).get("isError"))
            payload = self._tool_payload(response)
            self.assertEqual(payload["error"]["code"], "bridge_read_not_granted")
            ledger_text = group.ledger_path.read_text(encoding="utf-8")
            self.assertNotIn('"kind": "chat.message"', ledger_text)
            self.assertIn('"kind": "group_bridge.mcp_denied"', ledger_text)
        finally:
            cleanup()

    def test_read_repo_search_and_path_guardrail(self) -> None:
        home, cleanup = self._with_home()
        try:
            group, _root = self._create_group_with_scope(home)
            context = self._bridge_context(target_group_id=group.group_id, access_level="read")

            response = self._call_tool(
                "cccc_remote_repo",
                {"remote_group_id": group.group_id, "action": "search", "query": "alpha"},
                context,
            )
            payload = self._tool_payload(response)
            paths = {item["path"] for item in payload["matches"]}
            self.assertIn("README.md", paths)
            self.assertIn("src/app.py", paths)

            denied = self._call_tool(
                "cccc_remote_repo",
                {"remote_group_id": group.group_id, "action": "read", "path": "../outside.txt"},
                context,
            )
            self.assertTrue((denied.get("result") or {}).get("isError"))
            self.assertEqual(self._tool_payload(denied)["error"]["code"], "invalid_path")
            self.assertIn('"kind": "group_bridge.path_guardrail_denied"', group.ledger_path.read_text(encoding="utf-8"))
        finally:
            cleanup()

    def test_read_access_rejects_git_mutation(self) -> None:
        home, cleanup = self._with_home()
        try:
            group, _root = self._create_group_with_scope(home)
            context = self._bridge_context(target_group_id=group.group_id, access_level="read")

            denied = self._call_tool(
                "cccc_remote_git",
                {"remote_group_id": group.group_id, "action": "add", "path": "README.md"},
                context,
            )

            self.assertTrue((denied.get("result") or {}).get("isError"))
            self.assertEqual(self._tool_payload(denied)["error"]["code"], "bridge_full_access_not_granted")
        finally:
            cleanup()

    def test_full_exec_session_write_is_rechecked_after_downgrade(self) -> None:
        home, cleanup = self._with_home()
        try:
            group, _root = self._create_group_with_scope(home)
            full_context = self._bridge_context(target_group_id=group.group_id, access_level="full")
            read_context = self._bridge_context(target_group_id=group.group_id, access_level="read")

            started = self._call_tool(
                "cccc_remote_exec_command",
                {"remote_group_id": group.group_id, "command": "cat", "yield_time_ms": 0},
                full_context,
            )
            payload = self._tool_payload(started)
            session_id = str(payload.get("session_id") or "")
            self.assertTrue(session_id)

            denied = self._call_tool(
                "cccc_remote_write_stdin",
                {"remote_group_id": group.group_id, "session_id": session_id, "chars": "hello\n", "yield_time_ms": 0},
                read_context,
            )
            self.assertTrue((denied.get("result") or {}).get("isError"))
            self.assertEqual(self._tool_payload(denied)["error"]["code"], "bridge_full_access_not_granted")

            terminated = self._call_tool(
                "cccc_remote_write_stdin",
                {"remote_group_id": group.group_id, "session_id": session_id, "terminate": True, "yield_time_ms": 0},
                full_context,
            )
            self.assertFalse((terminated.get("result") or {}).get("isError"))
        finally:
            cleanup()

    def test_bridge_access_grant_is_directional(self) -> None:
        from cccc.kernel.federation.pairing import (
            approve_pairing_request,
            create_pairing_invite,
            create_pairing_request,
            list_trusts,
            update_trust_access_level,
        )

        _home, cleanup = self._with_home()
        try:
            invite_a = create_pairing_invite(group_id="g_a", ttl_seconds=600)
            request_ab = create_pairing_request(
                invite_a["pairing_code"],
                requester_group_id="g_b",
                requester_group_title="Group B",
                requester_peer_id="peer_b",
                requester_endpoint="http://b.example:8848",
            )
            approve_pairing_request(request_ab["request_id"], approver_user_id="owner-a")

            invite_b = create_pairing_invite(group_id="g_b", ttl_seconds=600)
            request_ba = create_pairing_request(
                invite_b["pairing_code"],
                requester_group_id="g_a",
                requester_group_title="Group A",
                requester_peer_id="peer_a",
                requester_endpoint="http://a.example:8848",
            )
            approve_pairing_request(request_ba["request_id"], approver_user_id="owner-b")

            trust_a = list_trusts(group_id="g_a")[0]
            trust_b = list_trusts(group_id="g_b")[0]
            self.assertEqual(trust_a["access_level"], "messages")
            self.assertEqual(trust_b["access_level"], "messages")

            update_trust_access_level(trust_a["trust_id"], "read", updated_by="owner-a")

            self.assertEqual(list_trusts(group_id="g_a")[0]["access_level"], "read")
            self.assertEqual(list_trusts(group_id="g_b")[0]["access_level"], "messages")
        finally:
            cleanup()

    def test_web_group_bridge_endpoint_uses_federation_token_and_access_update(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.federation.pairing import (
            approve_pairing_request,
            create_pairing_invite,
            create_pairing_request,
            get_pairing_request_public_status,
            list_trusts,
            revoke_trust,
        )
        from cccc.ports.web.app import create_app

        home, cleanup = self._with_home()
        try:
            group, _root = self._create_group_with_scope(home)
            invite = create_pairing_invite(group_id=group.group_id, ttl_seconds=600)
            pairing_request = create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_group_title="Remote",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
            )
            approve_pairing_request(pairing_request["request_id"], approver_user_id="user-a")
            public_status = get_pairing_request_public_status(pairing_request["request_id"], invite_id=invite["invite_id"])
            token = str((public_status or {}).get("remote_send_token") or "")
            self.assertTrue(token.startswith("frs_"))

            client = TestClient(create_app())
            bad = client.post(
                "/mcp/group-bridge",
                headers={"Authorization": f"Bearer {create_access_token('web', is_admin=True)['token']}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}},
            )
            self.assertEqual(bad.status_code, 401)
            query_token = client.post(
                f"/mcp/group-bridge?token={token}",
                json={"jsonrpc": "2.0", "id": 10, "method": "tools/list", "params": {}},
            )
            self.assertEqual(query_token.status_code, 401)
            path_token = client.post(
                f"/mcp/group-bridge/token/{token}",
                json={"jsonrpc": "2.0", "id": 11, "method": "tools/list", "params": {}},
            )
            self.assertEqual(path_token.status_code, 404)

            listed = client.post(
                "/mcp/group-bridge",
                headers={"Authorization": f"Bearer {token}"},
                json={"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
            )
            self.assertEqual(listed.status_code, 200, listed.text)
            self.assertEqual([item["name"] for item in listed.json()["result"]["tools"]], ["cccc_remote_access"])

            trust = list_trusts(group_id=group.group_id)[0]
            admin = create_access_token("admin", is_admin=True)["token"]
            updated = client.post(
                f"/api/federation/pairing/trusts/{trust['trust_id']}/access",
                headers={"Authorization": f"Bearer {admin}"},
                json={"access_level": "read", "updated_by": "user-a"},
            )
            self.assertEqual(updated.status_code, 200, updated.text)
            self.assertEqual(updated.json()["result"]["trust"]["access_level"], "read")

            read = client.post(
                "/mcp/group-bridge",
                headers={"Authorization": f"Bearer {token}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_remote_repo",
                        "arguments": {"remote_group_id": group.group_id, "action": "read", "path": "README.md"},
                    },
                },
            )
            self.assertEqual(read.status_code, 200, read.text)
            payload = self._tool_payload(read.json())
            self.assertEqual(payload["path"], "README.md")
            self.assertIn("project alpha", payload["content"])

            revoke_trust(trust["trust_id"], revoked_by="user-a")
            revoked = client.post(
                "/mcp/group-bridge",
                headers={"Authorization": f"Bearer {token}"},
                json={"jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": {}},
            )
            self.assertEqual(revoked.status_code, 401)
        finally:
            cleanup()

    def test_local_wrapper_strips_local_fields_and_requires_remote_group(self) -> None:
        from cccc.ports.mcp.common import MCPError
        from cccc.ports.mcp.handlers import group_bridge_client

        target = {
            "remote_group_id": "g_remote",
            "endpoint": "http://remote.example:8848",
            "_remote_send_token": "frs_test",
            "remote_mcp_available": True,
        }
        captured = {}

        def fake_call(remote_target, tool_name, arguments):
            captured["target"] = remote_target
            captured["tool_name"] = tool_name
            captured["arguments"] = dict(arguments)
            return {"ok": True}

        with patch.object(group_bridge_client, "_bridge_targets", return_value=[target]), patch.object(
            group_bridge_client,
            "_call_remote_tool",
            side_effect=fake_call,
        ):
            out = group_bridge_client.remote_repo(
                group_id="g_local",
                arguments={"group_id": "g_local", "remote_group_id": "g_remote", "action": "read", "path": "README.md"},
            )

        self.assertEqual(out, {"ok": True})
        self.assertEqual(captured["tool_name"], "cccc_remote_repo")
        self.assertEqual(captured["arguments"]["remote_group_id"], "g_remote")
        self.assertNotIn("group_id", captured["arguments"])

        with patch.object(group_bridge_client, "_bridge_targets", return_value=[target]):
            with self.assertRaises(MCPError) as raised:
                group_bridge_client.remote_repo(group_id="g_local", arguments={"action": "read"})
        self.assertEqual(raised.exception.code, "missing_remote_group_id")

    def test_local_wrapper_honors_remote_command_wait_bounds(self) -> None:
        from cccc.ports.mcp.handlers import group_bridge_client

        target = {
            "remote_group_id": "g_remote",
            "endpoint": "http://remote.example:8848",
            "_remote_send_token": "frs_test",
            "remote_mcp_available": True,
        }
        timeouts = []

        class FakeResponse:
            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def read(self):
                return json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": "cccc-group-bridge-call",
                        "result": {"content": [{"type": "text", "text": "{}"}]},
                    }
                ).encode("utf-8")

        def fake_urlopen(_request, *, timeout):
            timeouts.append(float(timeout))
            return FakeResponse()

        with patch.object(group_bridge_client, "_bridge_targets", return_value=[target]), patch.object(
            group_bridge_client.urllib.request,
            "urlopen",
            side_effect=fake_urlopen,
        ):
            group_bridge_client.remote_shell(
                group_id="g_local",
                arguments={"remote_group_id": "g_remote", "command": "sleep 10"},
            )
            group_bridge_client.remote_exec_command(
                group_id="g_local",
                arguments={"remote_group_id": "g_remote", "command": "cat", "yield_time_ms": 30000},
            )
            group_bridge_client.remote_write_stdin(
                group_id="g_local",
                arguments={"remote_group_id": "g_remote", "session_id": "sess_1", "yield_time_ms": 12000},
            )

        self.assertEqual(len(timeouts), 3)
        self.assertGreaterEqual(timeouts[0], 65.0)
        self.assertLessEqual(timeouts[0], 605.0)
        self.assertGreaterEqual(timeouts[1], 35.0)
        self.assertGreaterEqual(timeouts[2], 17.0)


if __name__ == "__main__":
    unittest.main()
