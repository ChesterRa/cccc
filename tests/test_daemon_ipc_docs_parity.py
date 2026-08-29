import re
import unittest
from pathlib import Path


class TestDaemonIpcDocsParity(unittest.TestCase):
    def test_removed_panorama_blueprint_operation_is_not_promised(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        spec_text = (
            repo_root / "docs" / "standards" / "CCCC_DAEMON_IPC_V1.md"
        ).read_text(encoding="utf-8")

        self.assertNotIn("#### `blueprint_generate`", spec_text)

    def test_authoritative_contract_points_to_native_sources(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        spec_path = repo_root / "docs" / "standards" / "CCCC_DAEMON_IPC_V1.md"
        spec_text = spec_path.read_text(encoding="utf-8")

        self.assertNotIn("src/cccc/", spec_text)
        self.assertIn("crates/cccc-contracts/src/ipc.rs", spec_text)
        self.assertIn("crates/cccc-contracts/src/event.rs", spec_text)
        self.assertIn("crates/cccc-core/src/permissions.rs", spec_text)
        self.assertIn('implementation: "rust"', spec_text)
        self.assertNotIn("Both daemon implementations", spec_text)
        self.assertNotIn("empty = broadcast", spec_text)

    def test_all_documented_daemon_ops_have_native_routes(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        spec_text = (
            repo_root / "docs" / "standards" / "CCCC_DAEMON_IPC_V1.md"
        ).read_text(encoding="utf-8")
        daemon_source = "\n".join(
            path.read_text(encoding="utf-8")
            for path in sorted(
                (repo_root / "crates" / "cccc-daemon" / "src").rglob("*.rs")
            )
        )

        documented_ops: set[str] = set()
        for line in spec_text.splitlines():
            if not line.startswith("#### "):
                continue
            documented_ops.update(
                token
                for token in re.findall(r"`([^`]+)`", line)
                if re.fullmatch(r"[a-z0-9_]+", token)
            )

        missing = sorted(
            operation
            for operation in documented_ops
            if f'"{operation}"' not in daemon_source
        )
        self.assertEqual(
            missing,
            [],
            msg=f"Documented daemon ops without a native route: {', '.join(missing)}",
        )


if __name__ == "__main__":
    unittest.main()
