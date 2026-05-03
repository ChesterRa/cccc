from __future__ import annotations

import asyncio
from pathlib import Path
import unittest
from unittest.mock import AsyncMock, patch

from cccc.daemon.assistants.voice_speaker_identity import (
    remap_speaker_embeddings,
    run_final_diarization_file,
    run_provisional_diarization_window,
    stabilize_diarization_speaker_identity,
)


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

    def test_stabilize_diarization_speaker_identity_merges_same_run_split_clusters(self) -> None:
        current = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 3000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 3200, "end_ms": 6200},
            {"speaker_label": "Speaker 3", "speaker_index": 2, "start_ms": 6400, "end_ms": 9400},
        ]

        segments = stabilize_diarization_speaker_identity(
            current,
            [],
            speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.98, 0.02]},
                {"speaker_index": 2, "embedding": [0.0, 1.0]},
            ],
        )

        self.assertEqual([item["speaker_index"] for item in segments], [0, 1])
        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 1", "Speaker 2"])
        self.assertEqual((segments[0]["start_ms"], segments[0]["end_ms"]), (0, 6200))

    def test_stabilize_diarization_speaker_identity_keeps_short_new_cluster_candidate_unpromoted(self) -> None:
        previous = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 5000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 5200, "end_ms": 10000},
        ]
        current = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 5000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 5200, "end_ms": 10000},
            {"speaker_label": "Speaker 3", "speaker_index": 2, "start_ms": 10200, "end_ms": 11800},
        ]

        segments = stabilize_diarization_speaker_identity(
            current,
            previous,
            speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.0, 1.0]},
                {"speaker_index": 2, "embedding": [0.55, 0.45]},
            ],
            previous_speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.0, 1.0]},
            ],
        )

        self.assertEqual([item["speaker_index"] for item in segments], [0, 1, 0])
        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 1", "Speaker 2", "Speaker 1"])

    def test_stabilize_diarization_speaker_identity_suppresses_short_low_similarity_candidate(self) -> None:
        previous = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 5000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 5200, "end_ms": 10000},
        ]
        current = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 5000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 5200, "end_ms": 10000},
            {"speaker_label": "Speaker 3", "speaker_index": 2, "start_ms": 10200, "end_ms": 11800},
        ]

        segments = stabilize_diarization_speaker_identity(
            current,
            previous,
            speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.0, 1.0, 0.0]},
                {"speaker_index": 2, "embedding": [0.0, 0.0, 1.0]},
            ],
            previous_speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.0, 1.0, 0.0]},
            ],
        )

        self.assertEqual([item["speaker_index"] for item in segments], [0, 1, 0])

    def test_stabilize_diarization_speaker_identity_suppresses_short_initial_candidate(self) -> None:
        current = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 7000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 7200, "end_ms": 14_000},
            {"speaker_label": "Speaker 4", "speaker_index": 3, "start_ms": 14_200, "end_ms": 16_000},
        ]

        segments = stabilize_diarization_speaker_identity(
            current,
            [],
            speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.0, 1.0, 0.0]},
                {"speaker_index": 3, "embedding": [0.0, 0.0, 1.0]},
            ],
        )

        self.assertEqual([item["speaker_index"] for item in segments], [0, 1])
        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 1", "Speaker 2"])
        self.assertEqual((segments[1]["start_ms"], segments[1]["end_ms"]), (7200, 16_000))

    def test_stabilize_diarization_speaker_identity_reuses_missing_display_index_for_new_speaker(self) -> None:
        previous = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 5000},
            {"speaker_label": "Speaker 4", "speaker_index": 3, "start_ms": 5200, "end_ms": 6200},
        ]
        current = [
            {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 5000},
            {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 7000, "end_ms": 14_000},
        ]

        segments = stabilize_diarization_speaker_identity(
            current,
            previous,
            speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0, 0.0]},
                {"speaker_index": 1, "embedding": [0.0, 1.0, 0.0]},
            ],
            previous_speaker_embeddings=[
                {"speaker_index": 0, "embedding": [1.0, 0.0, 0.0]},
                {"speaker_index": 3, "embedding": [0.0, 0.0, 1.0]},
            ],
        )

        self.assertEqual([item["speaker_index"] for item in segments], [0, 1])
        self.assertEqual([item["speaker_label"] for item in segments], ["Speaker 1", "Speaker 2"])

    def test_remap_speaker_embeddings_uses_global_assignments(self) -> None:
        embeddings = [
            {"speaker_index": 0, "embedding": [0.0, 1.0]},
            {"speaker_index": 1, "embedding": [1.0, 0.0]},
        ]

        remapped = remap_speaker_embeddings(embeddings, {"0": 1, "1": 0})

        self.assertEqual([item["speaker_index"] for item in remapped], [0, 1])
        self.assertEqual([item["speaker_label"] for item in remapped], ["Speaker 1", "Speaker 2"])

    def test_remap_speaker_embeddings_merges_same_global_speaker(self) -> None:
        embeddings = [
            {"speaker_index": 0, "embedding": [1.0, 0.0], "duration_ms": 3000, "sample_count": 1},
            {"speaker_index": 1, "embedding": [0.98, 0.02], "duration_ms": 1000, "sample_count": 1},
        ]

        remapped = remap_speaker_embeddings(embeddings, {"0": 0, "1": 0})

        self.assertEqual(len(remapped), 1)
        self.assertEqual(remapped[0]["speaker_index"], 0)
        self.assertEqual(remapped[0]["speaker_label"], "Speaker 1")
        self.assertEqual(remapped[0]["duration_ms"], 4000)
        self.assertEqual(remapped[0]["sample_count"], 2)

    def test_run_final_diarization_file_applies_identity_stabilization(self) -> None:
        async def _run() -> dict:
            with patch(
                "cccc.daemon.assistants.voice_speaker_identity.run_sherpa_diarization_file",
                new=AsyncMock(return_value={
                    "segments": [
                        {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 3000},
                        {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 3200, "end_ms": 6200},
                    ],
                    "speaker_embeddings": [
                        {"speaker_index": 0, "embedding": [1.0, 0.0], "duration_ms": 3000, "sample_count": 1},
                        {"speaker_index": 1, "embedding": [0.98, 0.02], "duration_ms": 3000, "sample_count": 1},
                    ],
                }),
            ) as mocked:
                result = await run_final_diarization_file(
                    Path("demo.pcm16"),
                    selected_model_id="model",
                    sample_rate=16000,
                )
                mocked.assert_awaited_once()
                self.assertTrue(mocked.await_args.kwargs.get("include_speaker_embeddings"))
                return result

        result = asyncio.run(_run())

        self.assertFalse(result["provisional"])
        self.assertEqual(len(result["segments"]), 1)
        self.assertEqual(result["segments"][0]["speaker_index"], 0)
        self.assertEqual(len(result["speaker_embeddings"]), 1)
        debug = result.get("speaker_identity_debug")
        self.assertIsInstance(debug, dict)
        self.assertEqual(debug.get("kind"), "final")
        self.assertIn("local_cluster_merge", [item.get("type") for item in debug.get("events", [])])

    def test_run_provisional_diarization_window_offsets_and_merges_global_segments(self) -> None:
        async def _run() -> dict:
            with patch(
                "cccc.daemon.assistants.voice_speaker_identity.run_sherpa_diarization",
                new=AsyncMock(return_value={
                    "segments": [
                        {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 3000},
                        {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 3200, "end_ms": 6200},
                    ],
                    "speaker_embeddings": [
                        {"speaker_index": 0, "embedding": [1.0, 0.0], "duration_ms": 3000, "sample_count": 1},
                        {"speaker_index": 1, "embedding": [0.0, 1.0], "duration_ms": 3000, "sample_count": 1},
                    ],
                }),
            ):
                return await run_provisional_diarization_window(
                    b"\0" * 128,
                    selected_model_id="model",
                    sample_rate=16000,
                    run_seq=3,
                    audio_duration_ms=100_000,
                    window_start_ms=40_000,
                    previous_segments=[
                        {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 10_000},
                        {"speaker_label": "Speaker 2", "speaker_index": 1, "start_ms": 39_000, "end_ms": 42_000},
                    ],
                    previous_speaker_embeddings=[
                        {"speaker_index": 0, "embedding": [1.0, 0.0]},
                        {"speaker_index": 1, "embedding": [0.0, 1.0]},
                    ],
                )

        result = asyncio.run(_run())

        self.assertEqual(result["window_start_ms"], 40_000)
        self.assertEqual([item["start_ms"] for item in result["segments"]], [0, 40_000, 43_200])
        self.assertEqual([item["end_ms"] for item in result["segments"]], [10_000, 43_000, 46_200])
        self.assertEqual([item["speaker_index"] for item in result["segments"]], [0, 0, 1])
        debug = result.get("speaker_identity_debug")
        self.assertIsInstance(debug, dict)
        self.assertEqual(debug.get("kind"), "provisional_window")
        self.assertEqual(debug.get("window_start_ms"), 40_000)
        self.assertIn("speaker_matched_embedding", [item.get("type") for item in debug.get("events", [])])

    def test_run_provisional_diarization_window_keeps_trimmed_boundary_history(self) -> None:
        async def _run() -> dict:
            with patch(
                "cccc.daemon.assistants.voice_speaker_identity.run_sherpa_diarization",
                new=AsyncMock(return_value={
                    "segments": [
                        {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 1000},
                    ],
                    "speaker_embeddings": [
                        {"speaker_index": 0, "embedding": [1.0, 0.0], "duration_ms": 1000, "sample_count": 1},
                    ],
                }),
            ):
                return await run_provisional_diarization_window(
                    b"\0" * 128,
                    selected_model_id="model",
                    sample_rate=16000,
                    run_seq=4,
                    audio_duration_ms=100_000,
                    window_start_ms=40_000,
                    previous_segments=[
                        {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 37_000, "end_ms": 39_000},
                    ],
                    previous_speaker_embeddings=[
                        {"speaker_index": 0, "embedding": [1.0, 0.0]},
                    ],
                )

        result = asyncio.run(_run())

        self.assertEqual([item["start_ms"] for item in result["segments"]], [37_000, 40_000])
        self.assertEqual([item["end_ms"] for item in result["segments"]], [38_500, 41_000])
        self.assertEqual([item["speaker_index"] for item in result["segments"]], [0, 0])


if __name__ == "__main__":
    unittest.main()
