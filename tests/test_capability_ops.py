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
        td_ctx = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
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
            self.assertIn("cccc_file", visible)
            self.assertNotIn("cccc_space", visible)
        finally:
            cleanup()

    def test_pet_capability_state_uses_minimal_core_surface(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.pet_actor import ensure_pet_actor

            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            group = load_group(gid)
            self.assertIsNotNone(group)
            ensure_pet_actor(group)

            state_resp, _ = self._call("capability_state", {"group_id": gid, "actor_id": "pet-peer", "by": "pet-peer"})
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            result = state_resp.result if isinstance(state_resp.result, dict) else {}
            visible = set(result.get("visible_tools") or [])
            autoload = set(result.get("autoload_capabilities") or [])

            self.assertIn("cccc_help", visible)
            self.assertIn("cccc_context_get", visible)
            self.assertIn("cccc_agent_state", visible)
            self.assertIn("pack:pet", autoload)
            self.assertNotIn("cccc_message_send", visible)
            self.assertNotIn("cccc_message_reply", visible)
            self.assertNotIn("cccc_file", visible)
            self.assertNotIn("cccc_coordination", visible)
            self.assertNotIn("cccc_task", visible)
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

    def test_search_without_external_catalog_returns_builtin_skill_for_symptom_query(self) -> None:
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
                    "query": "web startup",
                    "kind": "skill",
                    "include_external": False,
                    "limit": 20,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            skill = next(
                (
                    item
                    for item in items
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == "skill:cccc:runtime-bootstrap"
                ),
                {},
            )
            self.assertEqual(str(skill.get("source_id") or ""), "cccc_builtin")
            self.assertEqual(str(skill.get("kind") or ""), "skill")
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
                            "title": "Automation reminder cleanup",
                            "outcome": "stabilize reminder jobs",
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
                            "op": "agent_state.update",
                            "actor_id": "peer-1",
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
            self.assertEqual(str(before.get("external_capability_safety_mode") or ""), "normal")

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
            self.assertEqual(str(reset_result.get("external_capability_safety_mode") or ""), "normal")
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

    def test_sync_superpowers_skills_accepts_github_tree_payload(self) -> None:
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
            "name: example-power\n"
            "description: Example Superpowers skill\n"
            "license: MIT\n"
            "---\n"
            "Use this skill for focused examples.\n"
        )
        tree_payload = {
            "tree": [
                {"type": "blob", "path": "README.md", "sha": "ignore"},
                {"type": "blob", "path": "skills/example-power/SKILL.md", "sha": "abc123"},
            ]
        }
        with patch(
            "cccc.daemon.ops.capability_ops._http_get_json",
            return_value=tree_payload,
        ), patch("cccc.daemon.ops.capability_ops.urlopen", return_value=_Resp(skill_md)):
            upserted = ops._sync_superpowers_skills_source(catalog, force=True)

        self.assertEqual(upserted, 1)
        record = catalog.get("records", {}).get("skill:superpowers:example-power")
        self.assertIsInstance(record, dict)
        self.assertEqual(record.get("source_id"), "superpowers_skills")
        self.assertEqual(record.get("qualification_status"), "qualified")
        self.assertIn("Source: https://raw.githubusercontent.com/obra/superpowers/main/skills/example-power/SKILL.md", str(record.get("capsule_text") or ""))
        source_state = catalog.get("sources", {}).get("superpowers_skills", {})
        self.assertEqual(source_state.get("sync_state"), "fresh")

    def test_auto_sync_respects_source_enable_flags(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        catalog = ops._new_catalog_doc()
        with patch.dict(
            os.environ,
            {
                "CCCC_CAPABILITY_SOURCE_MCP_REGISTRY_ENABLED": "1",
                "CCCC_CAPABILITY_SOURCE_ANTHROPIC_SKILLS_ENABLED": "0",
                "CCCC_CAPABILITY_SOURCE_SUPERPOWERS_SKILLS_ENABLED": "0",
            },
            clear=False,
        ), patch("cccc.daemon.ops.capability_ops._sync_mcp_registry_source", return_value=0), patch(
            "cccc.daemon.ops.capability_ops._sync_anthropic_skills_source",
            side_effect=AssertionError("anthropic source should be disabled"),
        ), patch(
            "cccc.daemon.ops.capability_ops._sync_superpowers_skills_source",
            side_effect=AssertionError("superpowers source should be disabled"),
        ):
            changed = ops._auto_sync_catalog(catalog)

        self.assertTrue(changed)
        sources = catalog.get("sources", {})
        anthropic = sources.get("anthropic_skills", {})
        self.assertEqual(anthropic.get("sync_state"), "disabled")
        self.assertEqual(anthropic.get("error"), "source_disabled_by_policy")
        superpowers = sources.get("superpowers_skills", {})
        self.assertEqual(superpowers.get("sync_state"), "disabled")
        self.assertEqual(superpowers.get("error"), "source_disabled_by_policy")

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
            self.assertEqual(str(result.get("state") or ""), "activation_pending")
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
            self.assertEqual(str(result.get("state") or ""), "activation_pending")
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
                state="activation_pending",
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
                state="activation_pending",
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
                state="activation_pending",
                last_error="",
            )
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid,
                actor_id="peer-1",
                capability_id="mcp:test-server-b",
                artifact_id=artifact_b,
                state="activation_pending",
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
                state="activation_pending",
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
            self.assertEqual(str(result.get("state") or ""), "blocked")
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
            self.assertEqual(str(result.get("state") or ""), "blocked")
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
                state="activation_pending",
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
                state="activation_pending",
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

    def test_search_skill_query_multiple_names_returns_union_matches(self) -> None:
        _, cleanup = self._with_home()
        try:
            self._write_allowlist_override(
                extra=(
                    "skills:\n"
                    "  source_overrides:\n"
                    "    - source_id: github_skills_curated\n"
                    "      level: mounted\n"
                    "  curated:\n"
                    "    - capability_id: skill:github:demo:quant-analyst\n"
                    "      level: mounted\n"
                    "      source_id: github_skills_curated\n"
                    "      source_uri: https://github.com/demo/quant-analyst\n"
                    "      qualification_status: qualified\n"
                    "      description_short: Quant analyst toolkit.\n"
                    "    - capability_id: skill:github:demo:search-specialist\n"
                    "      level: mounted\n"
                    "      source_id: github_skills_curated\n"
                    "      source_uri: https://github.com/demo/search-specialist\n"
                    "      qualification_status: qualified\n"
                    "      description_short: Search specialist toolkit.\n"
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
                    "query": "quant-analyst search-specialist",
                    "kind": "skill",
                    "include_external": True,
                    "limit": 20,
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            result = search_resp.result if isinstance(search_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            ids = {str(item.get("capability_id") or "") for item in items if isinstance(item, dict)}
            self.assertIn("skill:github:demo:quant-analyst", ids)
            self.assertIn("skill:github:demo:search-specialist", ids)
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
            readiness = item.get("readiness_preview") if isinstance(item.get("readiness_preview"), dict) else {}
            self.assertEqual(str(readiness.get("preview_status") or ""), "enableable")

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
            self.assertEqual(str(enable_result.get("state") or ""), "runnable")
        finally:
            cleanup()

    def test_default_policy_surfaces_risky_third_party_mcp_not_hidden(self) -> None:
        """Risky third-party MCPs (desktop-commander) are indexed by default policy,
        so they are hidden from normal search results (policy_hidden_count > 0)."""
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
            # desktop-commander is indexed in default allowlist → hidden from search
            self.assertIsNone(row)
            diag = result.get("search_diagnostics") if isinstance(result.get("search_diagnostics"), dict) else {}
            self.assertGreater(int(diag.get("policy_hidden_count") or 0), 0)
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
            self.assertEqual(str(result.get("state") or ""), "blocked")
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

    def test_ensure_curated_catalog_records_includes_builtin_runtime_bootstrap_skill(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        catalog = ops._new_catalog_doc()
        policy = ops._compile_allowlist_policy({})
        changed = ops._ensure_curated_catalog_records(catalog, policy=policy)
        self.assertTrue(changed)

        rec = catalog.get("records", {}).get("skill:cccc:runtime-bootstrap")
        self.assertIsInstance(rec, dict)
        self.assertEqual(str((rec or {}).get("source_id") or ""), "cccc_builtin")
        self.assertEqual(str((rec or {}).get("kind") or ""), "skill")
        requires = (rec or {}).get("requires_capabilities") if isinstance(rec, dict) else []
        self.assertEqual(requires, ["pack:diagnostics", "pack:group-runtime"])

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
            rows_by_id = {
                str(item.get("capability_id") or ""): item
                for item in hidden
                if isinstance(item, dict)
            }
            manual_row = rows_by_id.get("mcp:manual") if isinstance(rows_by_id.get("mcp:manual"), dict) else {}
            self.assertEqual(str(manual_row.get("name") or ""), "manual")
            self.assertEqual(str(manual_row.get("description_short") or ""), "manual")
            self.assertEqual(str(manual_row.get("kind") or ""), "mcp_toolpack")
            self.assertEqual(str(manual_row.get("source_id") or ""), "anthropic_skills")
            shared_row = rows_by_id.get("mcp:shared") if isinstance(rows_by_id.get("mcp:shared"), dict) else {}
            self.assertEqual(str(shared_row.get("name") or ""), "shared")
            self.assertEqual(str(shared_row.get("description_short") or ""), "shared")
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
                self.assertEqual(str(first_result.get("state") or ""), "activation_pending")

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
            self.assertEqual(str(second_result.get("state") or ""), "blocked")
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
                state="activation_pending",
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
                state="activation_pending",
                last_error="",
            )
            ops._set_runtime_actor_binding(
                runtime_doc,
                group_id=gid_b,
                actor_id="peer-1",
                capability_id="mcp:test-server",
                artifact_id=artifact_id,
                state="activation_pending",
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
                state="activation_pending",
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
                state="activation_pending",
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
            active_capsule_skills = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_capsule_skills if isinstance(item, dict)}
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

    def test_builtin_runtime_bootstrap_enable_applies_builtin_dependencies(self) -> None:
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
                    "capability_id": "skill:cccc:runtime-bootstrap",
                    "scope": "session",
                    "enabled": True,
                },
            )
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))
            enable_result = enable_resp.result if isinstance(enable_resp.result, dict) else {}
            skill_payload = enable_result.get("skill") if isinstance(enable_result.get("skill"), dict) else {}
            self.assertEqual(str(skill_payload.get("capability_id") or ""), "skill:cccc:runtime-bootstrap")
            applied = skill_payload.get("applied_dependencies") if isinstance(skill_payload.get("applied_dependencies"), list) else []
            self.assertEqual(applied, ["pack:diagnostics", "pack:group-runtime"])

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            enabled = set(state.get("enabled_capabilities") or [])
            self.assertIn("skill:cccc:runtime-bootstrap", enabled)
            self.assertIn("pack:diagnostics", enabled)
            self.assertIn("pack:group-runtime", enabled)
            active_capsule_skills = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_capsule_skills if isinstance(item, dict)}
            self.assertIn("skill:cccc:runtime-bootstrap", active_ids)
            active_row = next(
                (
                    item
                    for item in active_capsule_skills
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == "skill:cccc:runtime-bootstrap"
                ),
                {},
            )
            self.assertIn("Restate the exact symptom", str(active_row.get("capsule_preview") or ""))
            self.assertIn("Gather evidence first", str(active_row.get("capsule_preview") or ""))
            visible_tools = set(state.get("visible_tools") or [])
            self.assertIn("cccc_terminal", visible_tools)
            self.assertIn("cccc_actor", visible_tools)
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
            active_capsule_skills = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_capsule_skills if isinstance(item, dict)}
            self.assertIn("skill:anthropic:triage", active_ids)
            active_row = next(
                (
                    item
                    for item in active_capsule_skills
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == "skill:anthropic:triage"
                ),
                {},
            )
            activation_sources = (
                active_row.get("activation_sources")
                if isinstance(active_row.get("activation_sources"), list)
                else []
            )
            self.assertEqual({str(item.get("scope") or "") for item in activation_sources if isinstance(item, dict)}, {"actor"})
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

            autoload_result = ops.apply_actor_capability_autoload(
                group_id=gid,
                actor_id="peer-1",
                autoload_capabilities=["skill:anthropic:triage"],
            )
            self.assertIn("skill:anthropic:triage", autoload_result.get("applied") or [])
            self.assertFalse(autoload_result.get("skipped") or [])

            active_state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(active_state_resp.ok, getattr(active_state_resp, "error", None))
            active_state = active_state_resp.result if isinstance(active_state_resp.result, dict) else {}
            active_capsule_skills = (
                active_state.get("active_capsule_skills")
                if isinstance(active_state.get("active_capsule_skills"), list)
                else []
            )
            active_row = next(
                (
                    item
                    for item in active_capsule_skills
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == "skill:anthropic:triage"
                ),
                {},
            )
            self.assertTrue(active_row)
            activation_sources = (
                active_row.get("activation_sources")
                if isinstance(active_row.get("activation_sources"), list)
                else []
            )
            self.assertEqual({str(item.get("scope") or "") for item in activation_sources if isinstance(item, dict)}, {"actor"})
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
                # Override allowlist so openclaw_skills_remote is mounted (visible in search)
                self._write_allowlist_override(
                    extra="    openclaw_skills_remote: mounted\n",
                )
                ops._POLICY_CACHE.clear()
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
            self.assertEqual(str(result.get("state") or ""), "activation_pending")
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

        supported_alias_js, reason_alias_js = ops._supported_external_install_record(
            {
                "install_mode": "package",
                "install_spec": {
                    "registry_type": "javascript",
                    "identifier": "@example/mcp-server",
                    "runtime_hint": "nodejs",
                },
            }
        )
        self.assertTrue(supported_alias_js, reason_alias_js)
        self.assertEqual(reason_alias_js, "")

        supported_command, reason_command = ops._supported_external_install_record(
            {
                "install_mode": "command",
                "install_spec": {
                    "command_candidates": [["npx", "-y", "@example/mcp-server"]],
                },
            }
        )
        self.assertTrue(supported_command, reason_command)
        self.assertEqual(reason_command, "")

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

    def test_install_external_capability_command_mode_installs(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "command",
            "install_spec": {
                "command_candidates": [["npx", "-y", "@example/mcp-server"]],
            },
        }

        with patch(
            "cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip",
            return_value=[
                {"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "demo"}]}},
            ],
        ) as probe:
            install = ops._install_external_capability(rec, capability_id="mcp:example/command")

        self.assertEqual(str(install.get("state") or ""), "installed")
        self.assertEqual(str(install.get("install_mode") or ""), "command")
        self.assertIn(str(install.get("installer") or ""), {"command_stdio", "command_npx"})
        invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
        self.assertEqual(str(invoker.get("type") or ""), "command_stdio")
        command = invoker.get("command") if isinstance(invoker.get("command"), list) else []
        self.assertTrue(command and str(command[0]) == "npx")
        called_cmd = probe.call_args[0][0]
        self.assertTrue(isinstance(called_cmd, list) and called_cmd and str(called_cmd[0]) == "npx")

    def test_install_external_capability_package_falls_back_to_command_candidates(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "package",
            "install_spec": {
                "registry_type": "mystery-registry",
                "identifier": "",
                "fallback_command_candidates": [["npx", "-y", "@example/mcp-server"]],
            },
        }

        with patch(
            "cccc.daemon.ops.capability_ops._stdio_mcp_roundtrip",
            return_value=[
                {"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "demo"}]}},
            ],
        ):
            install = ops._install_external_capability(rec, capability_id="mcp:example/fallback-command")

        self.assertEqual(str(install.get("state") or ""), "installed")
        self.assertEqual(str(install.get("install_mode") or ""), "command")
        self.assertEqual(str(install.get("fallback_from") or ""), "package")

    def test_preflight_external_install_detects_missing_env_and_binary(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec_missing_env = {
            "install_mode": "command",
            "install_spec": {
                "command_candidates": [["npx", "-y", "@example/mcp-server"]],
                "required_env": ["OPENAI_API_KEY"],
            },
        }
        with patch.dict(os.environ, {"OPENAI_API_KEY": ""}, clear=False):
            preflight_env = ops._preflight_external_install(rec_missing_env, capability_id="mcp:example/preflight-env")
        self.assertFalse(bool(preflight_env.get("ok")))
        self.assertEqual(str(preflight_env.get("code") or ""), "missing_required_env")
        required_env = preflight_env.get("required_env") if isinstance(preflight_env.get("required_env"), list) else []
        self.assertIn("OPENAI_API_KEY", required_env)

        rec_missing_bin = {
            "install_mode": "command",
            "install_spec": {
                "command_candidates": [["definitely-missing-runtime-binary", "--version"]],
            },
        }
        preflight_bin = ops._preflight_external_install(rec_missing_bin, capability_id="mcp:example/preflight-bin")
        self.assertFalse(bool(preflight_bin.get("ok")))
        self.assertEqual(str(preflight_bin.get("code") or ""), "runtime_binary_missing")
        missing_binaries = (
            preflight_bin.get("missing_binaries")
            if isinstance(preflight_bin.get("missing_binaries"), list)
            else []
        )
        self.assertIn("definitely-missing-runtime-binary", missing_binaries)

    def test_preflight_external_install_detects_invalid_remote_url(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        rec = {
            "install_mode": "remote_only",
            "install_spec": {
                "transport": "http",
                "url": "ftp://example.invalid/mcp",
            },
        }
        preflight = ops._preflight_external_install(rec, capability_id="mcp:example/preflight-url")
        self.assertFalse(bool(preflight.get("ok")))
        self.assertEqual(str(preflight.get("code") or ""), "invalid_remote_url")

    def test_capability_enable_returns_preflight_failed_for_missing_runtime_binary(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            catalog_path, catalog_doc = ops._load_catalog_doc()
            catalog_doc["records"]["mcp:example/preflight-fail"] = {
                "capability_id": "mcp:example/preflight-fail",
                "kind": "mcp_toolpack",
                "name": "preflight-fail",
                "description_short": "test preflight fail",
                "source_id": "mcp_registry_official",
                "source_tier": "tier1",
                "trust_tier": "tier1",
                "qualification_status": "qualified",
                "enable_supported": True,
                "install_mode": "command",
                "install_spec": {
                    "command_candidates": [["definitely-missing-runtime-binary", "--version"]],
                },
            }
            ops._save_catalog_doc(catalog_path, catalog_doc)

            resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": "mcp:example/preflight-fail",
                    "scope": "session",
                    "enabled": True,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "blocked")
            self.assertEqual(str(result.get("reason") or ""), "preflight_failed:runtime_binary_missing")
            missing_binaries = result.get("missing_binaries") if isinstance(result.get("missing_binaries"), list) else []
            self.assertIn("definitely-missing-runtime-binary", missing_binaries)
            diagnostics = result.get("diagnostics") if isinstance(result.get("diagnostics"), list) else []
            self.assertTrue(diagnostics)
        finally:
            cleanup()

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
            active_capsule_skills = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_capsule_skills if isinstance(item, dict)}
            self.assertNotIn("skill:anthropic:triage", active_ids)
            autoload_skills = state.get("autoload_skills") if isinstance(state.get("autoload_skills"), list) else []
            autoload_ids = {str(item.get("capability_id") or "") for item in autoload_skills if isinstance(item, dict)}
            self.assertNotIn("skill:anthropic:triage", autoload_ids)
        finally:
            cleanup()

    def test_capability_import_dry_run_does_not_persist_record(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:github:demo:triage"
            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "name": "Demo Triage",
                        "capsule_text": "Use triage checklist",
                    },
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertTrue(bool(result.get("dry_run")))
            self.assertFalse(bool(result.get("imported")))
            self.assertEqual(str(result.get("state") or ""), "enableable")
            self.assertEqual(str(result.get("capability_id") or ""), capability_id)

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            self.assertNotIn(capability_id, rows)
        finally:
            cleanup()

    def test_capability_import_dry_run_normalizes_source_and_reports_policy_enableable(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": "mcp:example/manual-import",
                        "kind": "mcp_toolpack",
                        "source_id": "github:random/repo",
                        "install_mode": "command",
                        "install_spec": {
                            "command": ["uvx", "demo-mcp"],
                        },
                    },
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            record = result.get("record") if isinstance(result.get("record"), dict) else {}
            self.assertEqual(str(record.get("source_id") or ""), "manual_import")
            self.assertEqual(str(result.get("effective_policy_level") or ""), "mounted")
            self.assertTrue(bool(result.get("enableable_now")))
            self.assertEqual(str(result.get("enable_block_reason") or ""), "")
            readiness = result.get("readiness_preview") if isinstance(result.get("readiness_preview"), dict) else {}
            self.assertEqual(str(readiness.get("preview_status") or ""), "enableable")
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_skill_source_is_narrowly_enableable(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": "skill:agent_self_proposed:repro-triage",
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Repro Triage",
                        "description_short": "Reusable repro-first triage checklist",
                        "capsule_text": (
                            "Skill: Repro Triage\n"
                            "When to use:\n"
                            "- Recurring debugging work after a stable repro path was found.\n"
                            "Avoid when:\n"
                            "- One-off status reporting.\n"
                            "Procedure:\n"
                            "1. Capture the failing trigger.\n"
                            "Pitfalls:\n"
                            "- Do not generalize one lucky workaround.\n"
                            "Verification:\n"
                            "- Re-run the same repro case."
                        ),
                    },
                },
            )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            record = result.get("record") if isinstance(result.get("record"), dict) else {}
            self.assertEqual(str(record.get("source_id") or ""), "agent_self_proposed")
            self.assertEqual(str(result.get("effective_policy_level") or ""), "mounted")
            self.assertTrue(bool(result.get("enableable_now")))
            readiness = result.get("readiness_preview") if isinstance(result.get("readiness_preview"), dict) else {}
            self.assertEqual(str(readiness.get("preview_status") or ""), "enableable")
        finally:
            cleanup()

    def test_self_evolution_curated_skills_have_actionable_capsules(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            policy = ops._allowlist_policy()
            records = ops._build_curated_records_from_policy(policy)
            for capability_id in [
                "skill:anthropic:skill-creator",
                "skill:github:blader:claudeception",
            ]:
                record = records.get(capability_id) if isinstance(records.get(capability_id), dict) else {}
                capsule_text = str(record.get("capsule_text") or "")
                self.assertIn("When to use:", capsule_text)
                self.assertIn("Avoid when:", capsule_text)
                self.assertIn("Procedure:", capsule_text)
                self.assertIn("Pitfalls:", capsule_text)
                self.assertIn("Verification:", capsule_text)
                self.assertIn("skill:agent_self_proposed:", capsule_text)
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_skill_persists_enables_and_is_visible(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:agent_self_proposed:self-evolution-smoke"

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "user",
                    "enable_after_import": True,
                    "scope": "session",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Self Evolution Smoke",
                        "description_short": "Smoke-test skill for autonomous proposal visibility.",
                        "capsule_text": (
                            "Skill: Self Evolution Smoke\n"
                            "When to use:\n"
                            "- Verify that self-proposed skill proposals are visible and activatable.\n"
                            "Avoid when:\n"
                            "- Production workflow execution.\n"
                            "Procedure:\n"
                            "1. Import and enable the proposed skill.\n"
                            "Pitfalls:\n"
                            "- Do not confuse catalog presence with runtime activation.\n"
                            "Verification:\n"
                            "- Re-read capability_state.active_capsule_skills."
                        ),
                    },
                },
            )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertIn(str(result.get("state") or ""), {"activation_pending", "runnable"})
            self.assertEqual(str(result.get("scope") or ""), "session")
            self.assertEqual(str(result.get("effective_policy_level") or ""), "mounted")
            self.assertTrue(bool(result.get("enableable_now")))
            enable_result = result.get("enable_result") if isinstance(result.get("enable_result"), dict) else {}
            self.assertEqual(str(enable_result.get("scope") or ""), "session")

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "user", "by": "user"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            enabled = set(state.get("enabled_capabilities") or [])
            self.assertIn(capability_id, enabled)
            active_capsule_skills = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_rows = [item for item in active_capsule_skills if isinstance(item, dict)]
            active_ids = {str(item.get("capability_id") or "") for item in active_rows}
            self.assertIn(capability_id, active_ids)
            active_row = next(item for item in active_rows if str(item.get("capability_id") or "") == capability_id)
            self.assertEqual(str(active_row.get("source_id") or ""), "agent_self_proposed")

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            record = rows.get(capability_id) if isinstance(rows.get(capability_id), dict) else {}
            self.assertEqual(str(record.get("source_id") or ""), "agent_self_proposed")
            self.assertEqual(str(record.get("origin_group_id") or ""), gid)
            self.assertEqual(str(record.get("qualification_status") or ""), "qualified")

            overview_resp, _ = self._call(
                "capability_overview",
                {
                    "query": capability_id,
                    "limit": 50,
                    "include_indexed": True,
                },
            )
            self.assertTrue(overview_resp.ok, getattr(overview_resp, "error", None))
            overview_result = overview_resp.result if isinstance(overview_resp.result, dict) else {}
            overview_items = overview_result.get("items") if isinstance(overview_result.get("items"), list) else []
            overview_row = next(
                (
                    item
                    for item in overview_items
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            self.assertTrue(overview_row)
            self.assertEqual(str(overview_row.get("source_id") or ""), "agent_self_proposed")
            self.assertEqual(str(overview_row.get("source_record_id") or ""), capability_id)
            self.assertEqual(str(overview_row.get("origin_group_id") or ""), gid)
            self.assertTrue(str(overview_row.get("updated_at_source") or "").strip())
            self.assertTrue(str(overview_row.get("last_synced_at") or "").strip())
            self.assertIn("Self Evolution Smoke", str(overview_row.get("capsule_text") or ""))
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_reimport_reports_update_and_active_binding(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:agent_self_proposed:self-evolution-active-update"

            def _capsule(version: str, procedure: str) -> str:
                return (
                    "Skill: Self Evolution Active Update\n"
                    "When to use:\n"
                    f"- Maintain an active self-proposed skill after {version} evidence.\n"
                    "Avoid when:\n"
                    "- The skill is not already scoped to the current actor.\n"
                    "Procedure:\n"
                    f"1. {procedure}\n"
                    "Pitfalls:\n"
                    "- Use import_action for create/update/unchanged state.\n"
                    "Verification:\n"
                    "- Re-read capability_state.active_capsule_skills."
                )

            first_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "enable_after_import": True,
                    "scope": "session",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Self Evolution Active Update",
                        "description_short": "initial active update guidance",
                        "capsule_text": _capsule("initial", "Capture the first proven active path."),
                    },
                },
            )
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))
            first = first_resp.result if isinstance(first_resp.result, dict) else {}
            self.assertEqual(str(first.get("import_action") or ""), "created")
            self.assertEqual(str(first.get("scope") or ""), "session")
            self.assertFalse(bool(first.get("record_changed")))
            self.assertFalse(bool(first.get("already_active")))
            self.assertTrue(bool(first.get("active_after_import")))
            self.assertNotIn("deduped", first)
            self.assertNotIn("record_existed", first)
            self.assertIn(str(first.get("state") or ""), {"activation_pending", "runnable"})

            second_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Self Evolution Active Update",
                        "description_short": "revised active update guidance",
                        "capsule_text": _capsule("revised", "Patch the active capability_id in place."),
                    },
                },
            )
            self.assertTrue(second_resp.ok, getattr(second_resp, "error", None))
            second = second_resp.result if isinstance(second_resp.result, dict) else {}
            self.assertEqual(str(second.get("import_action") or ""), "updated")
            self.assertEqual(str(second.get("scope") or ""), "session")
            self.assertTrue(bool(second.get("record_changed")))
            self.assertTrue(bool(second.get("already_active")))
            self.assertTrue(bool(second.get("active_after_import")))
            self.assertNotIn("deduped", second)
            self.assertNotIn("record_existed", second)
            self.assertEqual(str(second.get("state") or ""), "runnable")
            second_readiness = second.get("readiness_preview") if isinstance(second.get("readiness_preview"), dict) else {}
            self.assertEqual(str(second_readiness.get("preview_status") or ""), "active")
            self.assertEqual(str(second_readiness.get("next_step") or ""), "none")
            self.assertTrue(bool(second_readiness.get("already_active")))
            second_record = second.get("record") if isinstance(second.get("record"), dict) else {}

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            active_rows = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_row = next(
                (
                    item
                    for item in active_rows
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            self.assertIn("Patch the active capability_id in place.", str(active_row.get("capsule_text") or ""))
            self.assertIn("Verification:", str(active_row.get("capsule_text") or ""))

            search_resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "query": capability_id,
                    "include_external": True,
                    "limit": 20,
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            search_result = search_resp.result if isinstance(search_resp.result, dict) else {}
            search_items = search_result.get("items") if isinstance(search_result.get("items"), list) else []
            search_row = next(
                (
                    item
                    for item in search_items
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            self.assertEqual(str(search_row.get("enable_hint") or ""), "active")
            search_readiness = (
                search_row.get("readiness_preview") if isinstance(search_row.get("readiness_preview"), dict) else {}
            )
            self.assertEqual(str(search_readiness.get("preview_status") or ""), "active")
            self.assertEqual(str(search_readiness.get("next_step") or ""), "none")
            self.assertTrue(bool(search_readiness.get("already_active")))

            third_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Self Evolution Active Update",
                        "description_short": "revised active update guidance",
                        "capsule_text": _capsule("revised", "Patch the active capability_id in place."),
                    },
                },
            )
            self.assertTrue(third_resp.ok, getattr(third_resp, "error", None))
            third = third_resp.result if isinstance(third_resp.result, dict) else {}
            self.assertEqual(str(third.get("import_action") or ""), "unchanged")
            self.assertEqual(str(third.get("scope") or ""), "session")
            self.assertFalse(bool(third.get("record_changed")))
            self.assertTrue(bool(third.get("already_active")))
            self.assertTrue(bool(third.get("active_after_import")))
            self.assertNotIn("deduped", third)
            self.assertNotIn("record_existed", third)
            self.assertEqual(str(third.get("state") or ""), "runnable")
            third_readiness = third.get("readiness_preview") if isinstance(third.get("readiness_preview"), dict) else {}
            self.assertEqual(str(third_readiness.get("preview_status") or ""), "active")
            self.assertEqual(str(third_readiness.get("next_step") or ""), "none")
            self.assertTrue(bool(third_readiness.get("already_active")))
            third_record = third.get("record") if isinstance(third.get("record"), dict) else {}
            self.assertEqual(
                str(third_record.get("last_synced_at") or ""),
                str(second_record.get("last_synced_at") or ""),
            )
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_invalid_update_preserves_active_record(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:agent_self_proposed:self-evolution-invalid-update"
            initial_capsule = (
                "Skill: Self Evolution Invalid Update\n"
                "When to use:\n"
                "- Preserve the last valid skill when a malformed update arrives.\n"
                "Avoid when:\n"
                "- The update already passes the required capsule template.\n"
                "Procedure:\n"
                "1. Keep the active record intact.\n"
                "Pitfalls:\n"
                "- Do not let an invalid update remove a live capsule.\n"
                "Verification:\n"
                "- Re-read active_capsule_skills and the catalog row."
            )
            invalid_capsule = (
                "Skill: Self Evolution Invalid Update\n"
                "When to use:\n"
                "- This malformed update intentionally omits one required section.\n"
                "Avoid when:\n"
                "- The existing active record is more complete.\n"
                "Procedure:\n"
                "1. This should be rejected before it overwrites catalog state.\n"
                "Verification:\n"
                "- This should never replace the active record."
            )

            first_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "enable_after_import": True,
                    "scope": "session",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Self Evolution Invalid Update",
                        "description_short": "initial valid capsule",
                        "capsule_text": initial_capsule,
                    },
                },
            )
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))
            first = first_resp.result if isinstance(first_resp.result, dict) else {}
            self.assertTrue(bool(first.get("active_after_import")))

            invalid_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Self Evolution Invalid Update",
                        "description_short": "malformed update",
                        "capsule_text": invalid_capsule,
                    },
                },
            )
            self.assertFalse(invalid_resp.ok)
            self.assertEqual(getattr(invalid_resp.error, "code", ""), "capability_import_invalid")
            details = invalid_resp.error.details if invalid_resp.error is not None else {}
            self.assertTrue(bool(details.get("active_record_preserved")))
            self.assertTrue(bool(details.get("already_active")))
            self.assertIn("pitfalls", details.get("missing_sections") or [])

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            active_rows = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_row = next(
                (
                    item
                    for item in active_rows
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            self.assertIn("Keep the active record intact.", str(active_row.get("capsule_text") or ""))
            self.assertNotIn("This should be rejected", str(active_row.get("capsule_text") or ""))

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            record = rows.get(capability_id) if isinstance(rows.get(capability_id), dict) else {}
            self.assertEqual(str(record.get("qualification_status") or ""), "qualified")
            self.assertIn("Keep the active record intact.", str(record.get("capsule_text") or ""))
            self.assertNotIn("This should be rejected", str(record.get("capsule_text") or ""))
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_skill_reimport_updates_same_record(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:agent_self_proposed:self-evolution-maintenance"

            def _capsule(version: str, procedure: str) -> str:
                return (
                    "Skill: Self Evolution Maintenance\n"
                    "When to use:\n"
                    f"- Maintain a self-proposed skill after {version} evidence.\n"
                    "Avoid when:\n"
                    "- The existing skill belongs to an external curated source.\n"
                    "Procedure:\n"
                    f"1. {procedure}\n"
                    "Pitfalls:\n"
                    "- Do not create a near-duplicate for the same workflow.\n"
                    "Verification:\n"
                    "- Re-read the catalog record by capability_id."
                )

            for version, procedure in [
                ("initial", "Capture the first proven maintenance path."),
                ("revised", "Patch the existing capability_id with revised capsule_text."),
            ]:
                resp, _ = self._call(
                    "capability_import",
                    {
                        "group_id": gid,
                        "by": "peer-1",
                        "actor_id": "peer-1",
                        "probe": False,
                        "record": {
                            "capability_id": capability_id,
                            "kind": "skill",
                            "source_id": "agent_self_proposed",
                            "name": "Self Evolution Maintenance",
                            "description_short": f"{version} maintenance guidance",
                            "capsule_text": _capsule(version, procedure),
                        },
                    },
                )
                self.assertTrue(resp.ok, getattr(resp, "error", None))

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            matching_ids = [cap_id for cap_id in rows.keys() if str(cap_id or "") == capability_id]
            self.assertEqual(matching_ids, [capability_id])

            record = rows.get(capability_id) if isinstance(rows.get(capability_id), dict) else {}
            self.assertEqual(str(record.get("source_id") or ""), "agent_self_proposed")
            self.assertEqual(str(record.get("description_short") or ""), "revised maintenance guidance")
            capsule_text = str(record.get("capsule_text") or "")
            self.assertIn("Patch the existing capability_id with revised capsule_text.", capsule_text)
            self.assertNotIn("Capture the first proven maintenance path.", capsule_text)
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_reimport_preserves_origin_group(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            origin_gid = self._create_group()
            other_gid = self._create_group()
            self._add_actor(origin_gid, "peer-1", by="user")
            self._add_actor(other_gid, "peer-2", by="user")
            capability_id = "skill:agent_self_proposed:origin-group-preserved"

            def _capsule(version: str) -> str:
                return (
                    "Skill: Origin Group Preserved\n"
                    "When to use:\n"
                    f"- Track origin group metadata during {version} imports.\n"
                    "Avoid when:\n"
                    "- The record is not self-proposed.\n"
                    "Procedure:\n"
                    "1. Re-import the same capability id from another group.\n"
                    "Pitfalls:\n"
                    "- Do not silently reassign the origin group on edit.\n"
                    "Verification:\n"
                    "- Re-read the catalog record and overview row."
                )

            first_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": origin_gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Origin Group Preserved",
                        "description_short": "initial origin group",
                        "origin_group_id": other_gid,
                        "capsule_text": _capsule("initial"),
                    },
                },
            )
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))

            second_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": other_gid,
                    "by": "peer-2",
                    "actor_id": "peer-2",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Origin Group Preserved",
                        "description_short": "updated elsewhere",
                        "origin_group_id": other_gid,
                        "capsule_text": _capsule("cross-group update"),
                    },
                },
            )
            self.assertTrue(second_resp.ok, getattr(second_resp, "error", None))
            second = second_resp.result if isinstance(second_resp.result, dict) else {}
            second_record = second.get("record") if isinstance(second.get("record"), dict) else {}
            self.assertEqual(str(second_record.get("origin_group_id") or ""), origin_gid)

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            record = rows.get(capability_id) if isinstance(rows.get(capability_id), dict) else {}
            self.assertEqual(str(record.get("origin_group_id") or ""), origin_gid)

            overview_resp, _ = self._call(
                "capability_overview",
                {
                    "query": capability_id,
                    "limit": 200,
                    "include_indexed": True,
                },
            )
            self.assertTrue(overview_resp.ok, getattr(overview_resp, "error", None))
            overview = overview_resp.result if isinstance(overview_resp.result, dict) else {}
            rows = overview.get("items") if isinstance(overview.get("items"), list) else []
            row = next(
                (
                    item
                    for item in rows
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            self.assertTrue(row)
            self.assertEqual(str(row.get("origin_group_id") or ""), origin_gid)
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_skill_requires_template_sections(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": "skill:agent_self_proposed:thin-note",
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Thin Note",
                        "capsule_text": "Use a triage checklist.",
                    },
                },
            )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            record = result.get("record") if isinstance(result.get("record"), dict) else {}
            self.assertEqual(str(record.get("qualification_status") or ""), "blocked")
            reasons = record.get("qualification_reasons") if isinstance(record.get("qualification_reasons"), list) else []
            self.assertTrue(any("missing_agent_self_proposed_sections" in str(item) for item in reasons))
            self.assertFalse(bool(result.get("enableable_now")))
            self.assertEqual(str(result.get("enable_block_reason") or ""), "qualification_blocked")
            readiness = result.get("readiness_preview") if isinstance(result.get("readiness_preview"), dict) else {}
            self.assertEqual(str(readiness.get("preview_status") or ""), "blocked")
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_skill_requires_canonical_namespace(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": "skill:github:example:self-proposed-collision",
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Self Proposed Collision",
                        "capsule_text": (
                            "Skill: Self Proposed Collision\n"
                            "When to use:\n"
                            "- Verify namespace collision protection.\n"
                            "Avoid when:\n"
                            "- Writing curated namespace records.\n"
                            "Procedure:\n"
                            "1. Try to import a self-proposed skill under a curated namespace.\n"
                            "Pitfalls:\n"
                            "- A valid capsule body must not bypass namespace protection.\n"
                            "Verification:\n"
                            "- The import is rejected before catalog persistence."
                        ),
                    },
                },
            )

            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code if resp.error else "", "capability_import_invalid")
            self.assertIn("skill:agent_self_proposed:", resp.error.message if resp.error else "")
        finally:
            cleanup()

    def test_legacy_agent_self_proposed_skill_namespace_is_not_enableable(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:agent:legacy-self-proposed"

            path, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            rows[capability_id] = {
                "capability_id": capability_id,
                "kind": "skill",
                "source_id": "agent_self_proposed",
                "name": "Legacy Self Proposed",
                "description_short": "Legacy non-canonical self-proposed skill.",
                "qualification_status": "qualified",
                "enable_supported": True,
                "capsule_text": (
                    "Skill: Legacy Self Proposed\n"
                    "When to use:\n"
                    "- Verify legacy namespace rejection.\n"
                    "Avoid when:\n"
                    "- Enabling current self-proposed skills.\n"
                    "Procedure:\n"
                    "1. Try to enable the legacy capability id.\n"
                    "Pitfalls:\n"
                    "- Do not carry old skill:agent: ids forward.\n"
                    "Verification:\n"
                    "- Enable returns capability_unavailable."
                ),
            }
            catalog_doc["records"] = rows
            ops._save_catalog_doc(path, catalog_doc)
            _, normalized_catalog = ops._load_catalog_doc()
            normalized_rows = (
                normalized_catalog.get("records") if isinstance(normalized_catalog.get("records"), dict) else {}
            )
            normalized_row = (
                normalized_rows.get(capability_id)
                if isinstance(normalized_rows.get(capability_id), dict)
                else {}
            )
            self.assertFalse(bool(normalized_row.get("enable_supported")))
            self.assertEqual(str(normalized_row.get("qualification_status") or ""), "blocked")
            qualification_reasons = (
                normalized_row.get("qualification_reasons")
                if isinstance(normalized_row.get("qualification_reasons"), list)
                else []
            )
            self.assertIn("legacy_agent_self_proposed_namespace", qualification_reasons)

            runtime_path, runtime_doc = ops._load_runtime_doc()
            ops._record_runtime_recent_success(
                runtime_doc,
                capability_id=capability_id,
                group_id=gid,
                actor_id="peer-1",
                action="enable",
            )
            ops._save_runtime_doc(runtime_path, runtime_doc)

            overview_resp, _ = self._call(
                "capability_overview",
                {
                    "query": capability_id,
                    "limit": 10,
                    "include_indexed": True,
                },
            )
            self.assertTrue(overview_resp.ok, getattr(overview_resp, "error", None))
            overview = overview_resp.result if isinstance(overview_resp.result, dict) else {}
            items = overview.get("items") if isinstance(overview.get("items"), list) else []
            row = next(
                (
                    item
                    for item in items
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            self.assertFalse(bool(row.get("enable_supported")))
            readiness = row.get("readiness_preview") if isinstance(row.get("readiness_preview"), dict) else {}
            self.assertEqual(str(readiness.get("preview_status") or ""), "blocked")
            self.assertEqual(
                str(readiness.get("enable_block_reason") or ""),
                "legacy_agent_self_proposed_namespace",
            )
            self.assertEqual(
                str(readiness.get("next_step") or ""),
                "reimport_under_canonical_self_proposed_id",
            )
            self.assertEqual(str(readiness.get("canonical_prefix") or ""), "skill:agent_self_proposed:")
            self.assertIn("recent_success", readiness)

            actor_update, _ = self._call(
                "actor_update",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "user",
                    "patch": {"capability_autoload": [capability_id]},
                },
            )
            self.assertTrue(actor_update.ok, getattr(actor_update, "error", None))

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
            self.assertIn(capability_id, actor_autoload)
            autoload_skills = state.get("autoload_skills") if isinstance(state.get("autoload_skills"), list) else []
            autoload_ids = {str(item.get("capability_id") or "") for item in autoload_skills if isinstance(item, dict)}
            self.assertNotIn(capability_id, autoload_ids)
            active_skills = (
                state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            )
            active_ids = {str(item.get("capability_id") or "") for item in active_skills if isinstance(item, dict)}
            self.assertNotIn(capability_id, active_ids)
            hidden_rows = state.get("hidden_capabilities") if isinstance(state.get("hidden_capabilities"), list) else []
            hidden = next(
                (
                    item
                    for item in hidden_rows
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            self.assertEqual(str(hidden.get("reason") or ""), "legacy_agent_self_proposed_namespace")
            self.assertEqual(str(hidden.get("name") or ""), "Legacy Self Proposed")
            self.assertEqual(str(hidden.get("description_short") or ""), "Legacy non-canonical self-proposed skill.")
            self.assertEqual(str(hidden.get("kind") or ""), "skill")
            self.assertEqual(str(hidden.get("source_id") or ""), "agent_self_proposed")

            enable_resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "capability_id": capability_id,
                    "scope": "session",
                    "enabled": True,
                },
            )
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))
            enable_result = enable_resp.result if isinstance(enable_resp.result, dict) else {}
            self.assertEqual(str(enable_result.get("state") or ""), "blocked")
            self.assertEqual(str(enable_result.get("reason") or ""), "legacy_agent_self_proposed_namespace")
            diagnostics = (
                enable_result.get("diagnostics")
                if isinstance(enable_result.get("diagnostics"), list)
                else []
            )
            codes = {str(item.get("code") or "") for item in diagnostics if isinstance(item, dict)}
            self.assertIn("legacy_agent_self_proposed_namespace", codes)
            hints = {
                str(hint or "")
                for item in diagnostics
                if isinstance(item, dict)
                for hint in (item.get("action_hints") if isinstance(item.get("action_hints"), list) else [])
            }
            self.assertIn("reimport_the_capsule_under_skill_agent_self_proposed_stable_slug", hints)
            self.assertIn("call_cccc_capability_uninstall_on_the_legacy_capability_id_after_migration", hints)

            uninstall_resp, _ = self._call(
                "capability_uninstall",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "peer-1",
                    "capability_id": capability_id,
                    "reason": "cleanup legacy self-proposed skill",
                },
            )
            self.assertTrue(uninstall_resp.ok, getattr(uninstall_resp, "error", None))
            uninstall_result = uninstall_resp.result if isinstance(uninstall_resp.result, dict) else {}
            self.assertTrue(bool(uninstall_result.get("removed_record")))
            self.assertGreaterEqual(int(uninstall_result.get("removed_actor_autoload") or 0), 1)

            _, catalog_after = ops._load_catalog_doc()
            rows_after = catalog_after.get("records") if isinstance(catalog_after.get("records"), dict) else {}
            self.assertNotIn(capability_id, rows_after)

            actor_list, _ = self._call("actor_list", {"group_id": gid, "by": "user"})
            self.assertTrue(actor_list.ok, getattr(actor_list, "error", None))
            actors = actor_list.result.get("actors") if isinstance(actor_list.result, dict) else []
            peer = next(
                (item for item in actors if isinstance(item, dict) and str(item.get("id") or "") == "peer-1"),
                {},
            )
            self.assertNotIn(capability_id, peer.get("capability_autoload") or [])
        finally:
            cleanup()

    def test_capability_uninstall_self_proposed_skill_removes_record_and_references(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:agent_self_proposed:delete-cleanly"
            capsule_text = (
                "Skill: Remove Cleanly\n"
                "When to use:\n"
                "- Verify self-proposed skill removal.\n"
                "Avoid when:\n"
                "- Deleting curated skills.\n"
                "Procedure:\n"
                "1. Uninstall the self-proposed skill and clean references.\n"
                "Pitfalls:\n"
                "- Catalog deletion without autoload cleanup leaves stale references.\n"
                "Verification:\n"
                "- Re-read catalog, actor autoload, profile defaults, and capability state."
            )
            import_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "enable_after_import": True,
                    "scope": "session",
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Delete Cleanly",
                        "description_short": "delete cleanup validation",
                        "capsule_text": capsule_text,
                    },
                },
            )
            self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))

            actor_update, _ = self._call(
                "actor_update",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "user",
                    "patch": {"capability_autoload": [capability_id, "pack:space"]},
                },
            )
            self.assertTrue(actor_update.ok, getattr(actor_update, "error", None))

            profile_upsert, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "profile": {
                        "name": "Delete Cleanup Profile",
                        "runtime": "codex",
                        "runner": "headless",
                        "command": [],
                        "submit": "enter",
                        "capability_defaults": {
                            "autoload_capabilities": [capability_id, "pack:space"],
                            "default_scope": "actor",
                        },
                    },
                },
            )
            self.assertTrue(profile_upsert.ok, getattr(profile_upsert, "error", None))
            profile = (profile_upsert.result or {}).get("profile") if isinstance(profile_upsert.result, dict) else {}
            profile_id = str(profile.get("id") or "").strip() if isinstance(profile, dict) else ""
            self.assertTrue(profile_id)

            uninstall_resp, _ = self._call(
                "capability_uninstall",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "peer-1",
                    "capability_id": capability_id,
                    "reason": "user removed generated skill",
                },
            )
            self.assertTrue(uninstall_resp.ok, getattr(uninstall_resp, "error", None))
            uninstall_result = uninstall_resp.result if isinstance(uninstall_resp.result, dict) else {}
            self.assertTrue(bool(uninstall_result.get("removed_record")))
            self.assertGreaterEqual(int(uninstall_result.get("removed_bindings") or 0), 1)
            self.assertGreaterEqual(int(uninstall_result.get("removed_actor_autoload") or 0), 1)
            self.assertGreaterEqual(int(uninstall_result.get("removed_profile_autoload") or 0), 1)

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            self.assertNotIn(capability_id, rows)

            actor_list, _ = self._call("actor_list", {"group_id": gid, "by": "user"})
            self.assertTrue(actor_list.ok, getattr(actor_list, "error", None))
            actors = actor_list.result.get("actors") if isinstance(actor_list.result, dict) else []
            peer = next(
                (item for item in actors if isinstance(item, dict) and str(item.get("id") or "") == "peer-1"),
                {},
            )
            self.assertNotIn(capability_id, peer.get("capability_autoload") or [])
            self.assertIn("pack:space", peer.get("capability_autoload") or [])

            profile_get, _ = self._call("actor_profile_get", {"by": "user", "profile_id": profile_id})
            self.assertTrue(profile_get.ok, getattr(profile_get, "error", None))
            profile_after = (profile_get.result or {}).get("profile") if isinstance(profile_get.result, dict) else {}
            defaults = profile_after.get("capability_defaults") if isinstance(profile_after, dict) else {}
            profile_autoload = defaults.get("autoload_capabilities") if isinstance(defaults, dict) else []
            self.assertNotIn(capability_id, profile_autoload)
            self.assertIn("pack:space", profile_autoload)

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            self.assertNotIn(capability_id, state.get("enabled_capabilities") or [])
            active_rows = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_rows if isinstance(item, dict)}
            self.assertNotIn(capability_id, active_ids)
        finally:
            cleanup()

    def test_capability_state_reports_capability_usage_summary(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            self._add_actor(gid, "peer-2", by="user")
            capability_id = "skill:agent_self_proposed:usage-summary"
            capsule_text = (
                "Skill: Usage Summary\n"
                "When to use:\n"
                "- Verify current-use display for generated skills.\n"
                "Avoid when:\n"
                "- The skill is not self-proposed.\n"
                "Procedure:\n"
                "1. Enable the skill at group, actor, session, and autoload scopes.\n"
                "Pitfalls:\n"
                "- A settings UI must not confuse current state with next action.\n"
                "Verification:\n"
                "- Re-read capability_state.capability_usage."
            )
            import_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "user",
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Usage Summary",
                        "description_short": "usage summary validation",
                        "capsule_text": capsule_text,
                    },
                    "probe": False,
                },
            )
            self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))

            profile_upsert, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "profile": {
                        "name": "Usage Summary Profile",
                        "runtime": "custom",
                        "runner": "headless",
                        "command": [],
                        "submit": "enter",
                        "capability_defaults": {
                            "autoload_capabilities": [capability_id],
                            "default_scope": "actor",
                        },
                    },
                },
            )
            self.assertTrue(profile_upsert.ok, getattr(profile_upsert, "error", None))
            profile = (profile_upsert.result or {}).get("profile") if isinstance(profile_upsert.result, dict) else {}
            profile_id = str(profile.get("id") or "").strip() if isinstance(profile, dict) else ""
            self.assertTrue(profile_id)
            profile_actor, _ = self._call(
                "actor_add",
                {
                    "group_id": gid,
                    "actor_id": "peer-3",
                    "runtime": "custom",
                    "runner": "headless",
                    "profile_id": profile_id,
                    "by": "user",
                },
            )
            self.assertTrue(profile_actor.ok, getattr(profile_actor, "error", None))

            for actor_id, scope in (("user", "group"), ("peer-1", "actor"), ("peer-2", "session")):
                enable_resp, _ = self._call(
                    "capability_enable",
                    {
                        "group_id": gid,
                        "by": "user",
                        "actor_id": actor_id,
                        "capability_id": capability_id,
                        "scope": scope,
                        "ttl_seconds": 3600,
                    },
                )
                self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))

            actor_update, _ = self._call(
                "actor_update",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "user",
                    "patch": {"capability_autoload": [capability_id]},
                },
            )
            self.assertTrue(actor_update.ok, getattr(actor_update, "error", None))

            state_resp, _ = self._call(
                "capability_state",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "user",
                    "capability_id": capability_id,
                },
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state_result = state_resp.result if isinstance(state_resp.result, dict) else {}
            usage = state_result.get("capability_usage") if isinstance(state_result.get("capability_usage"), dict) else {}
            self.assertEqual(usage.get("capability_id"), capability_id)
            self.assertTrue(usage.get("used"))
            self.assertTrue(usage.get("group_enabled"))
            self.assertEqual(int(usage.get("group_actor_count") or 0), 3)
            self.assertEqual(int(usage.get("active_actor_count") or 0), 3)
            self.assertEqual(int(usage.get("startup_autoload_actor_count") or 0), 2)
            self.assertIn("peer-1", {str(item.get("actor_id") or "") for item in usage.get("actor_enabled") or []})
            self.assertIn("peer-2", {str(item.get("actor_id") or "") for item in usage.get("session_enabled") or []})
            self.assertIn("peer-1", {str(item.get("actor_id") or "") for item in usage.get("actor_autoload") or []})
            profile_autoload_rows = usage.get("profile_autoload") if isinstance(usage.get("profile_autoload"), list) else []
            profile_autoload_peer = next(
                (
                    item
                    for item in profile_autoload_rows
                    if isinstance(item, dict) and str(item.get("actor_id") or "") == "peer-3"
                ),
                {},
            )
            self.assertEqual(str(profile_autoload_peer.get("profile_id") or ""), profile_id)
            self.assertEqual(str(profile_autoload_peer.get("profile_name") or ""), "Usage Summary Profile")
            session_rows = usage.get("session_enabled") if isinstance(usage.get("session_enabled"), list) else []
            self.assertTrue(any(int(item.get("ttl_seconds") or 0) > 0 for item in session_rows if isinstance(item, dict)))

            peer_state_resp, _ = self._call(
                "capability_state",
                {
                    "group_id": gid,
                    "by": "peer-2",
                    "actor_id": "peer-2",
                },
            )
            self.assertTrue(peer_state_resp.ok, getattr(peer_state_resp, "error", None))
            peer_state = peer_state_resp.result if isinstance(peer_state_resp.result, dict) else {}
            active_rows = (
                peer_state.get("active_capsule_skills")
                if isinstance(peer_state.get("active_capsule_skills"), list)
                else []
            )
            active_row = next(
                (
                    item
                    for item in active_rows
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            activation_sources = (
                active_row.get("activation_sources")
                if isinstance(active_row.get("activation_sources"), list)
                else []
            )
            source_scopes = {str(item.get("scope") or "") for item in activation_sources if isinstance(item, dict)}
            self.assertEqual(source_scopes, {"group", "session"})
            session_sources = [
                item
                for item in activation_sources
                if isinstance(item, dict) and str(item.get("scope") or "") == "session"
            ]
            self.assertTrue(any(int(item.get("ttl_seconds") or 0) > 0 for item in session_sources))
        finally:
            cleanup()

    def test_self_proposed_actor_assignment_persists_autoload_and_can_activate_now(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            self._add_actor(gid, "peer-2", by="user")
            capability_id = "skill:agent_self_proposed:assignment-activate-now"
            capsule_text = (
                "Skill: Assignment Activate Now\n"
                "When to use:\n"
                "- Verify actor assignment writes startup autoload and supports immediate activation.\n"
                "Avoid when:\n"
                "- The skill should only be tried in a temporary session.\n"
                "Procedure:\n"
                "1. Persist actor capability_autoload, then enable actor scope for immediate use.\n"
                "Pitfalls:\n"
                "- Autoload metadata alone is not current runtime activation.\n"
                "Verification:\n"
                "- Re-read actor_autoload_capabilities and active_capsule_skills."
            )
            import_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "user",
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "source_id": "agent_self_proposed",
                        "name": "Assignment Activate Now",
                        "description_short": "assignment activation validation",
                        "capsule_text": capsule_text,
                    },
                    "probe": False,
                },
            )
            self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))

            actor_update, _ = self._call(
                "actor_update",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "user",
                    "patch": {"capability_autoload": [capability_id]},
                },
            )
            self.assertTrue(actor_update.ok, getattr(actor_update, "error", None))

            before_enable, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(before_enable.ok, getattr(before_enable, "error", None))
            before_state = before_enable.result if isinstance(before_enable.result, dict) else {}
            self.assertIn(capability_id, before_state.get("actor_autoload_capabilities") or [])
            before_active = (
                before_state.get("active_capsule_skills")
                if isinstance(before_state.get("active_capsule_skills"), list)
                else []
            )
            before_active_ids = {
                str(item.get("capability_id") or "")
                for item in before_active
                if isinstance(item, dict)
            }
            self.assertNotIn(capability_id, before_active_ids)

            enable_resp, _ = self._call(
                "capability_enable",
                {
                    "group_id": gid,
                    "by": "user",
                    "actor_id": "peer-1",
                    "capability_id": capability_id,
                    "scope": "actor",
                    "enabled": True,
                    "reason": "web_self_proposed_actor_assignment",
                },
            )
            self.assertTrue(enable_resp.ok, getattr(enable_resp, "error", None))

            after_enable, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(after_enable.ok, getattr(after_enable, "error", None))
            after_state = after_enable.result if isinstance(after_enable.result, dict) else {}
            self.assertIn(capability_id, after_state.get("actor_autoload_capabilities") or [])
            self.assertIn(capability_id, after_state.get("enabled_capabilities") or [])
            after_active = (
                after_state.get("active_capsule_skills")
                if isinstance(after_state.get("active_capsule_skills"), list)
                else []
            )
            after_active_ids = {
                str(item.get("capability_id") or "")
                for item in after_active
                if isinstance(item, dict)
            }
            self.assertIn(capability_id, after_active_ids)

            other_actor, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-2", "by": "peer-2"},
            )
            self.assertTrue(other_actor.ok, getattr(other_actor, "error", None))
            other_state = other_actor.result if isinstance(other_actor.result, dict) else {}
            self.assertNotIn(capability_id, other_state.get("enabled_capabilities") or [])
        finally:
            cleanup()

    def test_capability_import_agent_self_proposed_mcp_stays_indexed_by_default(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": "mcp:example/agent-proposed-tool",
                        "kind": "mcp_toolpack",
                        "source_id": "agent_self_proposed",
                        "install_mode": "command",
                        "install_spec": {
                            "command": ["uvx", "demo-mcp"],
                        },
                    },
                },
            )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            record = result.get("record") if isinstance(result.get("record"), dict) else {}
            self.assertEqual(str(record.get("source_id") or ""), "agent_self_proposed")
            self.assertEqual(str(result.get("effective_policy_level") or ""), "indexed")
            self.assertFalse(bool(result.get("enableable_now")))
            self.assertEqual(str(result.get("enable_block_reason") or ""), "policy_level_indexed")
            readiness = result.get("readiness_preview") if isinstance(result.get("readiness_preview"), dict) else {}
            self.assertEqual(str(readiness.get("preview_status") or ""), "blocked")
        finally:
            cleanup()

    def test_capability_import_dry_run_reports_policy_indexed_block(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            get_before, _ = self._call("capability_allowlist_get", {"by": "user"})
            self.assertTrue(get_before.ok, getattr(get_before, "error", None))
            before = get_before.result if isinstance(get_before.result, dict) else {}
            revision_before = str(before.get("revision") or "")
            self.assertTrue(revision_before)
            self.assertEqual(str(before.get("external_capability_safety_mode") or ""), "normal")

            update, _ = self._call(
                "capability_allowlist_update",
                {
                    "by": "user",
                    "mode": "patch",
                    "expected_revision": revision_before,
                    "patch": {
                        "defaults": {"source_level": {
                            "manual_import": "indexed",
                            "mcp_registry_official": "indexed",
                            "anthropic_skills": "indexed",
                            "superpowers_skills": "indexed",
                            "github_skills_curated": "indexed",
                            "skillsmp_remote": "indexed",
                            "clawhub_remote": "indexed",
                            "openclaw_skills_remote": "indexed",
                            "clawskills_remote": "indexed",
                        }},
                    },
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))
            update_result = update.result if isinstance(update.result, dict) else {}
            self.assertEqual(str(update_result.get("external_capability_safety_mode") or ""), "conservative")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": "mcp:example/manual-indexed",
                        "kind": "mcp_toolpack",
                        "install_mode": "command",
                        "install_spec": {
                            "command": ["uvx", "demo-mcp"],
                        },
                    },
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("effective_policy_level") or ""), "indexed")
            self.assertFalse(bool(result.get("enableable_now")))
            self.assertEqual(str(result.get("enable_block_reason") or ""), "policy_level_indexed")
            readiness = result.get("readiness_preview") if isinstance(result.get("readiness_preview"), dict) else {}
            self.assertEqual(str(readiness.get("policy_source") or ""), "external_capability_safety_mode")
            self.assertEqual(str(readiness.get("policy_mode") or ""), "conservative")
        finally:
            cleanup()

    def test_capability_import_copies_fallback_command_shortcuts_into_install_spec(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "mcp:example/import-fallback"
            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": capability_id,
                        "kind": "mcp_toolpack",
                        "install_mode": "package",
                        "install_spec": {
                            "registry_type": "npm",
                            "identifier": "@example/mcp-server",
                        },
                        "fallback_command": ["npx", "-y", "@example/mcp-server"],
                        "fallback_command_candidates": [["npx", "-y", "@example/mcp-server"]],
                    },
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            record = result.get("record") if isinstance(result.get("record"), dict) else {}
            install_spec = record.get("install_spec") if isinstance(record.get("install_spec"), dict) else {}
            self.assertEqual(install_spec.get("fallback_command"), ["npx", "-y", "@example/mcp-server"])
            self.assertEqual(
                install_spec.get("fallback_command_candidates"),
                [["npx", "-y", "@example/mcp-server"]],
            )
        finally:
            cleanup()

    def test_capability_import_mcp_persists_record_and_enables(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "mcp:example/echo-server"

            install_payload = {
                "state": "installed",
                "installer": "remote_http",
                "install_mode": "remote_only",
                "invoker": {"type": "remote_http", "url": "http://127.0.0.1:9900/mcp"},
                "tools": [
                    {
                        "name": "cccc_ext_deadbeef_echo",
                        "real_tool_name": "echo",
                        "description": "Echo tool",
                        "inputSchema": {"type": "object", "properties": {}, "required": []},
                    }
                ],
                "updated_at": "2026-03-01T00:00:00Z",
            }
            with patch(
                "cccc.daemon.ops.capability_ops._install_external_capability",
                return_value=install_payload,
            ):
                resp, _ = self._call(
                    "capability_import",
                    {
                        "group_id": gid,
                        "by": "peer-1",
                        "actor_id": "peer-1",
                        "enable_after_import": True,
                        "scope": "session",
                        "record": {
                            "capability_id": capability_id,
                            "kind": "mcp_toolpack",
                            "name": "Echo Server",
                            "description_short": "Imported echo MCP",
                            "source_id": "mcp_registry_official",
                            "install_mode": "remote_only",
                            "install_spec": {
                                "transport": "http",
                                "url": "http://127.0.0.1:9900/mcp",
                            },
                        },
                    },
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertTrue(bool(result.get("imported")))
            self.assertEqual(str(result.get("state") or ""), "activation_pending")
            self.assertEqual(str(result.get("capability_id") or ""), capability_id)
            enable_result = result.get("enable_result") if isinstance(result.get("enable_result"), dict) else {}
            self.assertEqual(str(enable_result.get("state") or ""), "activation_pending")
            self.assertTrue(bool(enable_result.get("refresh_required")))

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            enabled = set(state.get("enabled_capabilities") or [])
            self.assertIn(capability_id, enabled)

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            self.assertIn(capability_id, rows)
        finally:
            cleanup()

    def test_capability_import_skill_persists_and_enables(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:github:demo:triage"

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "enable_after_import": True,
                    "scope": "actor",
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "name": "Demo Triage",
                        "description_short": "Imported skill capsule",
                        "source_id": "github_skills_curated",
                        "capsule_text": "Use triage checklist",
                        "requires_capabilities": ["pack:group-runtime"],
                        "use_when": ["incident triage and debugging work"],
                        "avoid_when": ["greenfield feature implementation"],
                        "gotchas": ["capture concrete failing evidence before routing work"],
                        "evidence_kind": "error log plus minimal repro note",
                    },
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("state") or ""), "activation_pending")
            self.assertEqual(str(result.get("scope") or ""), "actor")
            enable_result = result.get("enable_result") if isinstance(result.get("enable_result"), dict) else {}
            self.assertEqual(str(enable_result.get("scope") or ""), "actor")
            skill = enable_result.get("skill") if isinstance(enable_result.get("skill"), dict) else {}
            self.assertEqual(str(skill.get("capability_id") or ""), capability_id)

            state_resp, _ = self._call(
                "capability_state",
                {"group_id": gid, "actor_id": "peer-1", "by": "peer-1"},
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            state = state_resp.result if isinstance(state_resp.result, dict) else {}
            enabled = set(state.get("enabled_capabilities") or [])
            self.assertIn(capability_id, enabled)
            active_capsule_skills = state.get("active_capsule_skills") if isinstance(state.get("active_capsule_skills"), list) else []
            active_ids = {str(item.get("capability_id") or "") for item in active_capsule_skills if isinstance(item, dict)}
            self.assertIn(capability_id, active_ids)
            active_row = next(
                (
                    item
                    for item in active_capsule_skills
                    if isinstance(item, dict) and str(item.get("capability_id") or "") == capability_id
                ),
                {},
            )
            activation_sources = (
                active_row.get("activation_sources")
                if isinstance(active_row.get("activation_sources"), list)
                else []
            )
            self.assertEqual({str(item.get("scope") or "") for item in activation_sources if isinstance(item, dict)}, {"actor"})

            _, catalog_doc = ops._load_catalog_doc()
            rows = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            self.assertIn(capability_id, rows)
            record = rows.get(capability_id) if isinstance(rows.get(capability_id), dict) else {}
            self.assertEqual(record.get("use_when"), ["incident triage and debugging work"])
            self.assertEqual(record.get("avoid_when"), ["greenfield feature implementation"])
            self.assertEqual(record.get("gotchas"), ["capture concrete failing evidence before routing work"])
            self.assertEqual(str(record.get("evidence_kind") or ""), "error log plus minimal repro note")
        finally:
            cleanup()

    def test_capability_overview_search_matches_recommendation_fields(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:github:demo:triage"

            import_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "name": "Demo Triage",
                        "description_short": "Imported skill capsule",
                        "source_id": "github_skills_curated",
                        "capsule_text": "Use triage checklist",
                        "use_when": ["incident triage and debugging work"],
                        "avoid_when": ["greenfield feature implementation"],
                        "gotchas": ["capture concrete failing evidence before routing work"],
                        "evidence_kind": "error log plus minimal repro note",
                    },
                },
            )
            self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))

            overview_resp, _ = self._call(
                "capability_overview",
                {
                    "query": "minimal repro note",
                    "limit": 50,
                    "include_indexed": True,
                },
            )
            self.assertTrue(overview_resp.ok, getattr(overview_resp, "error", None))
            result = overview_resp.result if isinstance(overview_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            row = next(
                (
                    item
                    for item in items
                    if isinstance(item, dict)
                    and str(item.get("capability_id") or "") == capability_id
                ),
                None,
            )
            self.assertIsNotNone(row)
            item = row if isinstance(row, dict) else {}
            self.assertEqual(item.get("use_when"), ["incident triage and debugging work"])
            self.assertEqual(item.get("avoid_when"), ["greenfield feature implementation"])
            self.assertEqual(item.get("gotchas"), ["capture concrete failing evidence before routing work"])
            self.assertEqual(str(item.get("evidence_kind") or ""), "error log plus minimal repro note")

            paged_resp, _ = self._call(
                "capability_overview",
                {
                    "query": "",
                    "limit": 1,
                    "offset": 0,
                    "include_indexed": True,
                    "kind": "skill",
                    "source_id": "github_skills_curated",
                },
            )
            self.assertTrue(paged_resp.ok, getattr(paged_resp, "error", None))
            paged_result = paged_resp.result if isinstance(paged_resp.result, dict) else {}
            paged_items = paged_result.get("items") if isinstance(paged_result.get("items"), list) else []
            self.assertEqual(len(paged_items), 1)
            self.assertGreaterEqual(int(paged_result.get("total_count") or 0), 1)
            self.assertIn("has_more", paged_result)
            only_item = paged_items[0] if paged_items and isinstance(paged_items[0], dict) else {}
            self.assertEqual(str(only_item.get("kind") or ""), "skill")
            self.assertEqual(str(only_item.get("source_id") or ""), "github_skills_curated")
        finally:
            cleanup()

    def test_capability_search_matches_recommendation_fields(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")
            capability_id = "skill:github:demo:triage"

            import_resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "record": {
                        "capability_id": capability_id,
                        "kind": "skill",
                        "name": "Demo Triage",
                        "description_short": "Imported skill capsule",
                        "source_id": "github_skills_curated",
                        "capsule_text": "Use triage checklist",
                        "use_when": ["incident triage and debugging work"],
                        "gotchas": ["capture concrete failing evidence before routing work"],
                        "evidence_kind": "error log plus minimal repro note",
                    },
                },
            )
            self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))

            search_resp, _ = self._call(
                "capability_search",
                {
                    "group_id": gid,
                    "actor_id": "peer-1",
                    "by": "peer-1",
                    "query": "failing evidence",
                    "kind": "skill",
                    "include_external": True,
                    "limit": 20,
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            result = search_resp.result if isinstance(search_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            row = next(
                (
                    item
                    for item in items
                    if isinstance(item, dict)
                    and str(item.get("capability_id") or "") == capability_id
                ),
                None,
            )
            self.assertIsNotNone(row)
            item = row if isinstance(row, dict) else {}
            self.assertEqual(item.get("gotchas"), ["capture concrete failing evidence before routing work"])
            self.assertEqual(str(item.get("evidence_kind") or ""), "error log plus minimal repro note")
        finally:
            cleanup()

    def test_capability_import_invalid_record_returns_validation_error(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "record": {"kind": "skill"},
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual((resp.error.code if resp.error else ""), "capability_import_invalid")
        finally:
            cleanup()

    def test_capability_import_command_mode_rejects_empty_command_shortcut(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._add_actor(gid, "peer-1", by="user")

            resp, _ = self._call(
                "capability_import",
                {
                    "group_id": gid,
                    "by": "peer-1",
                    "actor_id": "peer-1",
                    "dry_run": True,
                    "probe": False,
                    "record": {
                        "capability_id": "mcp:example/empty-command",
                        "kind": "mcp_toolpack",
                        "install_mode": "command",
                        "install_spec": {},
                        "command": [],
                    },
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual((resp.error.code if resp.error else ""), "capability_import_invalid")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
