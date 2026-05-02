from __future__ import annotations

import unittest

from cccc.daemon.assistants.voice_speaker_identity import remap_speaker_embeddings, stabilize_diarization_speaker_identity


class VoiceSpeakerIdentityTests(unittest.TestCase):
    def test_stabilize_diarization_speaker_identity_uses_previous_overlap(self) -> None:
        previous = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 4000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4200, "end_ms": 8000},
        ]
        current = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 4000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4200, "end_ms": 8000},
            {"speaker_label": "Speaker 3", "speaker_index": 2, "start_ms": 8200, "end_ms": 12000},
        ]

        segments = stabilize_diarization_speaker_identity(current, previous)

        self.assertEqual([item["speaker_index"] for item in segments], [0, 1, 2])

    def test_stabilize_diarization_speaker_identity_remaps_flipped_local_labels(self) -> None:
        previous = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 4000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4200, "end_ms": 8000},
        ]
        current = [
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 0, "end_ms": 4000},
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 4200, "end_ms": 8000},
        ]

        segments = stabilize_diarization_speaker_identity(current, previous)

        self.assertEqual([item["speaker_index"] for item in segments], [0, 1])
        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 1", "Speaker 2"])

    def test_stabilize_diarization_speaker_identity_prefers_embedding_match(self) -> None:
        previous = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 4000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4200, "end_ms": 8000},
        ]
        current = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 4000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 4200, "end_ms": 8000},
        ]

        segments = stabilize_diarization_speaker_identity(
            current,
            previous,
            speaker_embeddings=[
                {"speaker_index": 0, "embedding": [0.0, 1.0]},
                {"speaker_index": 1, "embedding": [1.0, 0.0]},
            ],
            previous_speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.0, 1.0]},
            ],
        )

        self.assertEqual([item["speaker_index"] for item in segments], [1, 0])
        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 2", "Speaker 1"])

    def test_remap_speaker_embeddings_uses_global_assignments(self) -> None:
        embeddings = [
            {"speaker_index": 0, "embedding": [0.0, 1.0]},
            {"speaker_index": 1, "embedding": [1.0, 0.0]},
        ]

        remapped = remap_speaker_embeddings(embeddings, {"0": 1, "1": 0})

        self.assertEqual([item["speaker_index"] for item in remapped], [1, 0])
        self.assertEqual([item["speaker_label"] for item in remapped], ["Speaker 2", "Speaker 1"])


if __name__ == "__main__":
    unittest.main()
