import re
import unittest
from pathlib import Path


class TestDaemonIpcDocsParity(unittest.TestCase):
    def test_all_daemon_ops_are_documented(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        daemon_dir = repo_root / "src" / "cccc" / "daemon"
        server_path = daemon_dir / "server.py"
        ops_dir = daemon_dir / "ops"
        spec_path = repo_root / "docs" / "standards" / "CCCC_DAEMON_IPC_V1.md"

        spec_text = spec_path.read_text(encoding="utf-8")

        impl_ops: set[str] = set()
        impl_sources = [server_path]
        if ops_dir.exists():
            impl_sources.extend(sorted(ops_dir.glob("*.py")))
        for source in impl_sources:
            text = source.read_text(encoding="utf-8")
            impl_ops.update(re.findall(r'if op == "([a-z0-9_]+)"', text))

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
