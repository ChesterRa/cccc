from __future__ import annotations

from cccc.ports.im.adapters.outbound_text import (
    mark_stream_updated,
    split_text_chunks,
    stream_preview,
    stream_update_due,
    utf8_stream_preview,
)


def test_split_text_chunks_is_unicode_safe_and_lossless() -> None:
    text = "你好🙂abcdef"
    chunks = split_text_chunks(text, max_chars=3, hard_limit=10)

    assert chunks == ["你好🙂", "abc", "def"]
    assert "".join(chunks) == text


def test_split_text_chunks_preserves_newlines_at_line_boundaries() -> None:
    text = "one\ntwo\nthree\nfour\n"
    chunks = split_text_chunks(
        text,
        max_chars=100,
        hard_limit=100,
        max_lines=2,
    )

    assert all(len(chunk.split("\n")) <= 2 for chunk in chunks)
    assert "".join(chunks) == text


def test_stream_preview_only_reports_exact_content_when_it_fits() -> None:
    assert stream_preview("hello", max_chars=5, hard_limit=10) == ("hello", True)

    preview, exact = stream_preview("hello!", max_chars=5, hard_limit=10)
    assert preview == "hell…"
    assert exact is False


def test_utf8_stream_preview_respects_byte_limit() -> None:
    preview, exact = utf8_stream_preview("你" * 10, max_bytes=10)

    assert exact is False
    assert len(preview.encode("utf-8")) <= 10
    assert "�" not in preview


def test_stream_update_throttle_uses_mutable_platform_handle() -> None:
    handle = {"platform_handle": {}}

    assert stream_update_due(handle, interval=1.0) is True
    mark_stream_updated(handle)
    assert stream_update_due(handle, interval=1.0) is False
