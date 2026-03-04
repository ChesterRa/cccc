from __future__ import annotations

import unittest
from unittest.mock import patch


class TestMcpCapabilityImport(unittest.TestCase):
    def test_capability_import_wrapper_calls_daemon(self) -> None:
        from cccc.ports.mcp.server import capability_import

        record = {
            "capability_id": "skill:github:demo:triage",
            "kind": "skill",
            "capsule_text": "Use triage checklist",
        }
        with patch(
            "cccc.ports.mcp.handlers.cccc_capability._call_daemon_or_raise",
            return_value={"ok": True},
        ) as daemon_mock:
            result = capability_import(
                group_id="g1",
                by="peer-1",
                actor_id="peer-1",
                record=record,
                dry_run=True,
                probe=False,
                enable_after_import=False,
                scope="session",
                ttl_seconds=600,
                reason="validate import",
            )

        self.assertEqual(result, {"ok": True})
        daemon_mock.assert_called_once()
        req = daemon_mock.call_args.args[0] if daemon_mock.call_args and daemon_mock.call_args.args else {}
        self.assertEqual(str(req.get("op") or ""), "capability_import")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(str(args.get("group_id") or ""), "g1")
        self.assertEqual(str(args.get("by") or ""), "peer-1")
        self.assertEqual(str(args.get("actor_id") or ""), "peer-1")
        self.assertTrue(bool(args.get("dry_run")))
        self.assertFalse(bool(args.get("probe")))
        self.assertEqual(str(args.get("record", {}).get("capability_id") or ""), "skill:github:demo:triage")

    def test_mcp_router_capability_import_accepts_actor_id_without_by(self) -> None:
        import os
        from cccc.ports.mcp.server import handle_tool_call

        with patch(
            "cccc.ports.mcp.server.capability_import",
            return_value={"ok": True, "state": "ready"},
        ) as import_mock, patch.dict(
            os.environ,
            {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"},
        ):
            result = handle_tool_call(
                "cccc_capability_import",
                {
                    "group_id": "g1",
                    "actor_id": "peer-1",
                    "record": {
                        "capability_id": "skill:github:demo:triage",
                        "kind": "skill",
                        "capsule_text": "Use triage checklist",
                    },
                    "dry_run": True,
                },
            )

        self.assertEqual(str(result.get("state") or ""), "ready")
        import_mock.assert_called_once()
        kwargs = import_mock.call_args.kwargs if import_mock.call_args else {}
        self.assertEqual(str(kwargs.get("group_id") or ""), "g1")
        self.assertEqual(str(kwargs.get("by") or ""), "peer-1")
        self.assertEqual(str(kwargs.get("actor_id") or ""), "peer-1")
        self.assertTrue(bool(kwargs.get("dry_run")))


if __name__ == "__main__":
    unittest.main()
