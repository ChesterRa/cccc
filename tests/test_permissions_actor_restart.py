import tempfile
import unittest
from pathlib import Path


class TestPermissionsActorRestart(unittest.TestCase):
    def _make_group(self):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import Group

        td_ctx = tempfile.TemporaryDirectory()
        group = Group(group_id="g_test", path=Path(td_ctx.name), doc={"actors": []})
        add_actor(group, actor_id="lead", runtime="codex", runner="headless")
        add_actor(group, actor_id="peer1", runtime="codex", runner="headless")
        add_actor(group, actor_id="peer2", runtime="codex", runner="headless")
        return td_ctx, group

    def test_peer_can_restart_foreman(self) -> None:
        from cccc.kernel.permissions import require_actor_permission

        td_ctx, group = self._make_group()
        try:
            require_actor_permission(group, by="peer1", action="actor.restart", target_actor_id="lead")
        finally:
            td_ctx.cleanup()

    def test_peer_can_restart_other_peer(self) -> None:
        from cccc.kernel.permissions import require_actor_permission

        td_ctx, group = self._make_group()
        try:
            require_actor_permission(group, by="peer1", action="actor.restart", target_actor_id="peer2")
        finally:
            td_ctx.cleanup()

    def test_peer_still_cannot_stop_other_actor(self) -> None:
        from cccc.kernel.permissions import require_actor_permission

        td_ctx, group = self._make_group()
        try:
            with self.assertRaisesRegex(ValueError, "peer can only actor.stop self, not lead"):
                require_actor_permission(group, by="peer1", action="actor.stop", target_actor_id="lead")
        finally:
            td_ctx.cleanup()


if __name__ == "__main__":
    unittest.main()
