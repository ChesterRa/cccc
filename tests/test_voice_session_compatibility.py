from cccc.daemon.assistants.assistant_ops import _public_voice_session


def test_public_voice_session_reads_legacy_window_segments() -> None:
    session = _public_voice_session(
        {
            "session_id": "python-session",
            "document_path": "docs/voice.md",
            "window_segments": [{"segment_id": "one", "text": "hello"}],
        },
        document_path="docs/voice.md",
    )

    assert session["capture_mode"] == "document"
    assert [segment["text"] for segment in session["segments"]] == ["hello"]
    assert session["transcript"] == "hello"


def test_public_voice_session_filters_segments_for_the_requested_document() -> None:
    session = _public_voice_session(
        {
            "session_id": "python-shared-session",
            "capture_mode": "document",
            "document_path": "docs/b.md",
            "segments": [
                {"segment_id": "a", "document_path": "docs/a.md", "text": "alpha"},
                {"segment_id": "b", "document_path": "docs/b.md", "text": "bravo"},
            ],
        },
        document_path="docs/b.md",
    )

    assert session["document_path"] == "docs/b.md"
    assert [segment["segment_id"] for segment in session["segments"]] == ["b"]
    assert session["transcript"] == "bravo"
