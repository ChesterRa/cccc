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
