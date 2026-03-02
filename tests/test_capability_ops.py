from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from typing import Any
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

    def _write_allowlist_override(self, *, mcp_registry_level: str = "mounted", extra: str = "") -> Path:
        home = Path(str(os.environ.get("CCCC_HOME") or "")).expanduser()
        cfg_dir = home / "config"
        cfg_dir.mkdir(parents=True, exist_ok=True)
        path = cfg_dir / "capability-allowlist.user.yaml"
        body = (
            "defaults:\n"
            "  source_level:\n"
            f"    mcp_registry_official: {mcp_registry_level}\n"
            "    anthropic_skills: mounted\n"
            "    github_skills_curated: indexed\n"
            "    cccc_builtin: enabled\n"
        )
        if extra.strip():
            body = f"{body}{extra.rstrip()}\n"
        path.write_text(
            body,
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

    def _seed_runtime_external_install(
        self,
        ops: Any,
        runtime_doc: dict,
        *,
        capability_id: str = "mcp:test-server",
        url: str = "http://127.0.0.1:9900/mcp",
        synthetic_tool_name: str = "cccc_ext_deadbeef_echo",
        real_tool_name: str = "echo",
        state: str = "installed",
        last_error: str = "",
        tools: Any = None,
    ) -> str:
        rec = {
            "install_mode": "remote_only",
            "install_spec": {"transport": "http", "url": url},
        }
        install_key = ops._external_artifact_cache_key(rec, capability_id=capability_id)
        artifact_id = ops._external_artifact_id(rec, capability_id=capability_id)
        normalized_tools = (
            list(tools)
            if isinstance(tools, list)
            else [
                {
                    "name": synthetic_tool_name,
                    "real_tool_name": real_tool_name,
                    "description": f"{real_tool_name} tool",
                    "inputSchema": {"type": "object", "properties": {}, "required": []},
                }
            ]
        )
        install_payload = {
            "state": state,
            "installer": "remote_http",
            "install_mode": "remote_only",
            "invoker": {"type": "remote_http", "url": url},
            "tools": normalized_tools,
            "last_error": last_error,
            "updated_at": "2026-02-25T00:00:00Z",
        }
        artifact = ops._artifact_entry_from_install(
            install_payload,
            artifact_id=artifact_id,
            install_key=install_key,
            capability_id=capability_id,
        )
        ops._upsert_runtime_artifact_for_capability(
            runtime_doc,
            artifact_id=artifact_id,
            capability_id=capability_id,
            artifact_entry=artifact,
        )
        return artifact_id

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
            self.assertNotIn("cccc_space", visible)
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
            self.assertIn("cccc_space", visible)
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

    def test_search_without_query_returns_builtin_packs(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "query": "",
                    "kind": "mcp_toolpack",
                    "include_external": False,
                    "limit": 20,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
            self.assertIn("pack:space", ids)
            self.assertIn("pack:group-runtime", ids)
            self.assertGreaterEqual(len(ids), 5)
        finally:
            cleanup()

    def test_search_empty_query_uses_context_signal_for_pack_ranking(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            create_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [
                        {
                            "op": "task.create",
                            "name": "Automation reminder cleanup",
                            "goal": "stabilize reminder jobs",
                            "assignee": "peer-1",
                        }
                    ],
                },
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            tasks_resp, _ = self._call("task_list", {"group_id": gid})
            self.assertTrue(tasks_resp.ok, getattr(tasks_resp, "error", None))
            tasks = tasks_resp.result.get("tasks") if isinstance(tasks_resp.result, dict) else []
            self.assertTrue(isinstance(tasks, list) and tasks)
            task_id = str((tasks[0] if isinstance(tasks[0], dict) else {}).get("id") or "")
            self.assertTrue(task_id)
            sync_agent_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "ops": [
                        {
                            "op": "agent.update",
                            "agent_id": "peer-1",
                            "active_task_id": task_id,
                            "focus": "automation schedule hygiene",
                        }
                    ],
                },
            )
            self.assertTrue(sync_agent_resp.ok, getattr(sync_agent_resp, "error", None))

            resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "query": "",
                    "kind": "mcp_toolpack",
                    "include_external": False,
                    "limit": 5,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            self.assertTrue(items)
            top = items[0] if isinstance(items[0], dict) else {}
            self.assertEqual(str(top.get("capability_id") or ""), "pack:automation")
        finally:
            cleanup()

    def test_search_builtin_pack_includes_tool_names(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
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
            pack = next(
                (
                    item
                    for item in items
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == "pack:space"
                ),
                {},
            )
            tool_names = pack.get("tool_names") if isinstance(pack.get("tool_names"), list) else []
            self.assertIn("cccc_space", tool_names)
            self.assertGreaterEqual(int(pack.get("tool_count") or 0), len(tool_names))
        finally:
            cleanup()

    def test_allowlist_overlay_update_validate_and_reset(self) -> None:
        _, cleanup = self._with_home()
        try:
            get_before, _ = self._call("capability_allowlist_get", {"by": "user"})
            self.assertTrue(get_before.ok, getattr(get_before, "error", None))
            before = get_before.result if isinstance(get_before.result, dict) else {}
            revision_before = str(before.get("revision") or "")
            self.assertTrue(revision_before)

            validate, _ = self._call(
                "capability_allowlist_validate",
                {
                    "mode": "patch",
                    "patch": {
                        "defaults": {"source_level": {"mcp_registry_official": "indexed"}},
                    },
                },
            )
            self.assertTrue(validate.ok, getattr(validate, "error", None))
            validate_result = validate.result if isinstance(validate.result, dict) else {}
            self.assertTrue(bool(validate_result.get("valid")))

            update, _ = self._call(
                "capability_allowlist_update",
                {
                    "by": "user",
                    "mode": "patch",
                    "expected_revision": revision_before,
                    "patch": {
                        "defaults": {"source_level": {"mcp_registry_official": "indexed"}},
                    },
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))
            update_result = update.result if isinstance(update.result, dict) else {}
            revision_after = str(update_result.get("revision") or "")
            self.assertTrue(revision_after)
            self.assertNotEqual(revision_after, revision_before)
            effective = update_result.get("effective") if isinstance(update_result.get("effective"), dict) else {}
            defaults = effective.get("defaults") if isinstance(effective.get("defaults"), dict) else {}
            source_level = defaults.get("source_level") if isinstance(defaults.get("source_level"), dict) else {}
            self.assertEqual(str(source_level.get("mcp_registry_official") or ""), "indexed")

            reset, _ = self._call("capability_allowlist_reset", {"by": "user"})
            self.assertTrue(reset.ok, getattr(reset, "error", None))
            reset_result = reset.result if isinstance(reset.result, dict) else {}
            overlay_after_reset = (
                reset_result.get("overlay") if isinstance(reset_result.get("overlay"), dict) else {}
            )
            self.assertEqual(overlay_after_reset, {})
        finally:
            cleanup()

    def test_allowlist_update_rejects_revision_mismatch(self) -> None:
        _, cleanup = self._with_home()
        try:
            first, _ = self._call(
                "capability_allowlist_update",
                {
                    "by": "user",
                    "mode": "patch",
                    "patch": {"defaults": {"source_level": {"anthropic_skills": "mounted"}}},
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            stale_revision = "deadbeef"
            second, _ = self._call(
                "capability_allowlist_update",
                {
                    "by": "user",
                    "mode": "patch",
                    "expected_revision": stale_revision,
                    "patch": {"defaults": {"source_level": {"anthropic_skills": "indexed"}}},
                },
            )
            self.assertFalse(second.ok)
            self.assertEqual(getattr(second.error, "code", ""), "allowlist_revision_mismatch")
        finally:
            cleanup()

    def test_search_without_external_never_triggers_auto_sync(self) -> None:
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

    def test_search_with_external_never_triggers_auto_sync(self) -> None:
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

    def test_search_with_external_ignores_auto_sync_flag(self) -> None:
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
            auto_sync.assert_not_called()
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
            },
            clear=False,
        ), patch("cccc.daemon.ops.capability_ops._sync_mcp_registry_source", return_value=0), patch(
            "cccc.daemon.ops.capability_ops._sync_anthropic_skills_source",
            side_effect=AssertionError("anthropic source should be disabled"),
        ):
            changed = ops._auto_sync_catalog(catalog)

        self.assertTrue(changed)
        sources = catalog.get("sources", {})
        anthropic = sources.get("anthropic_skills", {})
        self.assertEqual(anthropic.get("sync_state"), "disabled")
        self.assertEqual(anthropic.get("error"), "source_disabled_by_policy")

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

    def test_external_enable_succeeds_for_qualified_external(self) -> None:
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
                "qualification_status": "qualified",
                "install_mode": "remote_only",
                "install_spec": {"transport": "http", "url": "http://127.0.0.1:9900/mcp"},
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
            with patch("cccc.daemon.ops.capability_ops._load_catalog_doc", return_value=(Path("/tmp/cat.json"), catalog)), patch(
                "cccc.daemon.ops.capability_ops._install_external_capability",
                return_value=installed,
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
            self.assertEqual(str(result.get("state") or ""), "ready")
            self.assertTrue(bool(result.get("enabled")))
        finally:
            cleanup()

    def test_external_enable_installs_and_exposes_dynamic_tools(self) -> None:
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
                "qualification_status": "qualified",
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

    def test_external_enable_reports_degraded_install_state(self) -> None:
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
                "qualification_status": "qualified",
                "install_mode": "package",
                "install_spec": {"registry_type": "npm", "identifier": "@example/mcp-server"},
            }
            installed = {
                "state": "installed_degraded",
                "installer": "npm_npx",
                "install_mode": "package",
                "invoker": {"type": "package_stdio", "command": ["npx", "-y", "@example/mcp-server"]},
                "tools": [],
                "last_error": "stdio mcp request timed out",
                "last_error_code": "probe_timeout",
                "updated_at": "2026-02-25T00:00:00Z",
            }
            with patch("cccc.daemon.ops.capability_ops._load_catalog_doc", return_value=(Path("/tmp/cat.json"), catalog)), patch(
                "cccc.daemon.ops.capability_ops._install_external_capability",
                return_value=installed,
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
            self.assertEqual(str(result.get("state") or ""), "ready")
            self.assertTrue(bool(result.get("degraded")))
            self.assertEqual(str(result.get("install_state") or ""), "installed_degraded")
            self.assertEqual(str(result.get("install_error_code") or ""), "probe_timeout")
            self.assertIn("tools_not_listed_call_capability_use", str(result.get("degraded_call_hint") or ""))
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
            artifact_id = self._seed_runtime_external_install(ops, runtime_doc)
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
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

    def test_capability_tool_call_accepts_real_tool_name_with_capability_hint(self) -> None:
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
            artifact_id = self._seed_runtime_external_install(ops, runtime_doc)
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
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
                        "capability_id": "mcp:test-server",
                        "tool_name": "echo",
                        "arguments": {"message": "hello"},
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("capability_id") or ""), "mcp:test-server")
            self.assertEqual(str(result.get("resolved_tool_name") or ""), "cccc_ext_deadbeef_echo")
            self.assertEqual(str(result.get("real_tool_name") or ""), "echo")
        finally:
            cleanup()

    def test_capability_tool_call_real_name_requires_capability_when_ambiguous(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            state_path, state_doc = ops._load_state_doc()
            for cap_id in ("mcp:test-server-a", "mcp:test-server-b"):
                ops._set_enabled_capability(
                    state_doc,
                    group_id=gid,
                    actor_id="peer-1",
                    scope="session",
                    capability_id=cap_id,
                    enabled=True,
                    ttl_seconds=600,
                )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            artifact_a = self._seed_runtime_external_install(
                ops,
                runtime_doc,
                capability_id="mcp:test-server-a",
                synthetic_tool_name="cccc_ext_deadbeef_echo_a",
                real_tool_name="echo",
            )
            artifact_b = self._seed_runtime_external_install(
                ops,
                runtime_doc,
                capability_id="mcp:test-server-b",
                synthetic_tool_name="cccc_ext_deadbeef_echo_b",
                real_tool_name="echo",
            )
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server-a",
                artifact_id=artifact_a,
                state="ready",
                last_error="",
            )
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server-b",
                artifact_id=artifact_b,
                state="ready",
                last_error="",
            )
            ops._save_runtime_doc(runtime_path, runtime_doc)

            resp, _ = self._call(
                "capability_tool_call",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "tool_name": "echo",
                    "arguments": {"message": "hello"},
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual((resp.error.code if resp.error else ""), "capability_tool_ambiguous")
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
            artifact_id = self._seed_runtime_external_install(ops, runtime_doc)
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
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
            self.assertEqual(str(result.get("reason") or ""), "install_failed:probe_failed")
            self.assertEqual(str(result.get("install_error_code") or ""), "probe_failed")
            self.assertTrue(bool(result.get("retryable")))
            diagnostics = result.get("diagnostics") if isinstance(result.get("diagnostics"), list) else []
            self.assertTrue(diagnostics)
            first_diag = diagnostics[0] if isinstance(diagnostics[0], dict) else {}
            self.assertEqual(str(first_diag.get("code") or ""), "probe_failed")

            _, runtime_doc = ops._load_runtime_doc()
            _, entry = ops._runtime_install_for_capability(runtime_doc, capability_id="mcp:test-server")
            self.assertIsInstance(entry, dict)
            self.assertEqual(str(entry.get("state") or ""), "install_failed")
            self.assertIn("probe_failed", str(entry.get("last_error") or ""))
        finally:
            cleanup()

    def test_external_install_failure_classifies_runtime_dependency_missing(self) -> None:
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
                "install_mode": "package",
                "install_spec": {"registry_type": "npm", "identifier": "@example/mcp"},
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            with patch(
                "cccc.daemon.ops.capability_ops._install_external_capability",
                side_effect=RuntimeError("stdio mcp exited with code 1: Error: Cannot find module 'foo'"),
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
            self.assertEqual(str(result.get("reason") or ""), "install_failed:runtime_dependency_missing")
            self.assertEqual(str(result.get("install_error_code") or ""), "runtime_dependency_missing")
            self.assertFalse(bool(result.get("retryable")))
            diagnostics = result.get("diagnostics") if isinstance(result.get("diagnostics"), list) else []
            self.assertTrue(diagnostics)
            first_diag = diagnostics[0] if isinstance(diagnostics[0], dict) else {}
            self.assertEqual(str(first_diag.get("code") or ""), "runtime_dependency_missing")
        finally:
            cleanup()

    def test_capability_tool_call_rejects_tool_when_capability_not_enabled(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            runtime_path, runtime_doc = ops._load_runtime_doc()
            artifact_id = self._seed_runtime_external_install(ops, runtime_doc)
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
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

    def test_capability_tool_call_requires_actor_runtime_binding(self) -> None:
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
            self._seed_runtime_external_install(ops, runtime_doc)
            # Intentionally no runtime actor binding for peer-1.
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

    def test_capability_tool_call_accepts_hyphen_underscore_alias(self) -> None:
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
            artifact_id = self._seed_runtime_external_install(
                ops,
                runtime_doc,
                capability_id="mcp:test-server",
                synthetic_tool_name="cccc_ext_deadbeef_resolve_library_id",
                real_tool_name="resolve_library_id",
                state="installed",
            )
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
            ops._save_runtime_doc(runtime_path, runtime_doc)

            with patch(
                "cccc.daemon.ops.capability_ops._invoke_installed_external_tool_with_aliases",
                return_value=({"ok": True}, "resolve_library_id"),
            ):
                resp, _ = self._call(
                    "capability_tool_call",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "capability_id": "mcp:test-server",
                        "tool_name": "resolve-library-id",
                        "arguments": {"libraryName": "cccc"},
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("real_tool_name") or ""), "resolve_library_id")
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
                "qualification_status": "unavailable",
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
                    "qualification_status": "unavailable",
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

    def test_search_surfaces_curated_third_party_skill_as_enable_now(self) -> None:
        _, cleanup = self._with_home()
        try:
            self._write_allowlist_override(
                extra=(
                    "skills:\n"
                    "  source_overrides:\n"
                    "    - source_id: github_skills_curated\n"
                    "      level: mounted\n"
                    "  curated:\n"
                    "    - capability_id: skill:github:blader:claudeception\n"
                    "      level: mounted\n"
                    "      source_id: github_skills_curated\n"
                    "      source_uri: https://github.com/blader/Claudeception\n"
                    "      qualification_status: qualified\n"
                    "      description_short: Captures reusable lessons.\n"
                )
            )
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            search_resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "query": "claudeception",
                    "kind": "skill",
                    "include_external": True,
                    "limit": 20,
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            result = search_resp.result if isinstance(search_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            row = next(
                (x for x in items if isinstance(x, dict) and str(x.get("capability_id") or "") == "skill:github:blader:claudeception"),
                None,
            )
            self.assertIsNotNone(row)
            item = row if isinstance(row, dict) else {}
            self.assertEqual(str(item.get("source_id") or ""), "github_skills_curated")
            self.assertEqual(str(item.get("qualification_status") or ""), "qualified")
            self.assertEqual(str(item.get("enable_hint") or ""), "enable_now")

            enable_resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "scope": "session",
                    "capability_id": "skill:github:blader:claudeception",
                    "enabled": True,
                },
            )
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))
            enable_result = enable_resp.result if isinstance(enable_resp.result, dict) else {}
            self.assertEqual(str(enable_result.get("state") or ""), "ready")
        finally:
            cleanup()

    def test_default_policy_surfaces_risky_third_party_mcp_not_hidden(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            search_resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "query": "desktop-commander",
                    "kind": "mcp_toolpack",
                    "include_external": True,
                    "limit": 20,
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            result = search_resp.result if isinstance(search_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            row = next(
                (
                    x
                    for x in items
                    if isinstance(x, dict)
                    and str(x.get("capability_id") or "") == "mcp:io.github.wonderwhy-er/desktop-commander"
                ),
                None,
            )
            self.assertIsNotNone(row)
            item = row if isinstance(row, dict) else {}
            self.assertEqual(str(item.get("policy_level") or ""), "mounted")
            self.assertIn(str(item.get("qualification_status") or ""), {"qualified", "unavailable"})
            self.assertIn(str(item.get("enable_hint") or ""), {"enable_now", "unsupported"})
        finally:
            cleanup()

    def test_search_hides_indexed_capability_by_default_policy(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            self._write_allowlist_override(mcp_registry_level="indexed")
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
            self._write_allowlist_override(mcp_registry_level="indexed")
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
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "failed")
            self.assertEqual(str(result.get("reason") or ""), "policy_level_indexed")
            self.assertEqual(str(result.get("policy_level") or ""), "indexed")
        finally:
            cleanup()

    def test_ensure_curated_catalog_records_refreshes_existing_entry(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        catalog = ops._new_catalog_doc()
        policy_v1 = ops._compile_allowlist_policy(
            {
                "skills": {
                    "curated": [
                        {
                            "capability_id": "skill:github:demo:s1",
                            "level": "mounted",
                            "source_id": "github_skills_curated",
                            "description_short": "v1",
                        }
                    ]
                }
            }
        )
        changed_v1 = ops._ensure_curated_catalog_records(catalog, policy=policy_v1)
        self.assertTrue(changed_v1)

        rec_v1 = catalog.get("records", {}).get("skill:github:demo:s1")
        self.assertIsInstance(rec_v1, dict)
        self.assertEqual(str((rec_v1 or {}).get("description_short") or ""), "v1")

        policy_v2 = ops._compile_allowlist_policy(
            {
                "skills": {
                    "curated": [
                        {
                            "capability_id": "skill:github:demo:s1",
                            "level": "enabled",
                            "source_id": "github_skills_curated",
                            "description_short": "v2",
                        }
                    ]
                }
            }
        )
        changed_v2 = ops._ensure_curated_catalog_records(catalog, policy=policy_v2)
        self.assertTrue(changed_v2)

        rec_v2 = catalog.get("records", {}).get("skill:github:demo:s1")
        self.assertIsInstance(rec_v2, dict)
        self.assertEqual(str((rec_v2 or {}).get("description_short") or ""), "v2")

    def test_capability_state_reports_scope_mismatch_and_unavailable_hidden_reasons(self) -> None:
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
                "qualification_status": "unavailable",
                "enable_supported": False,
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
            self.assertEqual(by_id.get("mcp:manual"), "unavailable")
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
            artifact_id = self._seed_runtime_external_install(ops, runtime_doc, tools=[])
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
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
            _, install_after = ops._runtime_install_for_capability(runtime_doc_after, capability_id="mcp:test-server")
            self.assertIsNone(install_after)
        finally:
            cleanup()

    def test_capability_uninstall_isolated_to_target_group(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid_a = self._create_group(title="cap-uninstall-a")
            gid_b = self._create_group(title="cap-uninstall-b")
            self._add_actor(gid_a, "peer-1", by="user")
            self._add_actor(gid_b, "peer-1", by="user")

            state_path, state_doc = ops._load_state_doc()
            ops._set_enabled_capability(
                state_doc,
                group_id=gid_a,
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._set_enabled_capability(
                state_doc,
                group_id=gid_b,
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            artifact_id = self._seed_runtime_external_install(ops, runtime_doc, capability_id="mcp:test-server")
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid_a,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid_b,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
            ops._save_runtime_doc(runtime_path, runtime_doc)

            uninstall_resp, _ = self._call(
                "capability_uninstall",
                {
                    "group_id": gid_a,
                    "by": "user",
                    "actor_id": "peer-1",
                    "capability_id": "mcp:test-server",
                    "reason": "group-a cleanup",
                },
            )
            self.assertTrue(uninstall_resp.ok, getattr(uninstall_resp, "error", None))
            uninstall_result = uninstall_resp.result if isinstance(uninstall_resp.result, dict) else {}
            self.assertFalse(bool(uninstall_result.get("removed_installation")))
            self.assertEqual(
                str(uninstall_result.get("cleanup_skipped_reason") or ""),
                "cleanup_skipped_capability_still_bound",
            )

            _, state_doc_after = ops._load_state_doc()
            enabled_a, _ = ops._collect_enabled_capabilities(state_doc_after, group_id=gid_a, actor_id="peer-1")
            enabled_b, _ = ops._collect_enabled_capabilities(state_doc_after, group_id=gid_b, actor_id="peer-1")
            self.assertNotIn("mcp:test-server", set(enabled_a))
            self.assertIn("mcp:test-server", set(enabled_b))

            state_b_resp, _ = self._call(
                "capability_state",
                {"group_id": gid_b, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_b_resp.ok, getattr(state_b_resp, "error", None))
            state_b = state_b_resp.result if isinstance(state_b_resp.result, dict) else {}
            visible_b = state_b.get("visible_tools") if isinstance(state_b.get("visible_tools"), list) else []
            self.assertIn("cccc_ext_deadbeef_echo", visible_b)
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
            artifact_id = self._seed_runtime_external_install(
                ops,
                runtime_doc,
                tools=[
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
            )
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
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

    def test_group_block_by_foreman_revokes_binding_and_hides_tools(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "foreman-1", by="user")
            self._add_actor(gid, "peer-1", by="user")
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
                actor_id="peer-1",
                scope="session",
                capability_id="mcp:test-server",
                enabled=True,
                ttl_seconds=600,
            )
            ops._save_state_doc(state_path, state_doc)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            artifact_id = self._seed_runtime_external_install(ops, runtime_doc, tools=[])
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="ready",
                last_error="",
            )
            ops._save_runtime_doc(runtime_path, runtime_doc)

            block_resp, _ = self._call(
                "capability_block",
                {
                    "group_id": gid,
                    "by": "foreman-1",
                    "actor_id": "foreman-1",
                    "scope": "group",
                    "capability_id": "mcp:test-server",
                    "blocked": True,
                    "reason": "side_effect_detected",
                },
            )
            self.assertTrue(block_resp.ok, getattr(block_resp, "error", None))
            block_result = block_resp.result if isinstance(block_resp.result, dict) else {}
            self.assertTrue(bool(block_result.get("refresh_required")))
            self.assertGreaterEqual(int(block_result.get("removed_bindings") or 0), 1)

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state_result = state_resp.result if isinstance(state_resp.result, dict) else {}
            self.assertNotIn("mcp:test-server", state_result.get("enabled_capabilities") or [])
            blocked_rows = state_result.get("blocked_capabilities") if isinstance(state_result.get("blocked_capabilities"), list) else []
            blocked_ids = {str(item.get("capability_id") or "") for item in blocked_rows if isinstance(item, dict)}
            self.assertIn("mcp:test-server", blocked_ids)

            enable_resp, _ = self._call(
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
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))
            enable_result = enable_resp.result if isinstance(enable_resp.result, dict) else {}
            self.assertEqual(str(enable_result.get("reason") or ""), "blocked_by_group_policy")
        finally:
            cleanup()

    def test_group_block_requires_foreman(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "foreman-1", by="user")
            self._add_actor(gid, "peer-1", by="user")
            self._call(
                "actor_update",
                {
                    "group_id": gid,
                    "actor_id": "foreman-1",
                    "by": "user",
                    "patch": {"role": "foreman"},
                },
            )
            self._call(
                "actor_update",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "user",
                    "patch": {"role": "peer"},
                },
            )
            resp, _ = self._call(
                "capability_block",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "scope": "group",
                    "capability_id": "mcp:test-server",
                    "blocked": True,
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp.error, "code", "")), "permission_denied")
        finally:
            cleanup()

    def test_global_block_requires_user(self) -> None:
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
            resp, _ = self._call(
                "capability_block",
                {
                    "group_id": gid,
                    "by": "foreman-1",
                    "actor_id": "foreman-1",
                    "scope": "global",
                    "capability_id": "mcp:test-server",
                    "blocked": True,
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp.error, "code", "")), "permission_denied")
        finally:
            cleanup()

    def test_capability_overview_returns_builtin_items_and_sources(self) -> None:
        _, cleanup = self._with_home()
        try:
            resp, _ = self._call(
                "capability_overview",
                {
                    "query": "",
                    "limit": 200,
                    "include_indexed": True,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            self.assertTrue(items)
            ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
            self.assertIn("pack:group-runtime", ids)
            sources = result.get("sources") if isinstance(result.get("sources"), dict) else {}
            self.assertIn("mcp_registry_official", sources)
        finally:
            cleanup()

    def test_capability_overview_includes_recent_success_entry(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            runtime_path, runtime_doc = ops._load_runtime_doc()
            ops._record_runtime_recent_success(
                runtime_doc,
                capability_id="mcp:io.github.upstash/context7",
                group_id="g_recent",
                actor_id="peer-1",
                action="enable",
            )
            ops._save_runtime_doc(runtime_path, runtime_doc)

            resp, _ = self._call(
                "capability_overview",
                {
                    "query": "context7",
                    "limit": 50,
                    "include_indexed": True,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            target = next(
                (
                    item
                    for item in items
                    if isinstance(item, dict)
                    and str(item.get("capability_id") or "") == "mcp:io.github.upstash/context7"
                ),
                {},
            )
            self.assertTrue(target)
            recent = target.get("recent_success") if isinstance(target.get("recent_success"), dict) else {}
            self.assertEqual(int(recent.get("success_count") or 0), 1)
            self.assertEqual(str(recent.get("last_action") or ""), "enable")
        finally:
            cleanup()

    def test_capability_overview_marks_global_blocked_entries(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            block_resp, _ = self._call(
                "capability_block",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "user",
                    "scope": "global",
                    "capability_id": "pack:space",
                    "blocked": True,
                    "reason": "manual_block_test",
                },
            )
            self.assertTrue(block_resp.ok, getattr(block_resp, "error", None))

            overview_resp, _ = self._call(
                "capability_overview",
                {
                    "query": "pack:space",
                    "limit": 50,
                    "include_indexed": True,
                },
            )
            self.assertTrue(overview_resp.ok, getattr(overview_resp, "error", None))
            result = overview_resp.result if isinstance(overview_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            target = next(
                (
                    item
                    for item in items
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == "pack:space"
                ),
                {},
            )
            self.assertTrue(target)
            self.assertTrue(bool(target.get("blocked_global")))
            blocked = result.get("blocked_capabilities") if isinstance(result.get("blocked_capabilities"), list) else []
            blocked_ids = {str(item.get("capability_id") or "") for item in blocked if isinstance(item, dict)}
            self.assertIn("pack:space", blocked_ids)
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
            autoload_skills = state.get("autoload_skills") if isinstance(state.get("autoload_skills"), list) else []
            autoload_ids = {str(item.get("capability_id") or "") for item in autoload_skills if isinstance(item, dict)}
            self.assertNotIn("skill:anthropic:write-pr", autoload_ids)
            binding_states = (
                state.get("external_binding_states") if isinstance(state.get("external_binding_states"), dict) else {}
            )
            skill_binding = binding_states.get("skill:anthropic:write-pr") if isinstance(binding_states, dict) else {}
            self.assertEqual(str((skill_binding or {}).get("mode") or ""), "skill")
        finally:
            cleanup()

    def test_skill_actor_scope_enable_is_active_not_autoload(self) -> None:
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
            autoload_skills = state.get("autoload_skills") if isinstance(state.get("autoload_skills"), list) else []
            autoload_ids = {str(item.get("capability_id") or "") for item in autoload_skills if isinstance(item, dict)}
            # actor-scope enable does not mutate startup autoload config.
            self.assertNotIn("skill:anthropic:triage", autoload_ids)
            active_skills = state.get("active_skills") if isinstance(state.get("active_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_skills if isinstance(item, dict)}
            self.assertIn("skill:anthropic:triage", active_ids)
        finally:
            cleanup()

    def test_actor_autoload_skill_is_reported_in_autoload_skills(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            add_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "runtime": "codex",
                    "runner": "headless",
                    "capability_autoload": ["skill:anthropic:triage"],
                    "by": "user",
                },
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

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
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            actor_autoload = (
                state.get("actor_autoload_capabilities")
                if isinstance(state.get("actor_autoload_capabilities"), list)
                else []
            )
            self.assertIn("skill:anthropic:triage", actor_autoload)
            autoload_skills = state.get("autoload_skills") if isinstance(state.get("autoload_skills"), list) else []
            autoload_ids = {str(item.get("capability_id") or "") for item in autoload_skills if isinstance(item, dict)}
            self.assertIn("skill:anthropic:triage", autoload_ids)
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
            self._seed_runtime_external_install(ops, runtime_doc, tools=[])
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
            _, install_after = ops._runtime_install_for_capability(runtime_after, capability_id="mcp:test-server")
            self.assertIsNone(install_after)
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
            self._seed_runtime_external_install(ops, runtime_doc, tools=[])
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
            _, install_after = ops._runtime_install_for_capability(runtime_after, capability_id="mcp:test-server")
            self.assertIsInstance(install_after, dict)
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
            ), patch.dict(
                os.environ,
                {
                    "CCCC_CAPABILITY_SOURCE_SKILLSMP_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_CLAWHUB_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_OPENCLAW_SKILLS_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_CLAWSKILLS_REMOTE_ENABLED": "0",
                },
                clear=False,
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

    def test_search_remote_fallback_augments_skill_from_openclaw(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            tree_payload = {
                "tree": [
                    {
                        "path": "skills/demo/rareskilltoken/SKILL.md",
                        "type": "blob",
                    }
                ]
            }
            openclaw_markdown = (
                "---\n"
                "name: rareskilltoken\n"
                "description: Unique OpenClaw test skill\n"
                "---\n"
                "Workflow notes\n"
            )

            def _fake_github_json(url: str, *, headers=None, timeout=10.0):
                if "/git/trees/" in str(url):
                    return tree_payload
                raise AssertionError(f"unexpected github URL: {url}")

            with patch(
                "cccc.daemon.ops.capability_ops._http_get_json_obj",
                side_effect=_fake_github_json,
            ), patch(
                "cccc.daemon.ops.capability_ops._http_get_text",
                return_value=openclaw_markdown,
            ), patch.dict(
                "cccc.daemon.ops.capability_ops._OPENCLAW_TREE_CACHE",
                {"fetched_at": 0.0, "paths": []},
                clear=True,
            ), patch.dict(
                os.environ,
                {
                    "CCCC_CAPABILITY_SOURCE_CLAWHUB_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_CLAWSKILLS_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_OPENCLAW_SKILLS_REMOTE_ENABLED": "1",
                    "CCCC_CAPABILITY_SOURCE_SKILLSMP_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_OPENCLAW_FRONTMATTER_FETCH_MAX": "1",
                },
                clear=False,
            ):
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "rareskilltoken",
                        "kind": "skill",
                        "include_external": True,
                        "limit": 20,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            row = next(
                (
                    x
                    for x in items
                    if isinstance(x, dict)
                    and str(x.get("capability_id") or "").startswith("skill:openclaw:rareskilltoken-")
                ),
                None,
            )
            self.assertIsNotNone(row)
            item = row if isinstance(row, dict) else {}
            self.assertEqual(str(item.get("source_id") or ""), "openclaw_skills_remote")
            self.assertEqual(str(item.get("enable_hint") or ""), "enable_now")
            diag = result.get("search_diagnostics") if isinstance(result.get("search_diagnostics"), dict) else {}
            self.assertTrue(bool(diag.get("remote_augmented")))
            self.assertGreaterEqual(int(diag.get("remote_added") or 0), 1)

            catalog_path, catalog_doc = ops._load_catalog_doc()
            self.assertTrue(catalog_path.exists())
            cached_ids = {
                str(k or "")
                for k in (
                    catalog_doc.get("records").keys()
                    if isinstance(catalog_doc.get("records"), dict)
                    else []
                )
            }
            self.assertTrue(any(cid.startswith("skill:openclaw:rareskilltoken-") for cid in cached_ids))
        finally:
            cleanup()

    def test_search_remote_skill_fallback_can_be_disabled(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            with patch.dict(
                os.environ,
                {
                    "CCCC_CAPABILITY_SOURCE_SKILLSMP_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_CLAWHUB_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_OPENCLAW_SKILLS_REMOTE_ENABLED": "0",
                    "CCCC_CAPABILITY_SOURCE_CLAWSKILLS_REMOTE_ENABLED": "0",
                },
                clear=False,
            ), patch(
                "cccc.daemon.ops.capability_ops._http_get_json_obj",
                side_effect=AssertionError("openclaw remote fallback must be disabled"),
            ), patch(
                "cccc.daemon.ops.capability_ops._http_get_text",
                side_effect=AssertionError("skillsmp/clawhub/clawskills remote fallback must be disabled"),
            ):
                resp, _ = self._call(
                    "capability_search",
                    {
                        "group_id": gid,
                        "actor_id": "peer-1",
                        "by": "peer-1",
                        "query": "creative",
                        "kind": "skill",
                        "include_external": True,
                        "limit": 20,
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            diag = result.get("search_diagnostics") if isinstance(result.get("search_diagnostics"), dict) else {}
            self.assertFalse(bool(diag.get("remote_augmented")))
        finally:
            cleanup()

    def test_parse_skillsmp_proxy_search_markdown(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        markdown = (
            '[claudeception.md 1.7k ### export claudeception from "blader/Claudeception" '
            "Continuous learning skill 2026-02-21]"
            "(https://skillsmp.com/skills/blader-claudeception-skill-md)\n"
        )
        rows = ops._parse_skillsmp_proxy_search_markdown(markdown, limit=10)
        self.assertTrue(rows)
        first = rows[0] if isinstance(rows[0], dict) else {}
        self.assertEqual(str(first.get("source_id") or ""), "skillsmp_remote")
        self.assertEqual(str(first.get("kind") or ""), "skill")
        self.assertTrue(str(first.get("capability_id") or "").startswith("skill:skillsmp:"))
        self.assertIn("Continuous learning skill", str(first.get("description_short") or ""))

    def test_remote_search_skill_records_aggregates_sources(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        with patch(
            "cccc.daemon.ops.capability_ops._remote_search_skillsmp_records",
            return_value=[
                {
                    "capability_id": "skill:skillsmp:a",
                    "kind": "skill",
                    "name": "a",
                    "description_short": "a",
                    "source_id": "skillsmp_remote",
                }
            ],
        ), patch(
            "cccc.daemon.ops.capability_ops._remote_search_openclaw_skill_records",
            return_value=[
                {
                    "capability_id": "skill:openclaw:b",
                    "kind": "skill",
                    "name": "b",
                    "description_short": "b",
                    "source_id": "openclaw_skills_remote",
                }
            ],
        ), patch(
            "cccc.daemon.ops.capability_ops._remote_search_clawhub_records",
            return_value=[
                {
                    "capability_id": "skill:clawhub:d",
                    "kind": "skill",
                    "name": "d",
                    "description_short": "d",
                    "source_id": "clawhub_remote",
                }
            ],
        ), patch(
            "cccc.daemon.ops.capability_ops._remote_search_clawskills_records",
            return_value=[
                {
                    "capability_id": "skill:clawskills:c",
                    "kind": "skill",
                    "name": "c",
                    "description_short": "c",
                    "source_id": "clawskills_remote",
                }
            ],
        ):
            rows = ops._remote_search_skill_records(query="skill", limit=4)
        self.assertEqual(len(rows), 4)
        ids = {str(item.get("capability_id") or "") for item in rows if isinstance(item, dict)}
        self.assertIn("skill:skillsmp:a", ids)
        self.assertIn("skill:openclaw:b", ids)
        self.assertIn("skill:clawhub:d", ids)
        self.assertIn("skill:clawskills:c", ids)

    def test_remote_search_skill_records_honors_source_filter(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        with patch(
            "cccc.daemon.ops.capability_ops._remote_search_skillsmp_records",
            return_value=[
                {
                    "capability_id": "skill:skillsmp:a",
                    "kind": "skill",
                    "name": "a",
                    "description_short": "a",
                    "source_id": "skillsmp_remote",
                }
            ],
        ), patch(
            "cccc.daemon.ops.capability_ops._remote_search_openclaw_skill_records",
            return_value=[
                {
                    "capability_id": "skill:openclaw:b",
                    "kind": "skill",
                    "name": "b",
                    "description_short": "b",
                    "source_id": "openclaw_skills_remote",
                }
            ],
        ), patch(
            "cccc.daemon.ops.capability_ops._remote_search_clawhub_records",
            return_value=[
                {
                    "capability_id": "skill:clawhub:d",
                    "kind": "skill",
                    "name": "d",
                    "description_short": "d",
                    "source_id": "clawhub_remote",
                }
            ],
        ), patch(
            "cccc.daemon.ops.capability_ops._remote_search_clawskills_records",
            return_value=[
                {
                    "capability_id": "skill:clawskills:c",
                    "kind": "skill",
                    "name": "c",
                    "description_short": "c",
                    "source_id": "clawskills_remote",
                }
            ],
        ):
            rows = ops._remote_search_skill_records(query="skill", limit=5, source_filter="openclaw_skills_remote")
        self.assertEqual(len(rows), 1)
        self.assertEqual(str(rows[0].get("source_id") or ""), "openclaw_skills_remote")

    def test_parse_clawskills_data_js(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        script = (
            "// Auto-generated from awesome-openclaw-skills README.md\n"
            "var SKILLS_DATA = [\n"
            "  {name:'humanizer',desc:\"Remove AI tone from text\",category:'Writing',slug:'humanizer',author:'biostartechnology'},\n"
            "  {name:'foo',desc:\"bar\",category:'Misc',slug:'foo',author:'alice'}\n"
            "];\n"
        )
        rows = ops._parse_clawskills_data_js(script=script, query="humanizer", limit=10)
        self.assertTrue(rows)
        first = rows[0] if isinstance(rows[0], dict) else {}
        self.assertEqual(str(first.get("source_id") or ""), "clawskills_remote")
        self.assertEqual(str(first.get("name") or ""), "humanizer")
        self.assertTrue(str(first.get("capability_id") or "").startswith("skill:clawskills:"))

    def test_clawhub_item_to_record(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        record = ops._clawhub_item_to_record(
            {
                "slug": "humanizer",
                "displayName": "Humanizer",
                "summary": "Remove AI tone from text",
                "latestVersion": {"version": "1.2.3"},
            },
            now_iso="2026-02-28T00:00:00Z",
        )
        self.assertIsInstance(record, dict)
        row = record if isinstance(record, dict) else {}
        self.assertEqual(str(row.get("source_id") or ""), "clawhub_remote")
        self.assertEqual(str(row.get("name") or ""), "humanizer")
        self.assertEqual(str(row.get("source_record_version") or ""), "1.2.3")
        self.assertTrue(str(row.get("capability_id") or "").startswith("skill:clawhub:"))

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
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "ready")
        finally:
            cleanup()

    def test_supported_external_install_record_accepts_pypi_and_oci(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        supported_pypi, reason_pypi = ops._supported_external_install_record(
            {
                "install_mode": "package",
                "install_spec": {
                    "registry_type": "pypi",
                    "identifier": "example-mcp",
                    "version": "1.2.3",
                    "runtime_hint": "uvx",
                },
            }
        )
        self.assertTrue(supported_pypi, reason_pypi)
        self.assertEqual(reason_pypi, "")

        supported_oci, reason_oci = ops._supported_external_install_record(
            {
                "install_mode": "package",
                "install_spec": {
                    "registry_type": "oci",
                    "identifier": "ghcr.io/example/mcp:0.1.0",
                    "runtime_hint": "docker",
                },
            }
        )
        self.assertTrue(supported_oci, reason_oci)
        self.assertEqual(reason_oci, "")

        supported_alias_python, reason_alias_python = ops._supported_external_install_record(
            {
                "install_mode": "package",
                "install_spec": {
                    "registry_type": "python",
                    "identifier": "example-mcp",
                    "runtime_hint": "pipx",
                },
            }
        )
        self.assertTrue(supported_alias_python, reason_alias_python)
        self.assertEqual(reason_alias_python, "")

        supported_alias_docker, reason_alias_docker = ops._supported_external_install_record(
            {
                "install_mode": "package",
                "install_spec": {
                    "registry_type": "docker",
                    "identifier": "ghcr.io/example/mcp:0.1.0",
                },
            }
        )
        self.assertTrue(supported_alias_docker, reason_alias_docker)
        self.assertEqual(reason_alias_docker, "")

    def test_install_external_capability_pypi_prefers_uvx(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "pypi",
                "identifier": "realtimex-browser-use",
                "version": "0.7.10",
                "runtime_hint": "uvx",
                "runtime_arguments": [{"type": "positional", "value": "realtimex-browser-use[cli]@0.7.10"}],
                "package_arguments": [{"type": "positional", "value": "--mcp"}],
            },
        }

        with patch(
            "cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip",
            return_value=[
                {"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "search"}]}},
            ],
        ) as probe:
            install = ops._install_external_capability(rec, capability_id="mcp:io.github.therealtimex/browser-use")

        self.assertEqual(str(install.get("installer") or ""), "pypi_uvx")
        invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
        self.assertEqual(str(invoker.get("type") or ""), "package_stdio")
        command = invoker.get("command") if isinstance(invoker.get("command"), list) else []
        self.assertTrue(command and str(command[0]) == "uvx")
        self.assertIn("--mcp", [str(x) for x in command])
        called_cmd = probe.call_args[0][0]
        self.assertTrue(isinstance(called_cmd, list) and called_cmd and str(called_cmd[0]) == "uvx")

    def test_install_external_capability_oci_builds_container_command(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "oci",
                "identifier": "ghcr.io/github/github-mcp-server:0.31.0",
                "runtime_hint": "docker",
                "runtime_arguments": [
                    {"type": "named", "name": "-e", "value": "GITHUB_PERSONAL_ACCESS_TOKEN={token}"}
                ],
                "required_env": ["GITHUB_PERSONAL_ACCESS_TOKEN"],
            },
        }

        with patch.dict(os.environ, {"GITHUB_PERSONAL_ACCESS_TOKEN": "dummy-token"}, clear=False), patch(
            "cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip",
            return_value=[
                {"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "create_issue"}]}},
            ],
        ) as probe:
            install = ops._install_external_capability(rec, capability_id="mcp:io.github.github/github-mcp-server")

        self.assertEqual(str(install.get("installer") or ""), "oci_docker")
        invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
        command = invoker.get("command") if isinstance(invoker.get("command"), list) else []
        self.assertGreaterEqual(len(command), 6)
        self.assertEqual(str(command[0]), "docker")
        self.assertEqual(str(command[1]), "run")
        self.assertIn("ghcr.io/github/github-mcp-server:0.31.0", [str(x) for x in command])
        self.assertIn("GITHUB_PERSONAL_ACCESS_TOKEN", [str(x) for x in command])
        called_cmd = probe.call_args[0][0]
        self.assertTrue(isinstance(called_cmd, list) and called_cmd and str(called_cmd[0]) == "docker")

    def test_install_external_capability_fails_fast_when_required_env_missing(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "npm",
                "identifier": "@brave/brave-search-mcp-server",
                "version": "2.0.75",
                "required_env": ["BRAVE_API_KEY"],
            },
        }
        with patch.dict(os.environ, {"BRAVE_API_KEY": ""}, clear=False):
            with self.assertRaises(ValueError) as ctx:
                ops._install_external_capability(rec, capability_id="mcp:io.github.brave/brave-search-mcp-server")
        self.assertIn("missing_required_env:BRAVE_API_KEY", str(ctx.exception))

    def test_install_external_capability_package_falls_back_to_next_runner(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "pypi",
                "identifier": "demo-mcp",
                "version": "1.0.0",
                "runtime_hint": "auto",
            },
        }

        def _roundtrip(cmd, requests, *, timeout_s):  # type: ignore[no-untyped-def]
            if isinstance(cmd, list) and cmd and str(cmd[0]) == "uvx":
                raise RuntimeError("uvx failed")
            return [{"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "demo"}]}}]

        with patch("cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip", side_effect=_roundtrip):
            install = ops._install_external_capability(rec, capability_id="mcp:demo/runner-fallback")

        self.assertEqual(str(install.get("installer") or ""), "pypi_pipx")
        invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
        command = invoker.get("command") if isinstance(invoker.get("command"), list) else []
        self.assertTrue(command and str(command[0]) == "pipx")

    def test_install_external_capability_npm_retries_with_safe_env_flags(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "npm",
                "identifier": "@example/mcp-server",
            },
        }

        call_count = {"n": 0}

        def _roundtrip(cmd, requests, *, timeout_s, env_override=None):  # type: ignore[no-untyped-def]
            call_count["n"] += 1
            if not env_override:
                raise RuntimeError("stdio mcp exited with code 1: Error: Cannot find module 'puppeteer'")
            return [{"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "demo"}]}}]

        with patch("cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip", side_effect=_roundtrip):
            install = ops._install_external_capability(rec, capability_id="mcp:example/npm-autofix")

        self.assertEqual(str(install.get("state") or ""), "installed")
        self.assertEqual(str(install.get("installer") or ""), "npm_npx")
        invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
        self.assertEqual(str(invoker.get("type") or ""), "package_stdio")
        self.assertIn("env", invoker)
        env = invoker.get("env") if isinstance(invoker.get("env"), dict) else {}
        self.assertEqual(str(env.get("PUPPETEER_SKIP_DOWNLOAD") or ""), "1")
        self.assertGreaterEqual(call_count["n"], 2)

    def test_install_external_capability_npm_falls_back_to_unpinned_version(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "npm",
                "identifier": "@example/mcp-server",
                "version": "1.0.0",
            },
        }

        def _roundtrip(cmd, requests, *, timeout_s, env_override=None):  # type: ignore[no-untyped-def]
            token = " ".join(str(x) for x in cmd)
            if "@example/mcp-server@1.0.0" in token:
                raise RuntimeError("stdio mcp exited with code 1: Error: Cannot find module 'broken-version'")
            return [{"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "demo"}]}}]

        with patch("cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip", side_effect=_roundtrip):
            install = ops._install_external_capability(rec, capability_id="mcp:example/npm-unpinned-fallback")

        self.assertEqual(str(install.get("state") or ""), "installed")
        invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
        command = invoker.get("command") if isinstance(invoker.get("command"), list) else []
        cmd_token = " ".join(str(x) for x in command)
        self.assertIn("@example/mcp-server", cmd_token)
        self.assertNotIn("@example/mcp-server@1.0.0", cmd_token)

    def test_install_external_capability_degrades_when_probe_times_out(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "npm",
                "identifier": "@example/mcp-server",
                "runtime_hint": "node",
            },
        }

        with patch("cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip", side_effect=TimeoutError("stdio mcp request timed out")), patch(
            "cccc.daemon.ops.capability_ops._choose_available_command",
            return_value=[["npx", "-y", "@example/mcp-server"]],
        ):
            install = ops._install_external_capability(rec, capability_id="mcp:example/timeout")

            self.assertEqual(str(install.get("state") or ""), "installed_degraded")
            self.assertEqual(str(install.get("installer") or ""), "npm_npx")
            invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
            self.assertEqual(str(invoker.get("type") or ""), "package_stdio")
            command = invoker.get("command") if isinstance(invoker.get("command"), list) else []
            self.assertTrue(command and str(command[0]) == "npx")
        self.assertEqual(install.get("tools"), [])
        self.assertEqual(str(install.get("last_error_code") or ""), "probe_timeout")

    def test_classify_external_install_error_detects_runtime_permission_denied(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        info = ops._classify_external_install_error(
            RuntimeError(
                "docker: permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock"
            )
        )
        self.assertEqual(str(info.get("code") or ""), "runtime_permission_denied")

    def test_capability_uninstall_skill_removes_bindings(self) -> None:
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
                "description_short": "triage helper",
                "source_id": "anthropic_skills",
                "source_tier": "tier1",
                "trust_tier": "tier1",
                "qualification_status": "qualified",
                "enable_supported": True,
                "install_mode": "builtin",
                "install_spec": {},
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

            uninstall_resp, _ = self._call(
                "capability_uninstall",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "peer-1",
                    "capability_id": "skill:anthropic:triage",
                    "reason": "cleanup skill binding",
                },
            )
            self.assertTrue(uninstall_resp.ok, getattr(uninstall_resp, "error", None))
            result = uninstall_resp.result if isinstance(uninstall_resp.result, dict) else {}
            self.assertGreaterEqual(int(result.get("removed_bindings") or 0), 1)
            self.assertFalse(bool(result.get("removed_installation")))

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            enabled = set(state.get("enabled_capabilities") or [])
            self.assertNotIn("skill:anthropic:triage", enabled)
            active_skills = state.get("active_skills") if isinstance(state.get("active_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_skills if isinstance(item, dict)}
            self.assertNotIn("skill:anthropic:triage", active_ids)
            autoload_skills = state.get("autoload_skills") if isinstance(state.get("autoload_skills"), list) else []
            autoload_ids = {str(item.get("capability_id") or "") for item in autoload_skills if isinstance(item, dict)}
            self.assertNotIn("skill:anthropic:triage", autoload_ids)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
