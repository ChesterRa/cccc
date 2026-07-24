from __future__ import annotations

import os
import tempfile
import unittest


class TestVoiceSecretaryActor(unittest.TestCase):
    def test_canonical_assistant_enabled_keeps_voice_secretary_actor(self) -> None:
        from cccc.kernel.actors import add_actor, find_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.voice_secretary_actor import ensure_voice_secretary_actor, sync_voice_secretary_actor

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                group = create_group(load_registry(), title="voice-alias", topic="")
                add_actor(group, actor_id="lead", runtime="codex", runner="pty")
                ensure_voice_secretary_actor(group)
                group.doc["assistants"] = {"assistant": {"enabled": True}}
                group.save()

                synced = sync_voice_secretary_actor(group)

                self.assertIsNotNone(synced)
                reloaded = load_group(group.group_id)
                self.assertIsNotNone(reloaded)
                assert reloaded is not None
                self.assertIsNotNone(find_actor(reloaded, "voice-secretary"))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
