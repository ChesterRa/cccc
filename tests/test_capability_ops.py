from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestCapabilityOps(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _write_allowlist_override(self, *, mcp_registry_level: str = "mounted") -> Path:
        home = Path(str(os.environ.get("CCCC_HOME") or "")).expanduser()
        cfg_dir = home / "config"
        cfg_dir.mkdir(parents=True, exist_ok=True)
        path = cfg_dir / "capability-allowlist.yaml"
        path.write_text(
            (
                "defaults:\n"
                "  source_level:\n"
                f"    mcp_registry_official: {mcp_registry_level}\n"
                "    anthropic_skills: mounted\n"
                "    cccc_builtin: enabled\n"
            ),
            encoding="utf-8",
        )
        return path

    def _create_group(self, title: str = "capability-test") -> str:
        create_resp, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
        gid = str((create_resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _add_actor(self, group_id: str, actor_id: str, *, by: str = "user") -> None:
        add_resp, _ = self._call(
            "actor_add",
            {
                "group_id": group_id,
                "actor_id": actor_id,
                "runtime": "codex",
                "runner": "headless",
                "by": by,
            },
        )
        self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

    def test_capability_state_defaults_to_core_surface(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            state_resp, _ = self._call("capability_state", {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"})
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            result = state_resp.result if isinstance(state_resp.result, dict) else {}
            visible = result.get("visible_tools") if isinstance(result.get("visible_tools"), list) else []
            self.assertIn("cccc_help", visible)
            self.assertIn("cccc_capability_search", visible)
            self.assertNotIn("cccc_space_status", visible)
        finally:
            cleanup()

    def test_enable_session_pack_updates_visible_tools(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            enable_resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "pack:space",
                    "scope": "session",
                    "ttl_seconds": 600,
                    "enabled": True,
                },
            )
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))
            enable_result = enable_resp.result if isinstance(enable_resp.result, dict) else {}
            self.assertTrue(bool(enable_result.get("refresh_required")))

            state_resp, _ = self._call("capability_state", {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"})
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            result = state_resp.result if isinstance(state_resp.result, dict) else {}
            visible = result.get("visible_tools") if isinstance(result.get("visible_tools"), list) else []
            self.assertIn("cccc_space_status", visible)
            self.assertIn("pack:space", result.get("enabled_capabilities") or [])
        finally:
            cleanup()

    def test_non_foreman_cannot_enable_group_scope(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            self._add_actor(gid, "peer-2", by="user")
            resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-2",
                    "actor_id": "peer-2",
                    "capability_id": "pack:space",
                    "scope": "group",
                    "enabled": True,
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual((resp.error.code if resp.error else ""), "permission_denied")
        finally:
            cleanup()

    def test_search_returns_builtin_records_without_network(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            with patch("cccc.daemon.ops.capability_ops._auto_sync_catalog", return_value=False):
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "space",
                        "include_external": False,
                        "limit": 20,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
            self.assertIn("pack:space", ids)
        finally:
            cleanup()

    def test_search_without_external_skips_auto_sync(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            with patch("cccc.daemon.ops.capability_ops._auto_sync_catalog", return_value=False) as auto_sync:
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "",
                        "include_external": False,
                        "limit": 10,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            auto_sync.assert_not_called()
        finally:
            cleanup()

    def test_search_with_external_does_not_auto_sync_by_default(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            with patch("cccc.daemon.ops.capability_ops._auto_sync_catalog", return_value=False) as auto_sync:
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "",
                        "include_external": True,
                        "limit": 10,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            auto_sync.assert_not_called()
        finally:
            cleanup()

    def test_search_with_external_can_auto_sync_when_enabled(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            with patch.dict(os.environ, {"CCCC_CAPABILITY_SEARCH_AUTO_SYNC": "1"}, clear=False), patch(
                "cccc.daemon.ops.capability_ops._auto_sync_catalog",
                return_value=False,
            ) as auto_sync:
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "",
                        "include_external": True,
                        "limit": 10,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            auto_sync.assert_called_once()
        finally:
            cleanup()

    def test_sync_anthropic_skills_accepts_github_list_payload(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        class _Resp:
            def __init__(self, text: str) -> None:
                self._body = text.encode("utf-8")

            def read(self) -> bytes:
                return self._body

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb) -> bool:
                return False

        catalog = ops._new_catalog_doc()
        skill_md = (
            "---\n"
            "name: example-skill\n"
            "description: Example skill\n"
            "license: MIT\n"
            "---\n"
            "body\n"
        )
        with patch(
            "cccc.daemon.ops.capability_ops._http_get_json",
            return_value=[{"type": "dir", "name": "example-skill", "sha": "abc123"}],
        ), patch("cccc.daemon.ops.capability_ops.urlopen", return_value=_Resp(skill_md)):
            upserted = ops._sync_anthropic_skills_source(catalog, force=True)

        self.assertEqual(upserted, 1)
        record = catalog.get("records", {}).get("skill:anthropic:example-skill")
        self.assertIsInstance(record, dict)
        self.assertEqual(record.get("name"), "example-skill")
        source_state = catalog.get("sources", {}).get("anthropic_skills", {})
        self.assertEqual(source_state.get("sync_state"), "fresh")

    def test_auto_sync_respects_source_enable_flags(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        catalog = ops._new_catalog_doc()
        with patch.dict(
            os.environ,
            {
                "CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED": "1",
                "CCCC_CAPABILITY_SOURCE_ANTHROPIC_SKILLS_ENABLED": "0",
                "CCCC_CAPABILITY_SOURCE_AGENTSKILLS_VALIDATOR_ENABLED": "0",
            },
            clear=False,
        ), patch("cccc.daemon.ops.capability_ops._sync_mcp_registry_source", return_value=0), patch(
            "cccc.daemon.ops.capability_ops._sync_anthropic_skills_source",
            side_effect=AssertionError("anthropic source should be disabled"),
        ), patch(
            "cccc.daemon.ops.capability_ops._mark_agentskills_validator_state",
            side_effect=AssertionError("agentskills validator should be disabled"),
        ):
            changed = ops._auto_sync_catalog(catalog)

        self.assertTrue(changed)
        sources = catalog.get("sources", {})
        anthropic = sources.get("anthropic_skills", {})
        validator = sources.get("agentskills_validator", {})
        self.assertEqual(anthropic.get("sync_state"), "disabled")
        self.assertEqual(validator.get("sync_state"), "disabled")
        self.assertEqual(anthropic.get("error"), "source_disabled_by_policy")
        self.assertEqual(validator.get("error"), "source_disabled_by_policy")

    def test_sync_capability_catalog_once_saves_when_changed(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        fake_path = Path("/tmp/fake-capability-catalog.json")
        fake_doc = ops._new_catalog_doc()
        with patch(
            "cccc.daemon.ops.capability_ops._load_catalog_doc",
            return_value=(fake_path, fake_doc),
        ), patch(
            "cccc.daemon.ops.capability_ops._sync_catalog",
            return_value={"changed": True, "upserted_total": 2, "upserted": {"mcp_registry_official": 2}},
        ), patch(
            "cccc.daemon.ops.capability_ops._save_catalog_doc",
        ) as save_doc:
            result = ops.sync_capability_catalog_once(force=True)

        self.assertTrue(result.get("ok"))
        self.assertTrue(result.get("changed"))
        self.assertEqual(int(result.get("upserted_total") or 0), 2)
        save_doc.assert_called_once_with(fake_path, fake_doc)

    def test_external_enable_requires_approve_when_manual_review(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            catalog = ops._new_catalog_doc()
            catalog["records"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "kind": "mcp_toolpack",
                "name": "test-server",
                "qualification_status": "manual_review",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9900/mcp"},
            }
            with patch("cccc.daemon.ops.capability_ops._load_catalog_doc", return_value=(Path("/tmp/cat.json"), catalog)):
                resp, _ = self._call(
                    "capability_enable",
                    {
                        "group_id": gid,
                        "by": "peer-1",
                        "actor_id": "peer-1",
                        "capability_id": "mcp:test-server",
                        "scope": "session",
                        "enabled": True,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "pending_approval")
            self.assertFalse(bool(result.get("enabled")))
        finally:
            cleanup()

    def test_external_enable_with_approve_installs_and_exposes_dynamic_tools(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            catalog = ops._new_catalog_doc()
            catalog["records"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "kind": "mcp_toolpack",
                "name": "test-server",
                "qualification_status": "manual_review",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9900/mcp"},
            }
            installed = {
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [
                    {
                        "name": "cccc_ext_deadbeef_echo",
                        "real_tool_name": "echo",
                        "description": "echo tool",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    }
                ],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            with patch("cccc.daemon.ops.capability_ops._load_catalog_doc", return_value=(Path("/tmp/cat.json"), catalog)), patch(
                "cccc.daemon.ops.capability_ops._install_external_capability",
                return_value=installed,
            ):
                enable_resp, _ = self._call(
                    "capability_enable",
                    {
                        "group_id": gid,
                        "by": "user",
                        "actor_id": "peer-1",
                        "capability_id": "mcp:test-server",
                        "scope": "session",
                        "enabled": True,
                        "approve": True,
                    },
                )
                self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))
                state_resp, _ = self._call(
                    "capability_state",
                    {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
                )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            visible = state.get("visible_tools") if isinstance(state.get("visible_tools"), list) else []
            self.assertIn("cccc_ext_deadbeef_echo", visible)
            dynamic = state.get("dynamic_tools") if isinstance(state.get("dynamic_tools"), list) else []
            names = {str(x.get("name") or "") for x in dynamic if isinstance(x, dict)}
            self.assertIn("cccc_ext_deadbeef_echo", names)
        finally:
            cleanup()

    def test_capability_tool_call_invokes_enabled_dynamic_tool(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            # Seed enabled state directly for deterministic test.
            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            runtime_doc["installations"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [
                    {
                        "name": "cccc_ext_deadbeef_echo",
                        "real_tool_name": "echo",
                        "description": "echo tool",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    }
                ],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            ops._save_runtime_doc(runtime_path, runtime_doc)

            with patch(
                "cccc.daemon.ops.capability_ops._invoke_installed_external_tool",
                return_value={"content": [{"type": "text", "text": "ok"}]},
            ):
                resp, _ = self._call(
                    "capability_tool_call",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "tool_name": "cccc_ext_deadbeef_echo",
                        "arguments": {"message": "hello"},
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("capability_id") or ""), "mcp:test-server")
            call_result = result.get("result") if isinstance(result.get("result"), dict) else {}
            self.assertIn("content", call_result)
        finally:
            cleanup()

    def test_disable_external_capability_hides_dynamic_tools(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "kind": "mcp_toolpack",
                "name": "test-server",
                "qualification_status": "qualified",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9900/mcp"},
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            runtime_doc["installations"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [
                    {
                        "name": "cccc_ext_deadbeef_echo",
                        "real_tool_name": "echo",
                        "description": "echo tool",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    }
                ],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            ops._save_runtime_doc(runtime_path, runtime_doc)

            before_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(before_resp.ok, getattr(before_resp, "error", None))
            before_state = before_resp.result if isinstance(before_resp.result, dict) else {}
            before_visible = before_state.get("visible_tools") if isinstance(before_state.get("visible_tools"), list) else []
            self.assertIn("cccc_ext_deadbeef_echo", before_visible)

            disable_resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "mcp:test-server",
                    "scope": "session",
                    "enabled": False,
                },
            )
            self.assertTrue(disable_resp.ok, getattr(disable_resp, "error", None))

            after_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(after_resp.ok, getattr(after_resp, "error", None))
            after_state = after_resp.result if isinstance(after_resp.result, dict) else {}
            after_visible = after_state.get("visible_tools") if isinstance(after_state.get("visible_tools"), list) else []
            self.assertNotIn("cccc_ext_deadbeef_echo", after_visible)
        finally:
            cleanup()

    def test_external_install_failure_persists_runtime_failure_state(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "kind": "mcp_toolpack",
                "name": "test-server",
                "qualification_status": "qualified",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9900/mcp"},
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            with patch(
                "cccc.daemon.ops.capability_ops._install_external_capability",
                side_effect=RuntimeError("probe_failed"),
            ):
                resp, _ = self._call(
                    "capability_enable",
                    {
                        "group_id": gid,
                        "by": "peer-1",
                        "actor_id": "peer-1",
                        "capability_id": "mcp:test-server",
                        "scope": "session",
                        "enabled": True,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "failed")
            self.assertIn("install_failed", str(result.get("reason") or ""))

            _, runtime_doc = ops._load_runtime_doc()
            installs = runtime_doc.get("installations") if isinstance(runtime_doc.get("installations"), dict) else {}
            entry = installs.get("mcp:test-server") if isinstance(installs, dict) else None
            self.assertIsInstance(entry, dict)
            self.assertEqual(str(entry.get("state") or ""), "install_failed")
            self.assertIn("probe_failed", str(entry.get("last_error") or ""))
        finally:
            cleanup()

    def test_capability_tool_call_rejects_tool_when_capability_not_enabled(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            runtime_path, runtime_doc = ops._load_runtime_doc()
            runtime_doc["installations"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [
                    {
                        "name": "cccc_ext_deadbeef_echo",
                        "real_tool_name": "echo",
                        "description": "echo tool",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    }
                ],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            ops._save_runtime_doc(runtime_path, runtime_doc)

            resp, _ = self._call(
                "capability_tool_call",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "tool_name": "cccc_ext_deadbeef_echo",
                    "arguments": {"message": "hello"},
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual((resp.error.code if resp.error else ""), "capability_tool_not_found")
        finally:
            cleanup()

    def test_capability_enable_emits_action_id_and_audit_event(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "pack:space",
                    "scope": "session",
                    "enabled": True,
                    "reason": "need space tools for research",
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            action_id = str(result.get("action_id") or "")
            self.assertTrue(action_id.startswith("cact_"), msg=f"missing action_id: {action_id}")

            audit_path = ops._audit_path()
            self.assertTrue(audit_path.exists())
            lines = [line.strip() for line in audit_path.read_text(encoding="utf-8").splitlines() if line.strip()]
            self.assertTrue(lines)
            last = json.loads(lines[-1])
            self.assertEqual(str(last.get("action_id") or ""), action_id)
            self.assertEqual(str(last.get("op") or ""), "capability_enable")
            details = last.get("details") if isinstance(last.get("details"), dict) else {}
            self.assertIn("need space tools", str(details.get("reason") or ""))
        finally:
            cleanup()

    def test_capability_search_supports_source_trust_and_qualification_filters(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["mcp:alpha"] = {
                "capability_id": "mcp:alpha",
                "kind": "mcp_toolpack",
                "name": "alpha",
                "description_short": "alpha",
                "source_id": "mcp_registry_official",
                "source_tier": "official",
                "trust_tier": "official",
                "qualification_status": "qualified",
                "enable_supported": True,
                "sync_state": "fresh",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9900/mcp"},
            }
            catalog_doc["records"]["mcp:beta"] = {
                "capability_id": "mcp:beta",
                "kind": "mcp_toolpack",
                "name": "beta",
                "description_short": "beta",
                "source_id": "anthropic_skills",
                "source_tier": "tier1",
                "trust_tier": "community",
                "qualification_status": "manual_review",
                "enable_supported": True,
                "sync_state": "fresh",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9901/mcp"},
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "include_external": True,
                    "source_id": "anthropic_skills",
                    "trust_tier": "community",
                    "qualification_status": "manual_review",
                    "limit": 20,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
            self.assertIn("mcp:beta", ids)
            self.assertNotIn("mcp:alpha", ids)
        finally:
            cleanup()

    def test_search_hides_indexed_capability_by_default_policy(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["mcp:indexed-default"] = {
                "capability_id": "mcp:indexed-default",
                "kind": "mcp_toolpack",
                "name": "indexed-default",
                "description_short": "indexed-default",
                "source_id": "mcp_registry_official",
                "source_tier": "official",
                "trust_tier": "official",
                "qualification_status": "qualified",
                "enable_supported": True,
                "sync_state": "fresh",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9910/mcp"},
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "query": "indexed-default",
                    "include_external": True,
                    "limit": 20,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
            self.assertNotIn("mcp:indexed-default", ids)
            diagnostics = result.get("search_diagnostics") if isinstance(result.get("search_diagnostics"), dict) else {}
            self.assertGreaterEqual(int(diagnostics.get("policy_hidden_count") or 0), 1)
        finally:
            cleanup()

    def test_enable_rejects_indexed_policy_level(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["mcp:indexed-default"] = {
                "capability_id": "mcp:indexed-default",
                "kind": "mcp_toolpack",
                "name": "indexed-default",
                "description_short": "indexed-default",
                "source_id": "mcp_registry_official",
                "source_tier": "official",
                "trust_tier": "official",
                "qualification_status": "qualified",
                "enable_supported": True,
                "sync_state": "fresh",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9911/mcp"},
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "user",
                    "scope": "session",
                    "capability_id": "mcp:indexed-default",
                    "enabled": True,
                    "approve": True,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "failed")
            self.assertEqual(str(result.get("reason") or ""), "policy_level_indexed")
            self.assertEqual(str(result.get("policy_level") or ""), "indexed")
        finally:
            cleanup()

    def test_capability_state_reports_scope_mismatch_and_manual_review_hidden_reasons(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            self._write_allowlist_override(mcp_registry_level="mounted")
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            self._add_actor(gid, "peer-2", by="user")

            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["mcp:shared"] = {
                "capability_id": "mcp:shared",
                "kind": "mcp_toolpack",
                "name": "shared",
                "description_short": "shared",
                "source_id": "mcp_registry_official",
                "source_tier": "official",
                "trust_tier": "official",
                "qualification_status": "qualified",
                "enable_supported": True,
                "sync_state": "fresh",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9902/mcp"},
            }
            catalog_doc["records"]["mcp:manual"] = {
                "capability_id": "mcp:manual",
                "kind": "mcp_toolpack",
                "name": "manual",
                "description_short": "manual",
                "source_id": "anthropic_skills",
                "source_tier": "tier1",
                "trust_tier": "community",
                "qualification_status": "manual_review",
                "enable_supported": True,
                "sync_state": "fresh",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9903/mcp"},
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="peer-2",
                scope="actor",
                capability_id="mcp:shared",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            hidden = result.get("hidden_capabilities") if isinstance(result.get("hidden_capabilities"), list) else []
            by_id = {
                str(item.get("capability_id") or ""): str(item.get("reason") or "")
                for item in hidden
                if isinstance(item, dict)
            }
            self.assertEqual(by_id.get("mcp:manual"), "manual_review_required")
            self.assertEqual(by_id.get("mcp:shared"), "scope_mismatch")
        finally:
            cleanup()

    def test_capability_enable_respects_actor_quota(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            with patch.dict(os.environ, {"CCCC_CAPABILITY_MAX_ENABLED_PER_ACTOR": "1"}, clear=False):
                first, _ = self._call(
                    "capability_enable",
                    {
                        "group_id": gid,
                        "by": "peer-1",
                        "actor_id": "peer-1",
                        "capability_id": "pack:space",
                        "scope": "session",
                        "enabled": True,
                    },
                )
                self.assertTrue(first.ok, getattr(first, "error", None))
                first_result = first.result if isinstance(first.result, dict) else {}
                self.assertEqual(str(first_result.get("state") or ""), "ready")

                second, _ = self._call(
                    "capability_enable",
                    {
                        "group_id": gid,
                        "by": "peer-1",
                        "actor_id": "peer-1",
                        "capability_id": "pack:groups",
                        "scope": "session",
                        "enabled": True,
                    },
                )
            self.assertTrue(second.ok, getattr(second, "error", None))
            second_result = second.result if isinstance(second.result, dict) else {}
            self.assertEqual(str(second_result.get("state") or ""), "failed")
            self.assertIn("quota_enabled_actor_exceeded", str(second_result.get("reason") or ""))
        finally:
            cleanup()

    def test_capability_uninstall_revokes_binding_and_removes_installation(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "foreman-1", by="user")
            self._call(
                "actor_update",
                {
                    "group_id": gid,
                    "actor_id": "foreman-1",
                    "by": "user",
                    "patch": {"role": "foreman"},
                },
            )

            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="foreman-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            runtime_doc["installations"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            ops._save_runtime_doc(runtime_path, runtime_doc)

            uninstall_resp, _ = self._call(
                "capability_uninstall",
                {
                    "group_id": gid,
                    "by": "foreman-1",
                    "actor_id": "foreman-1",
                    "capability_id": "mcp:test-server",
                    "reason": "cleanup",
                },
            )
            self.assertTrue(uninstall_resp.ok, getattr(uninstall_resp, "error", None))
            uninstall_result = uninstall_resp.result if isinstance(uninstall_resp.result, dict) else {}
            self.assertTrue(bool(uninstall_result.get("removed_installation")))
            self.assertGreaterEqual(int(uninstall_result.get("removed_bindings") or 0), 1)

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "foreman-1", "by": "foreman-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state_result = state_resp.result if isinstance(state_resp.result, dict) else {}
            self.assertNotIn("mcp:test-server", state_result.get("enabled_capabilities") or [])

            _, runtime_doc_after = ops._load_runtime_doc()
            installs = runtime_doc_after.get("installations") if isinstance(runtime_doc_after.get("installations"), dict) else {}
            self.assertNotIn("mcp:test-server", installs)
        finally:
            cleanup()

    def test_capability_state_dynamic_tools_respects_visibility_limit(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            runtime_doc["installations"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [
                    {
                        "name": "cccc_ext_deadbeef_a",
                        "real_tool_name": "a",
                        "description": "a",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    },
                    {
                        "name": "cccc_ext_deadbeef_b",
                        "real_tool_name": "b",
                        "description": "b",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    },
                    {
                        "name": "cccc_ext_deadbeef_c",
                        "real_tool_name": "c",
                        "description": "c",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    },
                ],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            ops._save_runtime_doc(runtime_path, runtime_doc)

            with patch.dict(os.environ, {"CCCC_CAPABILITY_MAX_DYNAMIC_TOOLS_VISIBLE": "2"}, clear=False):
                resp, _ = self._call(
                    "capability_state",
                    {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            dynamic = result.get("dynamic_tools") if isinstance(result.get("dynamic_tools"), list) else []
            self.assertEqual(len(dynamic), 2)
            self.assertEqual(int(result.get("dynamic_tool_limit") or 0), 2)
            self.assertEqual(int(result.get("dynamic_tool_dropped") or 0), 1)
        finally:
            cleanup()

    def test_skill_enable_session_reports_active_skill_and_applies_dependencies(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["skill:anthropic:write-pr"] = {
                "capability_id": "skill:anthropic:write-pr",
                "kind": "skill",
                "name": "write-pr",
                "description_short": "Write concise PR summaries",
                "source_id": "anthropic_skills",
                "source_tier": "tier1",
                "source_uri": "https://example.invalid/skills/write-pr",
                "trust_tier": "tier1",
                "qualification_status": "qualified",
                "enable_supported": True,
                "capsule_text": "Use structured PR summary format.",
                "requires_capabilities": ["pack:space"],
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            enable_resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "skill:anthropic:write-pr",
                    "scope": "session",
                    "enabled": True,
                },
            )
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))
            enable_result = enable_resp.result if isinstance(enable_resp.result, dict) else {}
            skill_payload = enable_result.get("skill") if isinstance(enable_result.get("skill"), dict) else {}
            self.assertEqual(str(skill_payload.get("capability_id") or ""), "skill:anthropic:write-pr")
            applied = skill_payload.get("applied_dependencies") if isinstance(skill_payload.get("applied_dependencies"), list) else []
            self.assertIn("pack:space", applied)

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            enabled = state.get("enabled_capabilities") if isinstance(state.get("enabled_capabilities"), list) else []
            self.assertIn("skill:anthropic:write-pr", enabled)
            self.assertIn("pack:space", enabled)
            active_skills = state.get("active_skills") if isinstance(state.get("active_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_skills if isinstance(item, dict)}
            self.assertIn("skill:anthropic:write-pr", active_ids)
            pinned_skills = state.get("pinned_skills") if isinstance(state.get("pinned_skills"), list) else []
            pinned_ids = {str(item.get("capability_id") or "") for item in pinned_skills if isinstance(item, dict)}
            self.assertNotIn("skill:anthropic:write-pr", pinned_ids)
            binding_states = (
                state.get("external_binding_states") if isinstance(state.get("external_binding_states"), dict) else {}
            )
            skill_binding = binding_states.get("skill:anthropic:write-pr") if isinstance(binding_states, dict) else {}
            self.assertEqual(str((skill_binding or {}).get("mode") or ""), "skill")
        finally:
            cleanup()

    def test_skill_actor_scope_is_reported_as_pinned(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["skill:anthropic:triage"] = {
                "capability_id": "skill:anthropic:triage",
                "kind": "skill",
                "name": "triage",
                "description_short": "Issue triage checklist",
                "source_id": "anthropic_skills",
                "source_tier": "tier1",
                "trust_tier": "tier1",
                "qualification_status": "qualified",
                "enable_supported": True,
                "capsule_text": "Use strict triage checklist.",
                "requires_capabilities": [],
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            enable_resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "skill:anthropic:triage",
                    "scope": "actor",
                    "enabled": True,
                },
            )
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            pinned_skills = state.get("pinned_skills") if isinstance(state.get("pinned_skills"), list) else []
            pinned_ids = {str(item.get("capability_id") or "") for item in pinned_skills if isinstance(item, dict)}
            self.assertIn("skill:anthropic:triage", pinned_ids)
        finally:
            cleanup()

    def test_disable_with_cleanup_removes_cached_installation(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            runtime_doc["installations"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            ops._save_runtime_doc(runtime_path, runtime_doc)

            resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "mcp:test-server",
                    "scope": "session",
                    "enabled": False,
                    "cleanup": True,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertTrue(bool(result.get("removed_installation")))

            _, runtime_after = ops._load_runtime_doc()
            installs = runtime_after.get("installations") if isinstance(runtime_after.get("installations"), dict) else {}
            self.assertNotIn("mcp:test-server", installs)
        finally:
            cleanup()

    def test_disable_with_cleanup_skips_when_capability_still_bound_elsewhere(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            self._add_actor(gid, "peer-2", by="user")

            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._set_enabled_capability(
                state_doc,
                group_id=gid,
                actor_id="peer-2",
                scope="actor",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            runtime_doc["installations"]["mcp:test-server"] = {
                "capability_id": "mcp:test-server",
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            ops._save_runtime_doc(runtime_path, runtime_doc)

            resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "mcp:test-server",
                    "scope": "session",
                    "enabled": False,
                    "cleanup": True,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertFalse(bool(result.get("removed_installation")))
            self.assertEqual(
                str(result.get("cleanup_skipped_reason") or ""),
                "cleanup_skipped_capability_still_bound",
            )

            _, runtime_after = ops._load_runtime_doc()
            installs = runtime_after.get("installations") if isinstance(runtime_after.get("installations"), dict) else {}
            self.assertIn("mcp:test-server", installs)
        finally:
            cleanup()

    def test_catalog_prune_respects_configured_limit(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        catalog = ops._new_catalog_doc()
        for i in range(205):
            catalog["records"][f"mcp:test-{i}"] = {
                "capability_id": f"mcp:test-{i}",
                "kind": "mcp_toolpack",
                "name": f"test-{i}",
                "source_id": "mcp_registry_official",
                "source_tier": "tier1",
                "trust_tier": "tier1",
                "qualification_status": "qualified",
                "updated_at_source": f"2026-02-25T00:00:0{i}Z",
                "last_synced_at": f"2026-02-25T00:00:0{i}Z",
            }

        with patch.dict(os.environ, {"CCCC_CAPABILITY_CATALOG_MAX_RECORDS": "200"}, clear=False):
            pruned = ops._prune_catalog_records(catalog)
            ops._refresh_source_record_counts(catalog)
        self.assertEqual(pruned, 5)
        records = catalog.get("records") if isinstance(catalog.get("records"), dict) else {}
        self.assertEqual(len(records), 200)
        source_state = catalog.get("sources", {}).get("mcp_registry_official", {})
        self.assertEqual(int(source_state.get("record_count") or 0), 200)

    def test_search_remote_fallback_augments_catalog(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            self._write_allowlist_override(mcp_registry_level="mounted")
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            registry_payload = {
                "servers": [
                    {
                        "server": {
                            "name": "test-server",
                            "description": "Test MCP server",
                            "version": "1.0.0",
                            "remotes": [{"type": "http", "url": "http://127.0.0.1:9900/mcp"}],
                        },
                        "_meta": {"io.modelcontextprotocol.registry/official": {"status": "active"}},
                    }
                ],
                "metadata": {"nextCursor": ""},
            }
            with patch(
                "cccc.daemon.ops.capability_ops._http_get_json_obj",
                return_value=registry_payload,
            ):
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "test-server",
                        "include_external": True,
                        "limit": 20,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
            self.assertIn("mcp:test-server", ids)
            diag = result.get("search_diagnostics") if isinstance(result.get("search_diagnostics"), dict) else {}
            self.assertTrue(bool(diag.get("remote_augmented")))
            self.assertEqual(int(diag.get("remote_added") or 0), 1)

            catalog_path, catalog_doc = ops._load_catalog_doc()
            self.assertTrue(catalog_path.exists())
            self.assertIn("mcp:test-server", catalog_doc.get("records", {}))
        finally:
            cleanup()

    def test_search_remote_fallback_can_be_disabled(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            with patch.dict(os.environ, {"CCCC_CAPABILITY_SEARCH_REMOTE_FALLBACK": "0"}, clear=False), patch(
                "cccc.daemon.ops.capability_ops._http_get_json_obj",
                side_effect=AssertionError("remote fallback must be disabled"),
            ):
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "definitely-not-local",
                        "include_external": True,
                        "limit": 10,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            diag = result.get("search_diagnostics") if isinstance(result.get("search_diagnostics"), dict) else {}
            self.assertFalse(bool(diag.get("remote_augmented")))
        finally:
            cleanup()

    def test_enable_can_fetch_missing_mcp_record_from_registry(self) -> None:
        _, cleanup = self._with_home()
        try:
            self._write_allowlist_override(mcp_registry_level="mounted")
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            registry_payload = {
                "servers": [
                    {
                        "server": {
                            "name": "test-server",
                            "description": "Test MCP server",
                            "version": "1.0.0",
                            "remotes": [{"type": "http", "url": "http://127.0.0.1:9900/mcp"}],
                        },
                        "_meta": {"io.modelcontextprotocol.registry/official": {"status": "active"}},
                    }
                ],
                "metadata": {"nextCursor": ""},
            }
            installed = {
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [],
                "last_error": "",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            with patch(
                "cccc.daemon.ops.capability_ops._http_get_json_obj",
                return_value=registry_payload,
            ), patch(
                "cccc.daemon.ops.capability_ops._install_external_capability",
                return_value=installed,
            ):
                resp, _ = self._call(
                    "capability_enable",
                    {
                        "group_id": gid,
                        "by": "user",
                        "actor_id": "peer-1",
                        "capability_id": "mcp:test-server",
                        "scope": "session",
                        "enabled": True,
                        "approve": True,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "ready")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
