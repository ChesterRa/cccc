import re
import unittest
from pathlib import Path


class TestEventKindModelParity(unittest.TestCase):
    def test_standard_append_event_kinds_are_modeled(self) -> None:
        from cccc.contracts.v1.event import _KIND_TO_MODEL

        repo_root = Path(__file__).resolve().parents[1]
        files = [
            *Path(repo_root / "src" / "cccc" / "daemon").glob("**/*.py"),
            *Path(repo_root / "src" / "cccc" / "kernel").glob("**/*.py"),
            repo_root / "src" / "cccc" / "cli.py",
        ]

        pattern = re.compile(r'append_event\([^\)]*?kind\s*=\s*"([a-z0-9_.-]+)"', re.S)
        used_kinds = set()
        for path in files:
            text = path.read_text(encoding="utf-8", errors="ignore")
            used_kinds.update(pattern.findall(text))

        standard_kinds = {
            kind
            for kind in used_kinds
            if kind.startswith(("group.", "actor.", "chat.", "system.", "context."))
        }
        modeled = set(_KIND_TO_MODEL.keys())
        missing = sorted(standard_kinds - modeled)
        self.assertEqual(
            missing,
            [],
            msg=f"Standard append_event kinds missing from contracts.v1.event models: {', '.join(missing)}",
        )


if __name__ == "__main__":
    unittest.main()
