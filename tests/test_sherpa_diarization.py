from __future__ import annotations

import unittest

from cccc.daemon.assistants.sherpa_diarization import normalize_diarization_segments, resolve_diarization_num_speakers


class SherpaDiarizationTests(unittest.TestCase):
    def test_resolve_diarization_num_speakers_keeps_auto_detection(self) -> None:
        self.assertEqual(resolve_diarization_num_speakers(-1), -1)
        self.assertEqual(resolve_diarization_num_speakers(""), -1)
        self.assertEqual(resolve_diarization_num_speakers(3), 3)

    def test_normalize_diarization_segments_relabels_and_merges_for_meeting_display(self) -> None:
        segments = normalize_diarization_segments(
            [
                {"speaker_label": "Speaker 6", "speaker_index": 5, "start_ms": 0, "end_ms": 100},
                {"speaker_label": "Speaker 6", "speaker_index": 5, "start_ms": 1000, "end_ms": 1800},
                {"speaker_label": "Speaker 6", "speaker_index": 5, "start_ms": 1900, "end_ms": 2500},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 3000, "end_ms": 3700},
            ]
        )

        self.assertEqual(
            segments,
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 1000, "end_ms": 2500},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 3000, "end_ms": 3700},
            ],
        )

    def test_normalize_diarization_segments_absorbs_short_spurious_speaker_cluster(self) -> None:
        segments = normalize_diarization_segments(
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 1800},
                {"speaker_label": "Speaker 9", "speaker_index": 8, "start_ms": 1800, "end_ms": 2300},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 2300, "end_ms": 4200},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4300, "end_ms": 6200},
            ]
        )

        self.assertEqual(
            segments,
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 4200},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4300, "end_ms": 6200},
            ],
        )

    def test_normalize_diarization_segments_keeps_long_auto_detected_third_speaker(self) -> None:
        segments = normalize_diarization_segments(
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 1800},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 1900, "end_ms": 3800},
                {"speaker_label": "Speaker 3", "speaker_index": 2, "start_ms": 3900, "end_ms": 5700},
            ]
        )

        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 1", "Speaker 2", "Speaker 3"])

    def test_normalize_diarization_segments_keeps_real_short_two_speaker_turns(self) -> None:
        segments = normalize_diarization_segments(
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 4200},
                {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4300, "end_ms": 8200},
            ]
        )

        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 1", "Speaker 2"])


if __name__ == "__main__":
    unittest.main()
