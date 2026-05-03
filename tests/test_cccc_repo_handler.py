import io
import hashlib
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestCcccRepoHandler(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
