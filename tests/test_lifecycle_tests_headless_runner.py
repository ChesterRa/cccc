from pathlib import Path
import unittest


class TestLifecycleTestsHeadlessRunner(unittest.TestCase):
    def test_lifecycle_actor_tests_do_not_use_pty_runner(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        lifecycle_tests = [
            repo_root / "tests" / "test_actor_lifecycle_ops.py",
            repo_root / "tests" / "test_group_lifecycle_invariants.py",
        ]
        for path in lifecycle_tests:
            text = path.read_text(encoding="utf-8")
            self.assertIn('"runner": "headless"', text, msg=f"{path.name} should use headless runner in actor add payloads")
            self.assertNotIn('"runner": "pty"', text, msg=f"{path.name} must not depend on PTY runtime binaries")


if __name__ == "__main__":
    unittest.main()
