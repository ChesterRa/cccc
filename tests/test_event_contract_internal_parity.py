import unittest
from pathlib import Path
from typing import get_args


class TestEventContractInternalParity(unittest.TestCase):
    def test_event_kind_literal_and_model_map_stay_in_sync(self) -> None:
        from cccc.contracts.v1.event import EventKind, _KIND_TO_MODEL

        kinds = set(get_args(EventKind))
        mapped = set(_KIND_TO_MODEL.keys())

        self.assertEqual(
            sorted(kinds - mapped),
            [],
            msg=f"EventKind literals missing models: {sorted(kinds - mapped)}",
        )
        self.assertEqual(
            sorted(mapped - kinds),
            [],
            msg=f"Model map has kinds not declared in EventKind: {sorted(mapped - kinds)}",
        )

    def test_reference_architecture_names_all_event_kinds(self) -> None:
        from cccc.contracts.v1.event import EventKind

        repo_root = Path(__file__).resolve().parents[1]
        text = (repo_root / "docs/reference/architecture.md").read_text(encoding="utf-8")
        missing = [kind for kind in get_args(EventKind) if f"`{kind}`" not in text]

        self.assertEqual(missing, [])

    def test_actor_new_session_accepts_the_in_place_rotation_contract(self) -> None:
        from cccc.contracts.v1.event import normalize_event_data

        payload = normalize_event_data(
            "actor.new_session",
            {
                "actor_id": "peer1",
                "runner": "pty",
                "runtime": "antigravity",
                "rotation": "in_place",
            },
        )

        self.assertEqual(payload["actor_id"], "peer1")
        self.assertEqual(payload["runtime"], "antigravity")
        self.assertEqual(payload["rotation"], "in_place")


if __name__ == "__main__":
    unittest.main()
