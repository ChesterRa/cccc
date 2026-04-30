import io
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
            text, truncated, size = _read_text(Path("large.txt"), max_bytes=1234)

        self.assertEqual(read_sizes, [1234])
        self.assertEqual(len(text), 1234)
        self.assertTrue(truncated)
        self.assertEqual(size, 2_000_000)


if __name__ == "__main__":
    unittest.main()
