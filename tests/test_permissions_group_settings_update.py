import os
import tempfile
import unittest


class TestGroupSettingsUpdatePermission(unittest.TestCase):
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

    def test_group_settings_update_foreman_allowed_peer_denied(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group
        from cccc.kernel.permissions import require_group_permission

        _, cleanup = self._with_home()
        try:
            create_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {"op": "group_create", "args": {"title": "perm-test", "topic": "", "by": "user"}}
                )
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_foreman_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "actor_add",
                        "args": {
                            "group_id": group_id,
                            "by": "user",
                            "actor_id": "foreman_1",
                            "title": "Foreman",
                            "runtime": "codex",
                            "runner": "pty",
                        },
                    }
                )
            )
            self.assertTrue(add_foreman_resp.ok, getattr(add_foreman_resp, "error", None))

            add_peer_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "actor_add",
                        "args": {
                            "group_id": group_id,
                            "by": "user",
                            "actor_id": "peer_1",
                            "title": "Peer",
                            "runtime": "codex",
                            "runner": "pty",
                        },
                    }
                )
            )
            self.assertTrue(add_peer_resp.ok, getattr(add_peer_resp, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            require_group_permission(group, by="foreman_1", action="group.settings_update")

            with self.assertRaises(ValueError):
                require_group_permission(group, by="peer_1", action="group.settings_update")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
