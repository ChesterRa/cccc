import os
import tempfile
import unittest


class TestRuntimePoolDefaultsImmutable(unittest.TestCase):
    def test_get_runtime_pool_returns_copy_of_defaults(self) -> None:
        from cccc.kernel.settings import get_runtime_pool

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                first = get_runtime_pool()
                self.assertTrue(len(first) > 0)
                first[0].runtime = "mutated-runtime"
                first[0].scenarios.append("mutated-scenario")

                second = get_runtime_pool()
                self.assertTrue(len(second) > 0)
                self.assertNotEqual(second[0].runtime, "mutated-runtime")
                self.assertNotIn("mutated-scenario", second[0].scenarios)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_runtime_pool_from_dict_tolerates_dirty_fields(self) -> None:
        from cccc.kernel.settings import RuntimePoolEntry

        entry = RuntimePoolEntry.from_dict(
            {
                "runtime": "codex",
                "priority": "bad",
                "scenarios": ["coding", 1, "", None],
                "notes": 123,
            }
        )
        self.assertEqual(entry.runtime, "codex")
        self.assertEqual(entry.priority, 999)
        self.assertEqual(entry.scenarios, ["coding"])
        self.assertEqual(entry.notes, "123")


if __name__ == "__main__":
    unittest.main()
