from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path


class TestSelfEvolutionCapability(unittest.TestCase):
    def setUp(self) -> None:
        self._old_home = os.environ.get("CCCC_HOME")
        self._temp = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        os.environ["CCCC_HOME"] = self._temp.name

    def tearDown(self) -> None:
        self._temp.cleanup()
        if self._old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self._old_home

    def _call(self, op: str, args: dict):
        from cccc.daemon.ops import capability_ops

        handlers = {
            "capability_block": capability_ops.handle_capability_block,
            "capability_enable": capability_ops.handle_capability_enable,
            "capability_state": capability_ops.handle_capability_state,
        }
        return handlers[op](args)

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        return create_group(load_registry(), title="self-evolution").group_id

    def _state(self, group_id: str) -> dict:
        response = self._call(
            "capability_state",
            {"group_id": group_id, "actor_id": "user", "by": "user"},
        )
        self.assertTrue(response.ok, response.error)
        return response.result or {}

    def test_builtin_record_uses_packaged_capsule(self) -> None:
        from cccc.kernel.capabilities import BUILTIN_CAPSULE_SKILLS
        from cccc.kernel.self_evolution_capability import SELF_EVOLUTION_CAPABILITY_ID

        record = BUILTIN_CAPSULE_SKILLS[SELF_EVOLUTION_CAPABILITY_ID]
        self.assertEqual(record["name"], "cccc-self-evolution")
        self.assertIn("Five targets", record["capsule_text"])
        self.assertIn(SELF_EVOLUTION_CAPABILITY_ID, record["capsule_text"])

    def test_default_enable_is_seeded_once_and_manual_disable_persists(self) -> None:
        from cccc.kernel.self_evolution_capability import SELF_EVOLUTION_CAPABILITY_ID

        group_id = self._create_group()
        first = self._state(group_id)
        self.assertIn(SELF_EVOLUTION_CAPABILITY_ID, first["enabled_capabilities"])
        self.assertTrue(
            any(
                row.get("capability_id") == SELF_EVOLUTION_CAPABILITY_ID
                for row in first["active_capsule_skills"]
            )
        )

        disabled = self._call(
            "capability_enable",
            {
                "group_id": group_id,
                "actor_id": "user",
                "by": "user",
                "capability_id": SELF_EVOLUTION_CAPABILITY_ID,
                "scope": "group",
                "enabled": False,
            },
        )
        self.assertTrue(disabled.ok, disabled.error)
        self.assertNotIn(
            SELF_EVOLUTION_CAPABILITY_ID, self._state(group_id)["enabled_capabilities"]
        )
        self.assertNotIn(
            SELF_EVOLUTION_CAPABILITY_ID, self._state(group_id)["enabled_capabilities"]
        )

    def test_manual_disable_before_first_state_read_is_not_reversed(self) -> None:
        from cccc.kernel.self_evolution_capability import SELF_EVOLUTION_CAPABILITY_ID

        group_id = self._create_group()
        disabled = self._call(
            "capability_enable",
            {
                "group_id": group_id,
                "actor_id": "user",
                "by": "user",
                "capability_id": SELF_EVOLUTION_CAPABILITY_ID,
                "scope": "group",
                "enabled": False,
            },
        )
        self.assertTrue(disabled.ok, disabled.error)
        self.assertNotIn(
            SELF_EVOLUTION_CAPABILITY_ID, self._state(group_id)["enabled_capabilities"]
        )

    def test_legacy_binding_migrates_without_duplicate_activation(self) -> None:
        from cccc.kernel.self_evolution_capability import (
            LEGACY_SELF_EVOLUTION_CAPABILITY_ID,
            SELF_EVOLUTION_CAPABILITY_ID,
        )

        group_id = self._create_group()
        state_path = Path(self._temp.name) / "state" / "capabilities" / "state.json"
        state_path.parent.mkdir(parents=True, exist_ok=True)
        state_path.write_text(
            json.dumps(
                {
                    "v": 1,
                    "group_enabled": {group_id: [LEGACY_SELF_EVOLUTION_CAPABILITY_ID]},
                }
            ),
            encoding="utf-8",
        )

        state = self._state(group_id)
        self.assertIn(SELF_EVOLUTION_CAPABILITY_ID, state["enabled_capabilities"])
        self.assertNotIn(
            LEGACY_SELF_EVOLUTION_CAPABILITY_ID, state["enabled_capabilities"]
        )
        persisted = json.loads(state_path.read_text(encoding="utf-8"))
        self.assertIn(
            LEGACY_SELF_EVOLUTION_CAPABILITY_ID, persisted["group_removed"][group_id]
        )

    def test_legacy_manual_disable_migrates_to_the_builtin_capability(self) -> None:
        from cccc.kernel.self_evolution_capability import (
            LEGACY_SELF_EVOLUTION_CAPABILITY_ID,
            SELF_EVOLUTION_CAPABILITY_ID,
        )

        group_id = self._create_group()
        state_path = Path(self._temp.name) / "state" / "capabilities" / "state.json"
        state_path.parent.mkdir(parents=True, exist_ok=True)
        state_path.write_text(
            json.dumps(
                {
                    "v": 1,
                    "group_removed": {group_id: [LEGACY_SELF_EVOLUTION_CAPABILITY_ID]},
                }
            ),
            encoding="utf-8",
        )

        state = self._state(group_id)
        self.assertNotIn(SELF_EVOLUTION_CAPABILITY_ID, state["enabled_capabilities"])
        persisted = json.loads(state_path.read_text(encoding="utf-8"))
        self.assertIn(
            SELF_EVOLUTION_CAPABILITY_ID, persisted["group_removed"][group_id]
        )

    def test_legacy_block_and_hidden_controls_migrate_with_metadata(self) -> None:
        from cccc.kernel.self_evolution_capability import (
            LEGACY_SELF_EVOLUTION_CAPABILITY_ID,
            SELF_EVOLUTION_CAPABILITY_ID,
        )

        group_id = self._create_group()
        global_block = {
            "reason": "global policy",
            "by": "user",
            "blocked_at": "2026-08-25T00:00:00Z",
            "expires_at": "",
        }
        group_block = {
            "reason": "group policy",
            "by": "foreman",
            "blocked_at": "2026-08-25T01:00:00Z",
            "expires_at": "",
        }
        state_path = Path(self._temp.name) / "state" / "capabilities" / "state.json"
        state_path.parent.mkdir(parents=True, exist_ok=True)
        state_path.write_text(
            json.dumps(
                {
                    "v": 1,
                    "default_group_capability_seed_versions": {group_id: 1},
                    "global_blocked": {
                        LEGACY_SELF_EVOLUTION_CAPABILITY_ID: global_block
                    },
                    "group_blocked": {
                        group_id: {
                            LEGACY_SELF_EVOLUTION_CAPABILITY_ID: group_block
                        }
                    },
                    "actor_hidden": {
                        group_id: {
                            "user": [LEGACY_SELF_EVOLUTION_CAPABILITY_ID]
                        }
                    },
                }
            ),
            encoding="utf-8",
        )

        state = self._state(group_id)
        self.assertNotIn(SELF_EVOLUTION_CAPABILITY_ID, state["enabled_capabilities"])
        self.assertIn(
            SELF_EVOLUTION_CAPABILITY_ID,
            state["actor_hidden_capabilities"],
        )
        persisted = json.loads(state_path.read_text(encoding="utf-8"))
        self.assertEqual(
            persisted["global_blocked"][SELF_EVOLUTION_CAPABILITY_ID],
            global_block,
        )
        self.assertEqual(
            persisted["group_blocked"][group_id][SELF_EVOLUTION_CAPABILITY_ID],
            group_block,
        )
        self.assertIn(
            SELF_EVOLUTION_CAPABILITY_ID,
            persisted["actor_hidden"][group_id]["user"],
        )

    def test_blocked_default_is_not_active(self) -> None:
        from cccc.kernel.self_evolution_capability import SELF_EVOLUTION_CAPABILITY_ID

        group_id = self._create_group()
        blocked = self._call(
            "capability_block",
            {
                "group_id": group_id,
                "actor_id": "user",
                "by": "user",
                "capability_id": SELF_EVOLUTION_CAPABILITY_ID,
                "scope": "group",
                "blocked": True,
                "reason": "test",
            },
        )
        self.assertTrue(blocked.ok, blocked.error)
        state = self._state(group_id)
        self.assertNotIn(SELF_EVOLUTION_CAPABILITY_ID, state["enabled_capabilities"])
        self.assertFalse(
            any(
                row.get("capability_id") == SELF_EVOLUTION_CAPABILITY_ID
                for row in state["active_capsule_skills"]
            )
        )


if __name__ == "__main__":
    unittest.main()
