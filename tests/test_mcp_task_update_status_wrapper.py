import unittest
from unittest.mock import patch


class TestMcpTaskUpdateStatusWrapper(unittest.TestCase):
    def test_task_create_defaults_structural_type_when_type_is_omitted(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {"ops_history": []}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            captured["ops_history"].append(ops)
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            root_out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Root task",
                },
            )
            child_out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Child task",
                    "parent_id": "T001",
                },
            )

        self.assertTrue(bool(root_out.get("ok")))
        self.assertTrue(bool(child_out.get("ok")))
        self.assertEqual(captured["ops_history"][0][0]["task_type"], "standard")
        self.assertEqual(captured["ops_history"][1][0]["task_type"], "free")

    def test_task_create_defaults_peer_assignee_to_self(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "load_group", return_value=object()), patch.object(
            mcp_context, "get_effective_role", return_value="peer"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Peer-owned task",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(captured.get("by"), "peer1")
        self.assertEqual(captured["ops"][0]["assignee"], "peer1")

    def test_task_create_respects_explicit_empty_assignee_for_peer(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "load_group", return_value=object()), patch.object(
            mcp_context, "get_effective_role", return_value="peer"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Shared backlog card",
                    "assignee": "",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertIsNone(captured["ops"][0]["assignee"])

    def test_task_create_does_not_auto_assign_for_foreman(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="lead1"
        ), patch.object(mcp_context, "load_group", return_value=object()), patch.object(
            mcp_context, "get_effective_role", return_value="foreman"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Foreman backlog card",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertIsNone(captured["ops"][0]["assignee"])

    def test_task_create_with_type_persists_task_type_without_hidden_seed(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Improve cold start",
                    "type": "optimization",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(captured.get("group_id"), "g_test")
        self.assertEqual(captured.get("by"), "peer1")
        self.assertEqual(len(captured.get("ops") or []), 1)
        op = captured["ops"][0]
        self.assertEqual(op["op"], "task.create")
        self.assertEqual(op["title"], "Improve cold start")
        self.assertEqual(op["task_type"], "optimization")
        self.assertEqual(op.get("notes"), None)
        self.assertEqual(op.get("checklist"), None)

    def test_task_create_with_blank_notes_and_checklist_keeps_them_blank(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Improve cold start",
                    "type": "optimization",
                    "notes": "",
                    "checklist": [],
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(len(captured.get("ops") or []), 1)
        op = captured["ops"][0]
        self.assertEqual(op["task_type"], "optimization")
        self.assertEqual(op.get("notes"), "")
        self.assertEqual(op.get("checklist"), [])

    def test_task_create_with_explicit_notes_still_persists_task_type(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "create",
                    "title": "Improve cold start",
                    "type": "optimization",
                    "notes": "Keep the user's custom optimization notes.",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        op = captured["ops"][0]
        self.assertEqual(op["task_type"], "optimization")
        self.assertEqual(op["notes"], "Keep the user's custom optimization notes.")

    def test_task_update_with_status_batches_update_and_move(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["dry_run"] = dry_run
            captured["if_version"] = if_version
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "update",
                    "task_id": "T123",
                    "title": "Ship the patch",
                    "status": "done",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(captured.get("group_id"), "g_test")
        self.assertEqual(captured.get("by"), "peer1")
        self.assertEqual(
            captured.get("ops"),
            [
                {"op": "task.update", "task_id": "T123", "title": "Ship the patch"},
                {"op": "task.move", "task_id": "T123", "status": "done"},
            ],
        )

    def test_task_update_with_status_only_degenerates_to_move(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "update",
                    "task_id": "T123",
                    "status": "active",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(captured.get("group_id"), "g_test")
        self.assertEqual(captured.get("by"), "peer1")
        self.assertEqual(
            captured.get("ops"),
            [{"op": "task.move", "task_id": "T123", "status": "active"}],
        )

    def test_task_update_with_type_can_patch_before_move(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "update",
                    "task_id": "T123",
                    "type": "standard",
                    "status": "active",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(captured.get("group_id"), "g_test")
        self.assertEqual(captured.get("by"), "peer1")
        self.assertEqual(len(captured.get("ops") or []), 2)
        patch_op = captured["ops"][0]
        self.assertEqual(patch_op["op"], "task.update")
        self.assertEqual(patch_op["task_id"], "T123")
        self.assertEqual(patch_op["task_type"], "standard")
        self.assertEqual(captured["ops"][1], {"op": "task.move", "task_id": "T123", "status": "active"})

    def test_task_update_with_blank_notes_and_checklist_keeps_them_blank(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "update",
                    "task_id": "T123",
                    "type": "optimization",
                    "notes": "",
                    "checklist": [],
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(
            captured.get("ops"),
            [
                {
                    "op": "task.update",
                    "task_id": "T123",
                    "task_type": "optimization",
                    "notes": "",
                    "checklist": [],
                }
            ],
        )

    def test_task_update_without_status_stays_plain_patch(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "update",
                    "task_id": "T123",
                    "notes": "Need one more repro step.",
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(captured.get("group_id"), "g_test")
        self.assertEqual(captured.get("by"), "peer1")
        self.assertEqual(
            captured.get("ops"),
            [{"op": "task.update", "task_id": "T123", "notes": "Need one more repro step."}],
        )

    def test_task_update_preserves_explicit_notes_and_checklist_with_type(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import context as mcp_context

        captured = {}

        def _fake_context_sync(*, group_id, ops, dry_run=False, if_version=None, by=None):
            captured["group_id"] = group_id
            captured["ops"] = ops
            captured["by"] = by
            return {"ok": True}

        explicit_checklist = [{"text": "Use the custom checklist", "status": "pending"}]
        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_context, "context_sync", side_effect=_fake_context_sync):
            out = mcp_server.handle_tool_call(
                "cccc_task",
                {
                    "action": "update",
                    "task_id": "T123",
                    "type": "optimization",
                    "notes": "Custom note",
                    "checklist": explicit_checklist,
                },
            )

        self.assertTrue(bool(out.get("ok")))
        self.assertEqual(
            captured.get("ops"),
            [
                {
                    "op": "task.update",
                    "task_id": "T123",
                    "task_type": "optimization",
                    "notes": "Custom note",
                    "checklist": explicit_checklist,
                }
            ],
        )

    def test_task_type_rejects_unknown_id(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ):
            with self.assertRaises(mcp_server.MCPError) as raised:
                mcp_server.handle_tool_call(
                    "cccc_task",
                    {
                        "action": "create",
                        "title": "Bad task",
                        "type": "unknown",
                    },
                )

        self.assertEqual(raised.exception.code, "invalid_request")
        self.assertIn("type", raised.exception.message)


if __name__ == "__main__":
    unittest.main()
