import asyncio
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestWebStreamsTailerCleanup(unittest.TestCase):
    def test_idle_tailer_removes_registry_entry_on_exit(self) -> None:
        from cccc.ports.web import streams

        async def _timeout(awaitable, *_args, **_kwargs):
            if asyncio.iscoroutine(awaitable):
                awaitable.close()
            raise asyncio.TimeoutError()

        async def _run_case(path: Path) -> None:
            key = ("ledger", str(path))
            streams._TAILERS.pop(key, None)  # type: ignore[attr-defined]
            tailer = streams._SharedJSONLTailer(path, event_name="ledger", heartbeat_s=30.0)  # type: ignore[attr-defined]
            streams._TAILERS[key] = tailer  # type: ignore[attr-defined]
            with patch("cccc.ports.web.streams.asyncio.wait_for", new=_timeout):
                await tailer._run()  # type: ignore[attr-defined]
            self.assertNotIn(key, streams._TAILERS)  # type: ignore[attr-defined]
            self.assertIsNone(tailer._f)  # type: ignore[attr-defined]
            self.assertIsNone(tailer._task)  # type: ignore[attr-defined]

        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "ledger.jsonl"
            asyncio.run(_run_case(path))


if __name__ == "__main__":
    unittest.main()
