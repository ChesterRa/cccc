from pathlib import Path
import unittest


class TestLifecycleTestsHeadlessRunner(unittest.TestCase):
    def test_lifecycle_actor_tests_keep_headless_runner_coverage(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        lifecycle_tests = [
            repo_root / "tests" / "test_actor_lifecycle_ops.py",
            repo_root / "tests" / "test_group_lifecycle_invariants.py",
        ]
        for path in lifecycle_tests:
            text = path.read_text(encoding="utf-8")
            self.assertIn('"runner": "headless"', text, msg=f"{path.name} should use headless runner in actor add payloads")


if __name__ == "__main__":
    unittest.main()
