import unittest
from pathlib import Path

from cccc.ports.mcp.utils.help_markdown import (
    _select_help_markdown,
    build_help_markdown,
    parse_help_markdown,
    update_actor_help_note,
)

_FIXTURE = Path(__file__).resolve().parent / "fixtures" / "help_markdown_legacy_tag_roundtrip.md"


class TestHelpMarkdownLegacyTags(unittest.TestCase):
    def test_parse_help_markdown_preserves_legacy_pet_block_as_extra(self) -> None:
        markdown = """
Shared guidance.

## @pet

Legacy note.

## @actor: peer-1

Actor note.
""".strip()

        parsed = parse_help_markdown(markdown)
        self.assertNotIn("pet", parsed)
        self.assertEqual(list(parsed.get("extra_tagged_blocks") or []), ["## @pet\n\nLegacy note."])
        actor_notes = parsed.get("actor_notes") if isinstance(parsed.get("actor_notes"), dict) else {}
        self.assertEqual(str(actor_notes.get("peer-1") or ""), "Actor note.")

    def test_update_actor_help_note_preserves_legacy_pet_block(self) -> None:
        markdown = """
## @pet

Legacy note.

## @actor: peer-1

Old actor note.
""".strip()

        updated = update_actor_help_note(markdown, "peer-1", "New actor note.", ["peer-1"])
        parsed = parse_help_markdown(updated)
        actor_notes = parsed.get("actor_notes") if isinstance(parsed.get("actor_notes"), dict) else {}

        self.assertEqual(list(parsed.get("extra_tagged_blocks") or []), ["## @pet\n\nLegacy note."])
        self.assertEqual(str(actor_notes.get("peer-1") or ""), "New actor note.")

    def test_parse_help_markdown_recovers_inline_actor_note_text(self) -> None:
        markdown = """
## @actor: peer-1 first line
second line
""".strip()

        parsed = parse_help_markdown(markdown)
        actor_notes = parsed.get("actor_notes") if isinstance(parsed.get("actor_notes"), dict) else {}

        self.assertEqual(str(actor_notes.get("peer-1") or ""), "first line\nsecond line")

    def test_select_help_markdown_hides_legacy_pet_block(self) -> None:
        markdown = """
Common guidance.

## @pet

Legacy note.

## @actor: peer-1

Actor-only note.
""".strip()

        selected = _select_help_markdown(markdown, role="peer", actor_id="peer-1")
        self.assertIn("Common guidance.", selected)
        self.assertIn("Actor-only note.", selected)
        self.assertNotIn("Legacy note.", selected)

    def test_select_help_markdown_can_include_voice_secretary_block(self) -> None:
        markdown = """
Common guidance.

## @voice_secretary

Voice-only operating contract.

## @actor: voice-secretary

Actor note.
""".strip()

        normal = _select_help_markdown(markdown, role="peer", actor_id="voice-secretary")
        selected = _select_help_markdown(
            markdown,
            role="peer",
            actor_id="voice-secretary",
            include_voice_secretary=True,
        )

        self.assertIn("Common guidance.", normal)
        self.assertNotIn("Voice-only operating contract.", normal)
        self.assertIn("Voice-only operating contract.", selected)
        self.assertIn("Actor note.", selected)

    def test_voice_secretary_selector_hides_peer_role_block(self) -> None:
        markdown = """
Common guidance.

## @role: peer

Peer-only workflow.

## @voice_secretary

Voice-only operating contract.
""".strip()

        selected = _select_help_markdown(
            markdown,
            role="voice_secretary",
            actor_id="voice-secretary",
            include_voice_secretary=True,
        )

        self.assertIn("Common guidance.", selected)
        self.assertIn("Voice-only operating contract.", selected)
        self.assertNotIn("Peer-only workflow.", selected)

    def test_roundtrip_fixture_matches_expected_shape(self) -> None:
        markdown = _FIXTURE.read_text(encoding="utf-8")

        parsed = parse_help_markdown(markdown)
        self.assertEqual(str(parsed.get("common") or ""), "Shared guidance.")
        self.assertEqual(str(parsed.get("foreman") or ""), "Foreman note.")
        self.assertEqual(str(parsed.get("peer") or ""), "Peer note.")
        actor_notes = parsed.get("actor_notes") if isinstance(parsed.get("actor_notes"), dict) else {}
        self.assertEqual(str(actor_notes.get("peer-1") or ""), "Actor note.")
        self.assertEqual(str(actor_notes.get("reviewer-1") or ""), "Reviewer note.")
        self.assertEqual(
            list(parsed.get("extra_tagged_blocks") or []),
            ["## @pet\n\nLegacy note.", "## @role: observer\n\nObserver note."],
        )

        rebuilt = build_help_markdown(
            common=str(parsed.get("common") or ""),
            foreman=str(parsed.get("foreman") or ""),
            peer=str(parsed.get("peer") or ""),
            actor_notes=actor_notes,
            actor_order=["peer-1", "reviewer-1"],
            extra_tagged_blocks=list(parsed.get("extra_tagged_blocks") or []),
        )
        self.assertEqual(rebuilt, markdown)


if __name__ == "__main__":
    unittest.main()
