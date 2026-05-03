from __future__ import annotations

import unittest

from cccc.daemon.assistants.voice_speaker_transcript_windows import (
    merge_adjacent_speaker_turns,
    merge_display_transcript_segments,
    normalized_speaker_turns,
)


class VoiceSpeakerTranscriptWindowsTests(unittest.TestCase):
    def test_normalized_speaker_turns_filters_invalid_and_sorts(self) -> None:
        turns = normalized_speaker_turns(
            [
                {"speaker_label": "", "start_ms": 0, "end_ms": 1000},
                {"speaker_label": "Speaker 2", "start_ms": 2000, "end_ms": 3000},
                {"speaker_label": "Speaker 1", "start_ms": 0, "end_ms": 1500},
                {"speaker_label": "Speaker 3", "start_ms": 4000, "end_ms": 3500},
            ]
        )

        self.assertEqual([turn["speaker_label"] for turn in turns], ["Speaker 1", "Speaker 2"])

    def test_merge_adjacent_speaker_turns_keeps_same_speaker_context_together(self) -> None:
        turns = merge_adjacent_speaker_turns(
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 1200},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 1800, "end_ms": 3600},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 3900, "end_ms": 5200},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 9000, "end_ms": 10_000},
            ]
        )

        self.assertEqual(
            [(turn["speaker_label"], turn["start_ms"], turn["end_ms"]) for turn in turns],
            [
                ("Speaker 1", 0, 3600),
                ("Speaker 2", 3900, 5200),
                ("Speaker 1", 9000, 10_000),
            ],
        )

    def test_merge_adjacent_speaker_turns_does_not_merge_overlapping_turns(self) -> None:
        turns = merge_adjacent_speaker_turns(
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 3000},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 2400, "end_ms": 4200},
            ]
        )

        self.assertEqual(len(turns), 2)

    def test_merge_display_transcript_segments_removes_sense_voice_tags_and_joins_text(self) -> None:
        segments = merge_display_transcript_segments(
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 1000, "text": "<|zh|>一个"},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 2400, "end_ms": 3400, "text": "模型"},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 3600, "end_ms": 4500, "text": "next"},
            ]
        )

        self.assertEqual(
            [(segment["speaker_label"], segment["start_ms"], segment["end_ms"], segment["text"]) for segment in segments],
            [
                ("Speaker 1", 0, 3400, "一个模型"),
                ("Speaker 2", 3600, 4500, "next"),
            ],
        )

    def test_merge_display_transcript_segments_does_not_merge_overlapping_rows(self) -> None:
        segments = merge_display_transcript_segments(
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 3000, "text": "first"},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 2500, "end_ms": 5000, "text": "second"},
            ]
        )

        self.assertEqual([segment["text"] for segment in segments], ["first", "second"])


if __name__ == "__main__":
    unittest.main()
