from cccc.daemon.messaging.actor_turn_rendering import render_delivery_refs


def test_voice_document_ref_is_actionable_workspace_context() -> None:
    assert render_delivery_refs(
        [
            {
                "kind": "voice_document_ref",
                "group_id": "g_local",
                "document_path": "voice/meeting-notes.md",
                "title": "Meeting notes",
            }
        ]
    ) == [
        "[cccc] References:",
        "- Voice document Meeting notes (path=voice/meeting-notes.md, group_id=g_local); "
        "read this workspace-relative file before answering when its contents are needed.",
    ]


def test_voice_document_ref_preserves_a_long_workspace_path() -> None:
    document_path = "voice/" + "nested-directory/" * 10 + "meeting-notes.md"
    assert len(document_path) > 120

    rendered = render_delivery_refs(
        [
            {
                "kind": "voice_document_ref",
                "group_id": "g_local",
                "document_path": document_path,
                "title": "Meeting notes",
            }
        ]
    )

    assert f"path={document_path}, group_id=g_local" in rendered[1]
