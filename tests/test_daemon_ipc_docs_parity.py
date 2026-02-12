import re
import unittest
from pathlib import Path


class TestDaemonIpcDocsParity(unittest.TestCase):
    def test_all_daemon_ops_are_documented(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        server_path = repo_root / "src" / "cccc" / "daemon" / "server.py"
        spec_path = repo_root / "docs" / "standards" / "CCCC_DAEMON_IPC_V1.md"

        server_text = server_path.read_text(encoding="utf-8")
        spec_text = spec_path.read_text(encoding="utf-8")

        impl_ops = set(re.findall(r'if op == "([a-z0-9_]+)"', server_text))

        documented_ops: set[str] = set()
        for line in spec_text.splitlines():
            if not line.startswith("#### "):
                continue
            for token in re.findall(r"`([^`]+)`", line):
                if re.fullmatch(r"[a-z0-9_]+", token):
                    documented_ops.add(token)

        missing = sorted(impl_ops - documented_ops)
        self.assertEqual(
            missing,
            [],
            msg=f"Undocumented daemon ops in CCCC_DAEMON_IPC_V1.md: {', '.join(missing)}",
        )


if __name__ == "__main__":
    unittest.main()
