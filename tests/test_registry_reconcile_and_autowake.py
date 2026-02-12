import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestRegistryReconcileAndAutoWake(unittest.TestCase):
    def test_auto_wake_failure_keeps_actor_disabled(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon import server as daemon_server
        from cccc.daemon.server import handle_request
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                # Create group + attach a scope.
                create_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}}
                    )
                )
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                gid = str((create_resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(gid)

                scope_dir = Path(td) / "scope"
                scope_dir.mkdir(parents=True, exist_ok=True)
                attach_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "attach", "args": {"path": str(scope_dir), "group_id": gid, "by": "user"}}
                    )
                )
                self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))

                # Add a disabled recipient actor.
                add_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_add",
                            "args": {
                                "group_id": gid,
                                "actor_id": "peer1",
                                "runtime": "codex",
                                "runner": "headless",
                                "by": "user",
                            },
                        }
                    )
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))
                disable_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_update",
                            "args": {
                                "group_id": gid,
                                "actor_id": "peer1",
                                "patch": {"enabled": False},
                                "by": "user",
                            },
                        }
                    )
                )
                self.assertTrue(disable_resp.ok, getattr(disable_resp, "error", None))

                # Simulate wake-up startup failure.
                with patch.object(
                    daemon_server,
                    "_start_actor_process",
                    return_value={"success": False, "event": None, "effective_runner": None, "error": "boom"},
                ):
                    send_resp, _ = handle_request(
                        DaemonRequest.model_validate(
                            {
                                "op": "send",
                                "args": {
                                    "group_id": gid,
                                    "by": "user",
                                    "text": "hi",
                                    "to": ["peer1"],
                                },
                            }
                        )
                    )
                self.assertFalse(send_resp.ok)
                self.assertEqual((send_resp.error.code if send_resp.error else ""), "no_enabled_recipients")

                g = load_group(gid)
                self.assertIsNotNone(g)
                actor = find_actor(g, "peer1") if g is not None else None
                self.assertIsNotNone(actor)
                self.assertFalse(bool(actor.get("enabled", True)))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_registry_cleanup_is_explicit_not_implicit(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.registry import load_registry
        from cccc.paths import ensure_home

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                # Create group + attach scope so defaults also point to this group.
                create_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}}
                    )
                )
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                gid = str((create_resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(gid)

                scope_dir = Path(td) / "scope"
                scope_dir.mkdir(parents=True, exist_ok=True)
                attach_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "attach", "args": {"path": str(scope_dir), "group_id": gid, "by": "user"}}
                    )
                )
                self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))

                reg_before = load_registry()
                self.assertIn(gid, reg_before.groups)
                default_keys = [k for k, v in reg_before.defaults.items() if str(v or "").strip() == gid]

                # Simulate a missing group.yaml while registry entry remains.
                group_yaml = ensure_home() / "groups" / gid / "group.yaml"
                self.assertTrue(group_yaml.exists())
                group_yaml.unlink()

                # Listing groups must not mutate registry.
                groups_resp, _ = handle_request(DaemonRequest.model_validate({"op": "groups", "args": {}}))
                self.assertTrue(groups_resp.ok, getattr(groups_resp, "error", None))
                listed_ids = [str(g.get("group_id") or "").strip() for g in (groups_resp.result or {}).get("groups", [])]
                self.assertNotIn(gid, listed_ids)

                reg_after_list = load_registry()
                self.assertIn(gid, reg_after_list.groups)

                # Dry-run reconcile: detect but do not remove.
                dry_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "registry_reconcile", "args": {"remove_missing": False, "by": "user"}}
                    )
                )
                self.assertTrue(dry_resp.ok, getattr(dry_resp, "error", None))
                self.assertIn(gid, (dry_resp.result or {}).get("missing_group_ids", []))
                self.assertEqual((dry_resp.result or {}).get("removed_group_ids", []), [])

                # Explicit reconcile: remove missing entries (+ related defaults).
                clean_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "registry_reconcile", "args": {"remove_missing": True, "by": "user"}}
                    )
                )
                self.assertTrue(clean_resp.ok, getattr(clean_resp, "error", None))
                self.assertIn(gid, (clean_resp.result or {}).get("removed_group_ids", []))

                reg_after_clean = load_registry()
                self.assertNotIn(gid, reg_after_clean.groups)
                for k in default_keys:
                    self.assertNotIn(k, reg_after_clean.defaults)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
