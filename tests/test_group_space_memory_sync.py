from __future__ import annotations

import os
import tempfile
import unittest


class TestGroupSpaceMemorySync(unittest.TestCase):
    def _with_home(self, *, stub: bool = True, max_words: str = ""):
        old_home = os.environ.get("CCCC_HOME")
        old_stub = os.environ.get("CCCC_NOTEBOOKLM_STUB")
        old_max_words = os.environ.get("CCCC_SPACE_MEMORY_SOURCE_MAX_WORDS")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td
        if stub:
            os.environ["CCCC_NOTEBOOKLM_STUB"] = "1"
        else:
            os.environ.pop("CCCC_NOTEBOOKLM_STUB", None)
        if max_words:
            os.environ["CCCC_SPACE_MEMORY_SOURCE_MAX_WORDS"] = str(max_words)
        else:
            os.environ.pop("CCCC_SPACE_MEMORY_SOURCE_MAX_WORDS", None)

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_stub is None:
                os.environ.pop("CCCC_NOTEBOOKLM_STUB", None)
            else:
                os.environ["CCCC_NOTEBOOKLM_STUB"] = old_stub
            if old_max_words is None:
                os.environ.pop("CCCC_SPACE_MEMORY_SOURCE_MAX_WORDS", None)
            else:
                os.environ["CCCC_SPACE_MEMORY_SOURCE_MAX_WORDS"] = old_max_words

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group(self, title: str = "memory-space") -> str:
        create, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        gid = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _append_daily_entry(self, group_id: str, *, date: str, summary: str, entry_id: str) -> None:
        from cccc.kernel.memory_reme.writer import append_daily_entry, build_memory_entry
        from cccc.kernel.memory_reme.layout import resolve_memory_layout

        layout = resolve_memory_layout(group_id, date=date, ensure_files=True)
        entry = build_memory_entry(
            group_label=layout.group_label,
            kind="note",
            summary=summary,
            actor_id="tester",
            entry_id=entry_id,
            date=date,
        )
        out = append_daily_entry(group_id, entry=entry, date=date, idempotency_key=f"{date}:{entry_id}")
        self.assertEqual(str(out.get("status") or ""), "written")

    def test_memory_lane_sync_executes_daily_job_and_records_manifest(self) -> None:
        from cccc.daemon.space.group_space_memory_sync import read_memory_notebooklm_sync_state
        from cccc.daemon.space.group_space_runtime import process_due_space_jobs

        _, cleanup = self._with_home(stub=True)
        try:
            gid = self._create_group("memory-sync-success")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "bind",
                    "remote_space_id": "nb_memory_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))
            self._append_daily_entry(
                gid,
                date="2026-03-01",
                summary="A stable decision was made about the integration path.",
                entry_id="mem_sync_1",
            )

            sync, _ = self._call(
                "group_space_sync",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "run",
                    "force": False,
                    "by": "user",
                },
            )
            self.assertTrue(sync.ok, getattr(sync, "error", None))
            self.assertGreaterEqual(int(((sync.result or {}).get("sync_result") or {}).get("queued") or 0), 1)

            tick = process_due_space_jobs(limit=10)
            self.assertGreaterEqual(int(tick.get("processed") or 0), 1)

            state = read_memory_notebooklm_sync_state(gid, remote_space_id="nb_memory_1")
            files = state.get("files") if isinstance(state.get("files"), dict) else {}
            item = files.get("2026-03-01") if isinstance(files.get("2026-03-01"), dict) else {}
            self.assertEqual(str(item.get("state") or ""), "succeeded")
            self.assertEqual(str(item.get("source_strategy") or ""), "single")
            self.assertEqual(int(item.get("part_count") or 0), 1)
            self.assertEqual(len(item.get("source_ids") or []), 1)
            self.assertEqual(int(state.get("eligible_daily_files") or -1), 1)
            self.assertEqual(int(state.get("synced_daily_files") or -1), 1)
            self.assertEqual(int(state.get("empty_daily_skipped", -1)), 0)
            self.assertEqual(str(state.get("last_eligible_daily_date") or ""), "2026-03-01")
            self.assertEqual(str(state.get("last_synced_daily_date") or ""), "2026-03-01")
        finally:
            cleanup()

    def test_memory_lane_split_upload_when_daily_file_exceeds_word_budget(self) -> None:
        from cccc.daemon.space.group_space_memory_sync import read_memory_notebooklm_sync_state
        from cccc.daemon.space.group_space_runtime import process_due_space_jobs

        _, cleanup = self._with_home(stub=True, max_words="80")
        try:
            gid = self._create_group("memory-sync-split")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "bind",
                    "remote_space_id": "nb_memory_2",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))
            self._append_daily_entry(
                gid,
                date="2026-03-02",
                summary="one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen",
                entry_id="mem_split_1",
            )
            self._append_daily_entry(
                gid,
                date="2026-03-02",
                summary="alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma",
                entry_id="mem_split_2",
            )

            sync, _ = self._call(
                "group_space_sync",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "run",
                    "force": False,
                    "by": "user",
                },
            )
            self.assertTrue(sync.ok, getattr(sync, "error", None))
            tick = process_due_space_jobs(limit=10)
            self.assertGreaterEqual(int(tick.get("processed") or 0), 1)

            state = read_memory_notebooklm_sync_state(gid, remote_space_id="nb_memory_2")
            files = state.get("files") if isinstance(state.get("files"), dict) else {}
            item = files.get("2026-03-02") if isinstance(files.get("2026-03-02"), dict) else {}
            self.assertEqual(str(item.get("state") or ""), "succeeded")
            self.assertEqual(str(item.get("source_strategy") or ""), "split")
            self.assertEqual(int(item.get("part_count") or 0), 2)
            self.assertEqual(len(item.get("source_ids") or []), 2)
        finally:
            cleanup()

    def test_memory_lane_skips_empty_daily_file_and_reports_coverage(self) -> None:
        from cccc.daemon.space.group_space_memory_sync import read_memory_notebooklm_sync_state
        from cccc.kernel.memory_reme.layout import resolve_memory_layout

        _, cleanup = self._with_home(stub=True)
        try:
            gid = self._create_group("memory-sync-empty-skip")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "bind",
                    "remote_space_id": "nb_memory_empty_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))
            resolve_memory_layout(gid, date="2026-03-04", ensure_files=True)

            sync, _ = self._call(
                "group_space_sync",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "run",
                    "force": False,
                    "by": "user",
                },
            )
            self.assertTrue(sync.ok, getattr(sync, "error", None))
            sync_result = (sync.result or {}).get("sync_result") if isinstance(sync.result, dict) else {}
            self.assertEqual(int((sync_result or {}).get("queued") or 0), 0)
            self.assertEqual(int((sync_result or {}).get("empty_daily_skipped") or 0), 1)

            state = read_memory_notebooklm_sync_state(gid, remote_space_id="nb_memory_empty_1")
            files = state.get("files") if isinstance(state.get("files"), dict) else {}
            item = files.get("2026-03-04") if isinstance(files.get("2026-03-04"), dict) else {}
            self.assertEqual(str(item.get("state") or ""), "skipped_empty")
            self.assertEqual(int(item.get("entry_count", -1)), 0)
            self.assertEqual(int(state.get("eligible_daily_files", -1)), 0)
            self.assertEqual(int(state.get("synced_daily_files", -1)), 0)
            self.assertEqual(int(state.get("empty_daily_skipped") or -1), 1)
            self.assertEqual(str(state.get("last_eligible_daily_date") or ""), "")
            self.assertEqual(str(state.get("last_synced_daily_date") or ""), "")
        finally:
            cleanup()

    def test_memory_lane_skips_today_file(self) -> None:
        from cccc.daemon.space.group_space_memory_sync import read_memory_notebooklm_sync_state
        from cccc.util.time import utc_now_iso

        _, cleanup = self._with_home(stub=True)
        try:
            gid = self._create_group("memory-sync-today-skip")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "bind",
                    "remote_space_id": "nb_memory_3",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))
            today = utc_now_iso()[:10]
            self._append_daily_entry(
                gid,
                date=today,
                summary="today should stay local until the day closes",
                entry_id="mem_today_1",
            )
            sync, _ = self._call(
                "group_space_sync",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "run",
                    "force": False,
                    "by": "user",
                },
            )
            self.assertTrue(sync.ok, getattr(sync, "error", None))
            sync_result = (sync.result or {}).get("sync_result") if isinstance(sync.result, dict) else {}
            self.assertEqual(int((sync_result or {}).get("queued") or 0), 0)
            state = read_memory_notebooklm_sync_state(gid, remote_space_id="nb_memory_3")
            files = state.get("files") if isinstance(state.get("files"), dict) else {}
            self.assertNotIn(today, files)
        finally:
            cleanup()

    def test_periodic_memory_sync_skips_binding_for_deleted_group(self) -> None:
        from unittest.mock import patch

        from cccc.daemon.space.group_space_memory_sync import process_due_memory_space_syncs

        binding = {
            "group_id": "g_deleted",
            "provider": "notebooklm",
            "lane": "memory",
            "remote_space_id": "nb_stale",
            "status": "bound",
        }
        with (
            patch(
                "cccc.daemon.space.group_space_memory_sync.list_space_bindings",
                return_value=[binding],
            ),
            patch("cccc.daemon.space.group_space_memory_sync.load_group", return_value=None),
            patch("cccc.daemon.space.group_space_memory_sync.sync_memory_daily_files") as sync_mock,
        ):
            result = process_due_memory_space_syncs(provider="notebooklm", limit=20)

        self.assertEqual(result.get("processed"), 0)
        self.assertEqual(result.get("queued"), 0)
        self.assertEqual(result.get("blocked"), 0)
        self.assertEqual(result.get("missing_groups"), 1)
        sync_mock.assert_not_called()
