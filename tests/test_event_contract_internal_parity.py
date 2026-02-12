import unittest
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


if __name__ == "__main__":
    unittest.main()
