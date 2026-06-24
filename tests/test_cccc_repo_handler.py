import io
import hashlib
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestCcccRepoHandler(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return Path(td), cleanup

    def _create_group_with_scope(self, home: Path):
        from cccc.kernel.group import attach_scope_to_group, create_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope

        root = home / "repo"
        root.mkdir(parents=True, exist_ok=True)
        reg = load_registry()
        group = create_group(reg, title="repo-search", topic="")
        return attach_scope_to_group(reg, group, detect_scope(root), set_active=True), root

    def test_read_text_reads_only_requested_byte_limit(self) -> None:
        from cccc.ports.mcp.handlers.cccc_repo import _read_text

        read_sizes: list[int] = []

        class FakeFile(io.BytesIO):
            def read(self, size: int = -1) -> bytes:
                read_sizes.append(size)
                if size < 0:
                    return b"x" * 2_000_000
                return b"x" * size

        def fake_open(_self: Path, *_args, **_kwargs) -> FakeFile:
            return FakeFile()

        with (
            patch.object(Path, "exists", return_value=True),
            patch.object(Path, "is_file", return_value=True),
            patch.object(Path, "stat", return_value=SimpleNamespace(st_size=2_000_000)),
            patch.object(Path, "open", new=fake_open),
        ):
            text, truncated, size, sha256 = _read_text(Path("large.txt"), max_bytes=1234)

        self.assertEqual(read_sizes, [1234])
        self.assertEqual(len(text), 1234)
        self.assertTrue(truncated)
        self.assertEqual(size, 2_000_000)
        self.assertEqual(sha256, "")

    def test_read_text_returns_sha256_for_complete_read(self) -> None:
        from cccc.ports.mcp.handlers.cccc_repo import _read_text

        payload = b"small text\n"
        with (
            patch.object(Path, "exists", return_value=True),
            patch.object(Path, "is_file", return_value=True),
            patch.object(Path, "stat", return_value=SimpleNamespace(st_size=len(payload))),
            patch.object(Path, "open", return_value=io.BytesIO(payload)),
        ):
            text, truncated, size, sha256 = _read_text(Path("small.txt"), max_bytes=1234)

        self.assertEqual(text, payload.decode("utf-8"))
        self.assertFalse(truncated)
        self.assertEqual(size, len(payload))
        self.assertEqual(sha256, hashlib.sha256(payload).hexdigest())

    def test_repo_search_supports_regex_globs_and_context(self) -> None:
        from cccc.ports.mcp.handlers.cccc_repo import repo_search_tool

        home, cleanup = self._with_home()
        try:
            group, root = self._create_group_with_scope(home)
            (root / "src").mkdir()
            (root / "src" / "deep").mkdir()
            (root / "docs").mkdir()
            (root / "vendor" / "src").mkdir(parents=True)
            (root / "src" / "app.py").write_text("alpha\nbeta target\nomega\n", encoding="utf-8")
            (root / "src" / "skip.py").write_text("beta target should be excluded\n", encoding="utf-8")
            (root / "src" / "deep" / "nested.py").write_text("beta target should not match src/*.py\n", encoding="utf-8")
            (root / "docs" / "note.md").write_text("beta target outside include glob\n", encoding="utf-8")
            (root / "vendor" / "src" / "foo.py").write_text(
                "beta target should not suffix-match src/*.py\n",
                encoding="utf-8",
            )

            out = repo_search_tool(
                group_id=group.group_id,
                query=r"b.ta target",
                regex=True,
                include_globs=["src/*.py"],
                exclude_globs=["src/skip.py"],
                context_lines=1,
            )

            matches = out.get("matches") if isinstance(out.get("matches"), list) else []
            self.assertEqual(len(matches), 1)
            self.assertEqual(matches[0].get("path"), "src/app.py")
            self.assertEqual(matches[0].get("line_number"), 2)
            self.assertEqual((matches[0].get("before_context") or [{}])[0].get("line_text"), "alpha")
            self.assertEqual((matches[0].get("after_context") or [{}])[0].get("line_text"), "omega")
            self.assertEqual(out.get("include_globs"), ["src/*.py"])
            self.assertEqual(out.get("exclude_globs"), ["src/skip.py"])
            self.assertEqual(out.get("context_lines"), 1)
            self.assertEqual(out.get("regex"), True)
            self.assertEqual(out.get("filtered_files"), 4)
        finally:
            cleanup()

    def test_repo_search_searches_read_prefix_of_large_files(self) -> None:
        from cccc.ports.mcp.handlers.cccc_repo import repo_search_tool

        home, cleanup = self._with_home()
        try:
            group, root = self._create_group_with_scope(home)
            (root / "large.txt").write_text("target\n" + ("x" * 100), encoding="utf-8")

            out = repo_search_tool(group_id=group.group_id, query="target", max_file_bytes=10)

            matches = out.get("matches") if isinstance(out.get("matches"), list) else []
            self.assertEqual(len(matches), 1)
            self.assertEqual(matches[0].get("path"), "large.txt")
            self.assertTrue(matches[0].get("file_truncated"))
            self.assertEqual(out.get("truncated_files"), 1)
            self.assertEqual(out.get("skipped_files"), 0)
        finally:
            cleanup()

    def test_repo_search_rejects_invalid_regex(self) -> None:
        from cccc.ports.mcp.common import MCPError
        from cccc.ports.mcp.handlers.cccc_repo import repo_search_tool

        home, cleanup = self._with_home()
        try:
            group, root = self._create_group_with_scope(home)
            (root / "README.md").write_text("alpha\n", encoding="utf-8")

            with self.assertRaises(MCPError) as raised:
                repo_search_tool(group_id=group.group_id, query="[", regex=True)

            self.assertEqual(raised.exception.code, "invalid_regex")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
