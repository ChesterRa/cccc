import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import yaml
from fastapi.testclient import TestClient


class TestWebRemoteMcpEndpoint(unittest.TestCase):
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

        return td, cleanup

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _create_group_with_actor(self, root: str | None = None):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import attach_scope_to_group, create_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope
        from cccc.daemon.runner_state_ops import write_headless_state

        reg = load_registry()
        group = create_group(reg, title="web-model-mcp", topic="")
        if root:
            group = attach_scope_to_group(reg, group, detect_scope(Path(root)), set_active=True)
        add_actor(group, actor_id="peer1", title="Web Model", runtime="web_model", runner="headless")
        write_headless_state(group.group_id, "peer1")
        return group

    def _create_group_with_web_model_peer(self):
        from cccc.daemon.runner_state_ops import write_headless_state
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title="web-model-mcp-peer", topic="")
        add_actor(group, actor_id="foreman1", title="Foreman", runtime="codex", runner="headless")
        add_actor(group, actor_id="peer1", title="Web Model", runtime="web_model", runner="headless")
        write_headless_state(group.group_id, "peer1")
        return group

    def _create_group_with_codex_actor(self):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title="web-model-mcp-invalid", topic="")
        add_actor(group, actor_id="peer1", title="Codex", runtime="codex", runner="headless")
        return group

    def _local_call_daemon(self, req: dict, **_kwargs):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        resp, _ = handle_request(DaemonRequest.model_validate(req))
        return resp.model_dump(exclude_none=True)

    def _create_connector(self, client: TestClient, admin: str, group, *, provider: str = "chatgpt_web") -> tuple[str, str]:
        create_resp = client.post(
            "/api/v1/web-model/connectors",
            headers={"Authorization": f"Bearer {admin}"},
            json={"group_id": group.group_id, "actor_id": "peer1", "provider": provider},
        )
        self.assertEqual(create_resp.status_code, 200)
        result = create_resp.json().get("result") or {}
        connector_id = str(((result.get("connector") or {}).get("connector_id")) or "")
        secret = str(result.get("secret") or "")
        self.assertTrue(connector_id)
        self.assertTrue(secret)
        return connector_id, secret

    def test_web_model_mcp_endpoint_rejects_non_connector_token(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            member = str(create_access_token("member", is_admin=False).get("token") or "")
            client = self._client()
            resp = client.post(
                "/mcp/web-model/test-connector",
                headers={"Authorization": f"Bearer {member}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            )
            self.assertEqual(resp.status_code, 401)
        finally:
            cleanup()

    def test_web_model_connector_endpoint_serves_actor_scoped_mcp(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"text": "work through connector", "to": ["peer1"]},
            )
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            create_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1", "provider": "chatgpt_web"},
            )
            self.assertEqual(create_resp.status_code, 200)
            result = create_resp.json().get("result") or {}
            connector = result.get("connector") or {}
            connector_id = str(connector.get("connector_id") or "")
            secret = str(result.get("secret") or "")
            self.assertTrue(connector_id.startswith("wmc_"))
            self.assertTrue(secret.startswith("wmcs_"))
            self.assertNotIn("secret_hash", connector)

            init_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            )
            self.assertEqual(init_resp.status_code, 200)
            self.assertEqual((init_resp.json().get("result") or {}).get("serverInfo", {}).get("name"), "cccc-mcp")

            list_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {"limit": 200}},
            )
            self.assertEqual(list_resp.status_code, 200)
            tools = ((list_resp.json().get("result") or {}).get("tools") or [])
            names = {str(item.get("name") or "") for item in tools if isinstance(item, dict)}
            self.assertIn("cccc_runtime_wait_next_turn", names)
            self.assertIn("cccc_runtime_complete_turn", names)
            self.assertIn("cccc_repo", names)
            self.assertIn("cccc_repo_edit", names)
            self.assertIn("cccc_apply_patch", names)
            self.assertIn("cccc_shell", names)
            self.assertIn("cccc_exec_command", names)
            self.assertIn("cccc_write_stdin", names)
            self.assertIn("cccc_git", names)
            self.assertIn("cccc_actor", names)
            self.assertIn("cccc_group", names)
            self.assertIn("cccc_capability_enable", names)
            self.assertIn("cccc_context_sync", names)
            self.assertIn("cccc_terminal", names)
            self.assertIn("cccc_debug", names)
            self.assertIn("cccc_space", names)
            self.assertNotIn("cccc_voice_secretary_document", names)
            self.assertNotIn("cccc_voice_secretary_request", names)
            self.assertNotIn("cccc_voice_secretary_composer", names)
            self.assertNotIn("cccc_pet_decisions", names)
            repo_spec = next((item for item in tools if isinstance(item, dict) and item.get("name") == "cccc_repo"), {})
            self.assertTrue(((repo_spec.get("annotations") or {}).get("readOnlyHint")))

            help_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 20,
                    "method": "tools/call",
                    "params": {"name": "cccc_help", "arguments": {}},
                },
            )
            self.assertEqual(help_resp.status_code, 200)
            help_text = (((help_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}"
            help_payload = json.loads(help_text)
            help_markdown = str(help_payload.get("markdown") or "")
            self.assertIn("## Web Model Transport (Runtime)", help_markdown)
            self.assertIn("normal CCCC agent", help_markdown)
            self.assertIn("remote MCP pull", help_markdown)

            with patch("cccc.ports.mcp.common.call_daemon", side_effect=self._local_call_daemon):
                wait_resp = client.post(
                    f"/mcp/web-model/{connector_id}",
                    headers={"Authorization": f"Bearer {secret}"},
                    json={
                        "jsonrpc": "2.0",
                        "id": 3,
                        "method": "tools/call",
                        "params": {"name": "cccc_runtime_wait_next_turn", "arguments": {}},
                    },
                )
            self.assertEqual(wait_resp.status_code, 200)
            content = (((wait_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}"
            payload = json.loads(content)
            turn = payload.get("turn") or {}
            self.assertEqual(payload.get("status"), "work_available")
            self.assertEqual(turn.get("group_id"), group.group_id)
            self.assertEqual(turn.get("actor_id"), "peer1")
            self.assertIn("work through connector", str(turn.get("coalesced_text") or ""))

            activity_resp = client.get(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
            )
            self.assertEqual(activity_resp.status_code, 200)
            activity_items = ((activity_resp.json().get("result") or {}).get("connectors") or [])
            activity = next(item for item in activity_items if str(item.get("connector_id") or "") == connector_id)
            self.assertEqual(activity.get("last_tool_name"), "cccc_runtime_wait_next_turn")
            self.assertEqual(activity.get("last_wait_status"), "work_available")
            self.assertEqual(activity.get("last_turn_id"), turn.get("turn_id"))
            self.assertTrue(str(activity.get("last_activity_at") or "").strip())
        finally:
            cleanup()

    def test_web_model_peer_sees_stable_schema_but_control_tools_are_role_gated(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_web_model_peer()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            connector_id, secret = self._create_connector(client, admin, group)

            list_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {"limit": 200}},
            )
            self.assertEqual(list_resp.status_code, 200)
            tools = ((list_resp.json().get("result") or {}).get("tools") or [])
            names = {str(item.get("name") or "") for item in tools if isinstance(item, dict)}
            self.assertIn("cccc_actor", names)
            self.assertIn("cccc_capability_enable", names)
            self.assertIn("cccc_shell", names)
            self.assertIn("cccc_exec_command", names)
            self.assertIn("cccc_write_stdin", names)
            self.assertNotIn("cccc_voice_secretary_document", names)

            actor_call = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {"name": "cccc_actor", "arguments": {"action": "list"}},
                },
            )
            self.assertEqual(actor_call.status_code, 200)
            self.assertTrue(bool((actor_call.json().get("result") or {}).get("isError")))
            self.assertIn("requires a Web Model foreman actor", actor_call.text)

            cap_call = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_capability_enable",
                        "arguments": {"capability_id": "pack:diagnostics", "scope": "group"},
                    },
                },
            )
            self.assertEqual(cap_call.status_code, 200)
            self.assertTrue(bool((cap_call.json().get("result") or {}).get("isError")))
            self.assertIn("permission_denied", cap_call.text)
        finally:
            cleanup()

    def test_web_model_foreman_can_call_advertised_control_tools(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            connector_id, secret = self._create_connector(client, admin, group)

            with patch("cccc.ports.mcp.common.call_daemon", side_effect=self._local_call_daemon):
                actor_call = client.post(
                    f"/mcp/web-model/{connector_id}",
                    headers={"Authorization": f"Bearer {secret}"},
                    json={
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "tools/call",
                        "params": {"name": "cccc_actor", "arguments": {"action": "list"}},
                    },
                )
            self.assertEqual(actor_call.status_code, 200)
            self.assertFalse(bool((actor_call.json().get("result") or {}).get("isError")))
            payload = json.loads(
                (((actor_call.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}"
            )
            self.assertEqual((payload.get("actors") or [{}])[0].get("id"), "peer1")

            with patch("cccc.ports.mcp.common.call_daemon", side_effect=self._local_call_daemon):
                cap_call = client.post(
                    f"/mcp/web-model/{connector_id}",
                    headers={"Authorization": f"Bearer {secret}"},
                    json={
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": "tools/call",
                        "params": {
                            "name": "cccc_capability_enable",
                            "arguments": {"capability_id": "pack:diagnostics", "scope": "group"},
                        },
                    },
                )
            self.assertEqual(cap_call.status_code, 200)
            self.assertFalse(bool((cap_call.json().get("result") or {}).get("isError")))
            self.assertIn("pack:diagnostics", cap_call.text)
        finally:
            cleanup()

    def test_web_model_connector_rejects_non_web_model_actor(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_codex_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            create_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1", "provider": "chatgpt_web"},
            )
            self.assertEqual(create_resp.status_code, 400)
            self.assertEqual((create_resp.json().get("error") or {}).get("code"), "invalid_actor_runtime")
        finally:
            cleanup()

    def test_web_model_connector_url_prefers_public_web_url(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from urllib.parse import parse_qs, urlparse

        _, cleanup = self._with_home()
        old_public_url = os.environ.get("CCCC_WEB_PUBLIC_URL")
        try:
            os.environ["CCCC_WEB_PUBLIC_URL"] = "https://cccc.example.test/ui/"
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            create_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1", "provider": "chatgpt_web"},
            )
            self.assertEqual(create_resp.status_code, 200)
            result = create_resp.json().get("result") or {}
            connector = (result.get("connector") or {})
            secret = str(result.get("secret") or "")
            self.assertEqual(
                connector.get("connector_url"),
                f"https://cccc.example.test/mcp/web-model/{connector.get('connector_id')}",
            )
            created_url = str(connector.get("connector_url_with_token") or "")
            created_parsed = urlparse(created_url)
            self.assertEqual(f"{created_parsed.scheme}://{created_parsed.netloc}{created_parsed.path}", connector.get("connector_url"))
            self.assertEqual(parse_qs(created_parsed.query).get("token"), [secret])
            self.assertNotIn("secret", connector)

            listing = client.get("/api/v1/web-model/connectors", headers={"Authorization": f"Bearer {admin}"})
            self.assertEqual(listing.status_code, 200)
            listed = ((listing.json().get("result") or {}).get("connectors") or [])[0]
            listed_url = str(listed.get("connector_url_with_token") or "")
            listed_parsed = urlparse(listed_url)
            self.assertEqual(f"{listed_parsed.scheme}://{listed_parsed.netloc}{listed_parsed.path}", listed.get("connector_url"))
            self.assertEqual(parse_qs(listed_parsed.query).get("token"), [secret])
            self.assertNotIn("secret", listed)
        finally:
            if old_public_url is None:
                os.environ.pop("CCCC_WEB_PUBLIC_URL", None)
            else:
                os.environ["CCCC_WEB_PUBLIC_URL"] = old_public_url
            cleanup()

    def test_web_model_connector_repo_tool_is_active_scope_bound(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        home, cleanup = self._with_home()
        try:
            workspace = Path(home) / "workspace"
            workspace.mkdir(parents=True, exist_ok=True)
            (workspace / "README.md").write_text("hello from workspace\n", encoding="utf-8")
            group = self._create_group_with_actor(str(workspace))
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            create_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1", "provider": "chatgpt_web"},
            )
            self.assertEqual(create_resp.status_code, 200)
            result = create_resp.json().get("result") or {}
            connector_id = str(((result.get("connector") or {}).get("connector_id")) or "")
            secret = str(result.get("secret") or "")

            read_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 10,
                    "method": "tools/call",
                    "params": {"name": "cccc_repo", "arguments": {"action": "read", "path": "README.md"}},
                },
            )
            self.assertEqual(read_resp.status_code, 200)
            payload = json.loads((((read_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertEqual(payload.get("path"), "README.md")
            self.assertIn("hello from workspace", str(payload.get("content") or ""))

            blocked = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 11,
                    "method": "tools/call",
                    "params": {"name": "cccc_repo", "arguments": {"action": "read", "path": "../outside.txt"}},
                },
            )
            self.assertEqual(blocked.status_code, 200)
            error_text = json.dumps(blocked.json(), ensure_ascii=False)
            self.assertIn("invalid_path", error_text)

            edit_blocked = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 12,
                    "method": "tools/call",
                    "params": {"name": "cccc_repo", "arguments": {"action": "write", "path": "x.txt", "content": "x"}},
                },
            )
            self.assertEqual(edit_blocked.status_code, 200)
            self.assertIn("cccc_repo_edit", json.dumps(edit_blocked.json(), ensure_ascii=False))
        finally:
            cleanup()

    def test_web_model_connector_local_power_tools_are_active_scope_bound(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        home, cleanup = self._with_home()
        try:
            workspace = Path(home) / "workspace"
            workspace.mkdir(parents=True, exist_ok=True)
            subprocess.run(["git", "init"], cwd=str(workspace), check=True, capture_output=True, text=True)
            subprocess.run(["git", "config", "user.email", "cccc@example.invalid"], cwd=str(workspace), check=True)
            subprocess.run(["git", "config", "user.name", "CCCC Test"], cwd=str(workspace), check=True)
            (workspace / "src").mkdir()
            (workspace / "src" / "app.txt").write_text("one\ntwo\nthree\nfour\n", encoding="utf-8")
            group = self._create_group_with_actor(str(workspace))
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            connector_id, secret = self._create_connector(client, admin, group)

            list_dir_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 190,
                    "method": "tools/call",
                    "params": {"name": "cccc_repo", "arguments": {"action": "list_dir", "path": ".", "depth": 2}},
                },
            )
            self.assertEqual(list_dir_resp.status_code, 200)
            list_dir_payload = json.loads((((list_dir_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertIn("src/app.txt", {str(item.get("path") or "") for item in list_dir_payload.get("entries") or [] if isinstance(item, dict)})

            range_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 191,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_repo",
                        "arguments": {"action": "read", "path": "src/app.txt", "start_line": 2, "end_line": 3},
                    },
                },
            )
            self.assertEqual(range_resp.status_code, 200)
            range_payload = json.loads((((range_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertEqual(range_payload.get("content"), "two\nthree\n")
            self.assertEqual(range_payload.get("start_line"), 2)
            self.assertEqual(range_payload.get("end_line"), 3)

            patch_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 192,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_apply_patch",
                        "arguments": {
                            "patch": "\n".join(
                                [
                                    "*** Begin Patch",
                                    "*** Update File: src/app.txt",
                                    "@@",
                                    " one",
                                    "-two",
                                    "+TWO",
                                    " three",
                                    "*** Add File: src/new.txt",
                                    "+created",
                                    "*** End Patch",
                                    "",
                                ]
                            )
                        },
                    },
                },
            )
            self.assertEqual(patch_resp.status_code, 200)
            patch_payload = json.loads((((patch_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertTrue(bool(patch_payload.get("applied")))
            self.assertEqual((workspace / "src" / "app.txt").read_text(encoding="utf-8"), "one\nTWO\nthree\nfour\n")
            self.assertEqual((workspace / "src" / "new.txt").read_text(encoding="utf-8"), "created\n")

            shell_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 20,
                    "method": "tools/call",
                    "params": {"name": "cccc_shell", "arguments": {"command": "printf shell-ok > shell.txt && cat shell.txt"}},
                },
            )
            self.assertEqual(shell_resp.status_code, 200)
            shell_payload = json.loads((((shell_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertTrue(bool(shell_payload.get("ok")))
            self.assertEqual(shell_payload.get("returncode"), 0)
            self.assertIn("shell-ok", str(shell_payload.get("stdout") or ""))
            self.assertEqual((workspace / "shell.txt").read_text(encoding="utf-8"), "shell-ok")

            exec_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 200,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_exec_command",
                        "arguments": {
                            "command": "printf exec-start; sleep 0.2; printf exec-done",
                            "yield_time_ms": 10,
                        },
                    },
                },
            )
            self.assertEqual(exec_resp.status_code, 200)
            exec_payload = json.loads((((exec_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertTrue(bool(exec_payload.get("running")))
            exec_session_id = str(exec_payload.get("session_id") or "")
            self.assertTrue(exec_session_id)

            exec_poll = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 2001,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_write_stdin",
                        "arguments": {"session_id": exec_session_id, "yield_time_ms": 300},
                    },
                },
            )
            self.assertEqual(exec_poll.status_code, 200)
            poll_payload = json.loads((((exec_poll.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertFalse(bool(poll_payload.get("running")))
            self.assertEqual(poll_payload.get("returncode"), 0)
            self.assertIn("exec-done", str(poll_payload.get("output") or ""))

            read_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 201,
                    "method": "tools/call",
                    "params": {"name": "cccc_repo", "arguments": {"action": "read", "path": "shell.txt"}},
                },
            )
            self.assertEqual(read_resp.status_code, 200)
            read_payload = json.loads((((read_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertEqual(read_payload.get("content"), "shell-ok")
            original_sha = str(read_payload.get("sha256") or "")
            self.assertTrue(original_sha)

            replace_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 202,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_repo_edit",
                        "arguments": {
                            "action": "replace",
                            "path": "shell.txt",
                            "old_text": "shell-ok",
                            "new_text": "shell-better",
                            "expected_sha256": original_sha,
                        },
                    },
                },
            )
            self.assertEqual(replace_resp.status_code, 200)
            replace_payload = json.loads((((replace_resp.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertTrue(bool(replace_payload.get("replaced")))
            self.assertNotEqual(str(replace_payload.get("sha256") or ""), original_sha)
            self.assertEqual((workspace / "shell.txt").read_text(encoding="utf-8"), "shell-better")

            stale_replace = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 203,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_repo_edit",
                        "arguments": {
                            "action": "replace",
                            "path": "shell.txt",
                            "old_text": "shell-better",
                            "new_text": "shell-final",
                            "expected_sha256": original_sha,
                        },
                    },
                },
            )
            self.assertEqual(stale_replace.status_code, 200)
            self.assertIn("stale_file", json.dumps(stale_replace.json(), ensure_ascii=False))
            self.assertEqual((workspace / "shell.txt").read_text(encoding="utf-8"), "shell-better")

            fresh_sha = str(replace_payload.get("sha256") or "")
            multi_replace = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 204,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_repo_edit",
                        "arguments": {
                            "action": "multi_replace",
                            "path": "shell.txt",
                            "expected_sha256": fresh_sha,
                            "replacements": [
                                {"old_text": "shell", "new_text": "mcp", "expected_replacements": 1},
                                {"old_text": "better", "new_text": "better-again", "expected_replacements": 1},
                            ],
                        },
                    },
                },
            )
            self.assertEqual(multi_replace.status_code, 200)
            multi_payload = json.loads((((multi_replace.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertEqual(multi_payload.get("replacements"), 2)
            self.assertEqual((workspace / "shell.txt").read_text(encoding="utf-8"), "mcp-better-again")

            mkdir_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 21,
                    "method": "tools/call",
                    "params": {"name": "cccc_repo_edit", "arguments": {"action": "mkdir", "path": "notes"}},
                },
            )
            self.assertEqual(mkdir_resp.status_code, 200)
            self.assertTrue((workspace / "notes").is_dir())

            move_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 22,
                    "method": "tools/call",
                    "params": {
                        "name": "cccc_repo_edit",
                        "arguments": {"action": "move", "path": "shell.txt", "dest_path": "notes/shell.txt"},
                    },
                },
            )
            self.assertEqual(move_resp.status_code, 200)
            self.assertTrue((workspace / "notes" / "shell.txt").exists())
            self.assertEqual((workspace / "notes" / "shell.txt").read_text(encoding="utf-8"), "mcp-better-again")

            git_status = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 23,
                    "method": "tools/call",
                    "params": {"name": "cccc_git", "arguments": {"action": "status"}},
                },
            )
            self.assertEqual(git_status.status_code, 200)
            status_payload = json.loads((((git_status.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertIn("notes/", str(status_payload.get("stdout") or ""))

            git_add = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 24,
                    "method": "tools/call",
                    "params": {"name": "cccc_git", "arguments": {"action": "add", "all_changes": True}},
                },
            )
            self.assertEqual(git_add.status_code, 200)
            git_add_payload = json.loads((((git_add.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}")
            self.assertTrue(bool(git_add_payload.get("ok")))
            self.assertEqual(git_add_payload.get("returncode"), 0)

            git_commit = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 25,
                    "method": "tools/call",
                    "params": {"name": "cccc_git", "arguments": {"action": "commit", "message": "Add shell output"}},
                },
            )
            self.assertEqual(git_commit.status_code, 200)
            self.assertEqual(json.loads((((git_commit.json().get("result") or {}).get("content") or [{}])[0] or {}).get("text") or "{}").get("returncode"), 0)

            delete_resp = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 26,
                    "method": "tools/call",
                    "params": {"name": "cccc_repo_edit", "arguments": {"action": "delete", "path": "notes", "recursive": True}},
                },
            )
            self.assertEqual(delete_resp.status_code, 200)
            self.assertFalse((workspace / "notes").exists())

            blocked = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={
                    "jsonrpc": "2.0",
                    "id": 27,
                    "method": "tools/call",
                    "params": {"name": "cccc_shell", "arguments": {"command": "pwd", "cwd": "../outside"}},
                },
            )
            self.assertEqual(blocked.status_code, 200)
            self.assertIn("invalid_path", json.dumps(blocked.json(), ensure_ascii=False))
        finally:
            cleanup()

    def test_web_model_connector_revoke_blocks_mcp_endpoint(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            create_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1"},
            )
            self.assertEqual(create_resp.status_code, 200)
            result = create_resp.json().get("result") or {}
            connector_id = str(((result.get("connector") or {}).get("connector_id")) or "")
            secret = str(result.get("secret") or "")

            revoke_resp = client.delete(
                f"/api/v1/web-model/connectors/{connector_id}",
                headers={"Authorization": f"Bearer {admin}"},
            )
            self.assertEqual(revoke_resp.status_code, 200)

            blocked = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            )
            self.assertEqual(blocked.status_code, 401)
        finally:
            cleanup()

    def test_web_model_connector_create_rotates_actor_active_connector(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            first_id, first_secret = self._create_connector(client, admin, group)

            second_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1", "provider": "chatgpt_web"},
            )
            self.assertEqual(second_resp.status_code, 200)
            second_result = second_resp.json().get("result") or {}
            second_id = str(((second_result.get("connector") or {}).get("connector_id")) or "")
            second_secret = str(second_result.get("secret") or "")
            self.assertNotEqual(first_id, second_id)
            self.assertIn(first_id, second_result.get("replaced_connector_ids") or [])

            listing = client.get("/api/v1/web-model/connectors", headers={"Authorization": f"Bearer {admin}"})
            self.assertEqual(listing.status_code, 200)
            items = (listing.json().get("result") or {}).get("connectors") or []
            first = next(item for item in items if str(item.get("connector_id") or "") == first_id)
            second = next(item for item in items if str(item.get("connector_id") or "") == second_id)
            self.assertTrue(bool(first.get("revoked")))
            self.assertFalse(bool(second.get("revoked")))

            old_blocked = client.post(
                f"/mcp/web-model/{first_id}",
                headers={"Authorization": f"Bearer {first_secret}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            )
            self.assertEqual(old_blocked.status_code, 401)

            current_ok = client.post(
                f"/mcp/web-model/{second_id}",
                headers={"Authorization": f"Bearer {second_secret}"},
                json={"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {}},
            )
            self.assertEqual(current_ok.status_code, 200)
        finally:
            cleanup()

    def test_web_model_connector_legacy_duplicate_active_entries_use_latest(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        home, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            first_id, first_secret = self._create_connector(client, admin, group)

            second_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1", "provider": "chatgpt_web"},
            )
            self.assertEqual(second_resp.status_code, 200)
            second_result = second_resp.json().get("result") or {}
            second_id = str(((second_result.get("connector") or {}).get("connector_id")) or "")

            path = Path(home) / "web_model_connectors.yaml"
            raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
            raw["connectors"][first_id]["revoked"] = False
            path.write_text(yaml.safe_dump(raw, allow_unicode=True, sort_keys=False), encoding="utf-8")

            listing = client.get("/api/v1/web-model/connectors", headers={"Authorization": f"Bearer {admin}"})
            self.assertEqual(listing.status_code, 200)
            items = (listing.json().get("result") or {}).get("connectors") or []
            first = next(item for item in items if str(item.get("connector_id") or "") == first_id)
            second = next(item for item in items if str(item.get("connector_id") or "") == second_id)
            self.assertTrue(bool(first.get("revoked")))
            self.assertFalse(bool(second.get("revoked")))

            old_blocked = client.post(
                f"/mcp/web-model/{first_id}",
                headers={"Authorization": f"Bearer {first_secret}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            )
            self.assertEqual(old_blocked.status_code, 401)
        finally:
            cleanup()

    def test_web_model_connector_revalidates_actor_lifecycle_before_serving_mcp(self) -> None:
        from cccc.daemon.runner_state_ops import remove_headless_state
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.actors import update_actor

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            connector_id, secret = self._create_connector(client, admin, group)

            update_actor(group, "peer1", {"enabled": False})
            remove_headless_state(group.group_id, "peer1")

            blocked = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            )
            self.assertEqual(blocked.status_code, 403)
            self.assertIn("connector_actor_stopped", blocked.text)
        finally:
            cleanup()

    def test_web_model_connector_revalidates_actor_runtime_before_serving_mcp(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.actors import update_actor

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            connector_id, secret = self._create_connector(client, admin, group)

            update_actor(group, "peer1", {"runtime": "codex"})

            blocked = client.post(
                f"/mcp/web-model/{connector_id}",
                headers={"Authorization": f"Bearer {secret}"},
                json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            )
            self.assertEqual(blocked.status_code, 403)
            self.assertIn("invalid_actor_runtime", blocked.text)
        finally:
            cleanup()

    def test_web_model_browser_session_routes_bind_to_web_model_actor(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            headers = {"Authorization": f"Bearer {admin}"}

            with (
                patch(
                    "cccc.ports.web_model_browser_sidecar.chatgpt_browser_session_status",
                    return_value={"active": False, "tab_url": "https://chatgpt.com/c/test-chat"},
                ),
                patch("cccc.ports.web_model_browser_sidecar.record_chatgpt_browser_state") as record_browser_state,
                patch("cccc.daemon.actors.web_model_browser_session.get_web_model_chatgpt_browser_session_state", return_value={"active": False, "state": "idle"}),
                patch(
                    "cccc.daemon.actors.web_model_browser_session.open_web_model_chatgpt_browser_session",
                    return_value={"active": True, "state": "ready", "metadata": {"cdp_port": 9222}},
                ) as open_session,
                patch(
                    "cccc.daemon.actors.web_model_browser_session.close_web_model_chatgpt_browser_session",
                    return_value={"closed": True, "browser_surface": {"active": False, "state": "idle"}},
                ) as close_session,
            ):
                status = client.get(
                    f"/api/v1/web-model/browser-session?group_id={group.group_id}&actor_id=peer1",
                    headers=headers,
                )
                self.assertEqual(status.status_code, 200)
                self.assertFalse(bool(((status.json().get("result") or {}).get("browser_session") or {}).get("active")))
                self.assertEqual(((status.json().get("result") or {}).get("browser_surface") or {}).get("state"), "idle")

                opened = client.post(
                    "/api/v1/web-model/browser-session/open",
                    headers=headers,
                    json={"group_id": group.group_id, "actor_id": "peer1", "width": 1440, "height": 900},
                )
                self.assertEqual(opened.status_code, 200)
                self.assertTrue(bool(((opened.json().get("result") or {}).get("browser_surface") or {}).get("active")))
                open_session.assert_called_once()
                self.assertEqual(open_session.call_args.kwargs.get("width"), 1440)

                closed = client.post(
                    "/api/v1/web-model/browser-session/close",
                    headers=headers,
                    json={"group_id": group.group_id, "actor_id": "peer1"},
                )
                self.assertEqual(closed.status_code, 200)
                close_session.assert_called_once()

                bound = client.post(
                    "/api/v1/web-model/browser-session/bind-current",
                    headers=headers,
                    json={"group_id": group.group_id, "actor_id": "peer1"},
                )
                self.assertEqual(bound.status_code, 200)
                record_browser_state.assert_called_with(
                    group.group_id,
                    "peer1",
                    {
                        "conversation_url": "https://chatgpt.com/c/test-chat",
                        "pending_new_chat_bind": False,
                        "pending_new_chat_url": "",
                        "pending_new_chat_bind_started_at": "",
                        "pending_new_chat_submitted": False,
                        "pending_new_chat_submitted_at": "",
                        "pending_new_chat_delivery_id": "",
                        "pending_new_chat_last_turn_id": "",
                        "pending_new_chat_last_event_ids": [],
                        "pending_new_chat_last_tab_url": "",
                        "new_chat_bound_at": "",
                        "bootstrap_seed_delivered_at": "",
                        "bootstrap_seed_version": "",
                        "bootstrap_seed_digest": "",
                        "bootstrap_seed_conversation_url": "",
                        "last_error": "",
                    },
                )
        finally:
            cleanup()

    def test_web_model_browser_session_can_arm_new_chat_auto_bind(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()

            with (
                patch(
                    "cccc.ports.web_model_browser_sidecar.chatgpt_browser_session_status",
                    return_value={"active": True, "tab_url": "https://chatgpt.com/"},
                ),
                patch("cccc.ports.web_model_browser_sidecar.record_chatgpt_browser_state") as record_browser_state,
                patch("cccc.daemon.actors.web_model_browser_session.get_web_model_chatgpt_browser_session_state", return_value={"active": True, "state": "ready"}),
            ):
                resp = client.post(
                    "/api/v1/web-model/browser-session/bind-current",
                    headers={"Authorization": f"Bearer {admin}"},
                    json={"group_id": group.group_id, "actor_id": "peer1"},
                )

            self.assertEqual(resp.status_code, 200)
            call = record_browser_state.call_args.args
            self.assertEqual(call[0], group.group_id)
            self.assertEqual(call[1], "peer1")
            state = call[2]
            self.assertEqual(state.get("conversation_url"), "")
            self.assertEqual(state.get("pending_new_chat_bind"), True)
            self.assertEqual(state.get("pending_new_chat_url"), "https://chatgpt.com/")
            self.assertEqual(state.get("pending_new_chat_submitted"), False)
            self.assertEqual(state.get("pending_new_chat_delivery_id"), "")
            self.assertTrue(str(state.get("pending_new_chat_bind_started_at") or ""))
        finally:
            cleanup()

    def test_web_model_browser_session_rejects_non_web_model_actor(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_codex_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            resp = client.post(
                "/api/v1/web-model/browser-session/open",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1"},
            )
            self.assertEqual(resp.status_code, 400)
            self.assertIn("invalid_actor_runtime", resp.text)
        finally:
            cleanup()

    def test_web_model_connector_supports_streamable_http_probe_and_options(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group = self._create_group_with_actor()
            admin = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._client()
            create_resp = client.post(
                "/api/v1/web-model/connectors",
                headers={"Authorization": f"Bearer {admin}"},
                json={"group_id": group.group_id, "actor_id": "peer1"},
            )
            self.assertEqual(create_resp.status_code, 200)
            result = create_resp.json().get("result") or {}
            connector_id = str(((result.get("connector") or {}).get("connector_id")) or "")
            secret = str(result.get("secret") or "")

            options = client.options(f"/mcp/web-model/{connector_id}")
            self.assertEqual(options.status_code, 204)
            self.assertIn("POST", str(options.headers.get("allow") or ""))

            unauthenticated = client.get(f"/mcp/web-model/{connector_id}")
            self.assertEqual(unauthenticated.status_code, 401)

            probe = client.get(f"/mcp/web-model/{connector_id}?token={secret}")
            self.assertEqual(probe.status_code, 200)
            self.assertIn("text/event-stream", str(probe.headers.get("content-type") or ""))
            self.assertIn("connector ready", probe.text)
        finally:
            cleanup()
