from __future__ import annotations

import asyncio
import unittest
from unittest.mock import patch

from cccc.providers.notebooklm.adapter import _ingest_async
from cccc.providers.notebooklm.errors import NotebookLMProviderError


class _FakeSource:
    def __init__(self, source_id: str, title: str) -> None:
        self.id = source_id
        self.title = title


class _FakeSources:
    def __init__(self) -> None:
        self.called: str = ""
        self.kwargs: dict = {}

    async def add_url(self, notebook_id: str, url: str, wait: bool = False):
        self.called = "add_url"
        self.kwargs = {"notebook_id": notebook_id, "url": url, "wait": wait}
        return _FakeSource("src_url_1", "URL Source")

    async def add_text(self, notebook_id: str, title: str, content: str, wait: bool = False):
        self.called = "add_text"
        self.kwargs = {
            "notebook_id": notebook_id,
            "title": title,
            "content": content,
            "wait": wait,
        }
        return _FakeSource("src_text_1", title)

    async def add_drive(
        self,
        notebook_id: str,
        file_id: str,
        title: str,
        mime_type: str,
        wait: bool = False,
    ):
        self.called = "add_drive"
        self.kwargs = {
            "notebook_id": notebook_id,
            "file_id": file_id,
            "title": title,
            "mime_type": mime_type,
            "wait": wait,
        }
        return _FakeSource("src_drive_1", title)


class _FakeClient:
    def __init__(self, sources: _FakeSources) -> None:
        self.sources = sources

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False


class TestNotebookLMResourceIngestModes(unittest.TestCase):
    def _run_ingest(self, payload: dict):
        fake_sources = _FakeSources()

        async def _fake_build_client(*, auth_payload, timeout_seconds):
            _ = auth_payload, timeout_seconds
            return _FakeClient(fake_sources)

        with patch("cccc.providers.notebooklm.adapter._build_client", side_effect=_fake_build_client):
            out = asyncio.run(
                _ingest_async(
                    notebook_id="nb_test_1",
                    kind="resource_ingest",
                    payload=payload,
                    auth_payload={},
                    timeout_seconds=10.0,
                )
            )
        return out, fake_sources

    def test_resource_ingest_web_page_uses_add_url(self) -> None:
        out, fake_sources = self._run_ingest(
            {
                "source_type": "web_page",
                "title": "Spec",
                "url": "https://example.com/spec",
            }
        )
        self.assertEqual(fake_sources.called, "add_url")
        self.assertEqual(str(fake_sources.kwargs.get("url") or ""), "https://example.com/spec")
        self.assertEqual(str(out.get("source_mode") or ""), "web_page")

    def test_resource_ingest_youtube_is_auto_inferred_from_url(self) -> None:
        out, fake_sources = self._run_ingest(
            {
                "title": "Demo Video",
                "url": "https://www.youtube.com/watch?v=abc123",
            }
        )
        self.assertEqual(fake_sources.called, "add_url")
        self.assertEqual(str(out.get("source_mode") or ""), "youtube")

    def test_resource_ingest_pasted_text_uses_add_text(self) -> None:
        out, fake_sources = self._run_ingest(
            {
                "source_type": "pasted_text",
                "title": "Meeting Notes",
                "content": "Line1\nLine2",
            }
        )
        self.assertEqual(fake_sources.called, "add_text")
        self.assertEqual(str(fake_sources.kwargs.get("title") or ""), "Meeting Notes")
        self.assertIn("Line1", str(fake_sources.kwargs.get("content") or ""))
        self.assertEqual(str(out.get("source_mode") or ""), "pasted_text")

    def test_resource_ingest_google_docs_uses_add_drive(self) -> None:
        out, fake_sources = self._run_ingest(
            {
                "source_type": "google_docs",
                "title": "Roadmap",
                "file_id": "doc_123",
            }
        )
        self.assertEqual(fake_sources.called, "add_drive")
        self.assertEqual(str(fake_sources.kwargs.get("file_id") or ""), "doc_123")
        self.assertEqual(
            str(fake_sources.kwargs.get("mime_type") or ""),
            "application/vnd.google-apps.document",
        )
        self.assertEqual(str(out.get("source_mode") or ""), "google_docs")

    def test_resource_ingest_google_docs_without_file_id_rejected(self) -> None:
        async def _fake_build_client(*, auth_payload, timeout_seconds):
            _ = auth_payload, timeout_seconds
            return _FakeClient(_FakeSources())

        with patch("cccc.providers.notebooklm.adapter._build_client", side_effect=_fake_build_client):
            with self.assertRaises(NotebookLMProviderError) as ctx:
                asyncio.run(
                    _ingest_async(
                        notebook_id="nb_test_1",
                        kind="resource_ingest",
                        payload={"source_type": "google_docs", "title": "No ID"},
                        auth_payload={},
                        timeout_seconds=10.0,
                    )
                )
        self.assertEqual(ctx.exception.code, "space_job_invalid")
        self.assertIn("file_id is required", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
