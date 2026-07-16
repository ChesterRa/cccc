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

    def test_close_tailers_under_releases_group_file_handles(self) -> None:
        from cccc.ports.web import streams

        async def _run_case(group_dir: Path) -> None:
            path = group_dir / "state" / "headless" / "events.jsonl"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("", encoding="utf-8")
            key = ("headless", str(path), "0.050")
            streams._TAILERS.pop(key, None)  # type: ignore[attr-defined]
            tailer = await streams._get_tailer(path, event_name="headless", heartbeat_s=30.0, poll_interval_s=0.05)  # type: ignore[attr-defined]
            q: asyncio.Queue[bytes | None] = asyncio.Queue()
            tailer.subscribe(q)
            for _ in range(20):
                if tailer._f is not None:  # type: ignore[attr-defined]
                    break
                await asyncio.sleep(0.01)
            self.assertIsNotNone(tailer._f)  # type: ignore[attr-defined]

            await streams.close_sse_tailers_under(group_dir)

            self.assertIsNone(tailer._f)  # type: ignore[attr-defined]
            self.assertIsNone(tailer._task)  # type: ignore[attr-defined]
            self.assertNotIn(key, streams._TAILERS)  # type: ignore[attr-defined]
            self.assertTrue(path.exists())

        with tempfile.TemporaryDirectory() as td:
            asyncio.run(_run_case(Path(td) / "groups" / "g-test"))


if __name__ == "__main__":
    unittest.main()
