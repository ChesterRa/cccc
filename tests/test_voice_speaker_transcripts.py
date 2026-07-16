from __future__ import annotations

import unittest
from unittest.mock import AsyncMock, patch

from cccc.daemon.assistants.voice_speaker_transcripts import (
    build_offline_speaker_transcript_segments,
    build_speaker_transcript_segments,
)
from cccc.daemon.assistants.voice_speaker_transcript_windows import slice_pcm16_by_ms


class VoiceSpeakerTranscriptsTests(unittest.IsolatedAsyncioTestCase):
    def test_slice_pcm16_by_ms_uses_sample_boundaries(self) -> None:
        audio = b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(10))

        sliced = slice_pcm16_by_ms(audio, start_ms=200, end_ms=500, sample_rate=10)

        self.assertEqual(sliced, b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(2, 5)))

    async def test_build_speaker_transcript_segments_transcribes_each_turn(self) -> None:
        audio = b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(80))
        calls: list[bytes] = []

        async def transcribe(chunk: bytes, sample_rate: int) -> str:
            self.assertEqual(sample_rate, 10)
            calls.append(chunk)
            return f"text-{len(calls)}"

        segments = await build_speaker_transcript_segments(
            audio,
            [
                {"speaker_label": "Speaker 1", "start_ms": 0, "end_ms": 1000},
                {"speaker_label": "Speaker 2", "start_ms": 1000, "end_ms": 2000},
            ],
            sample_rate=10,
            transcribe_segment=transcribe,
        )

        self.assertEqual([segment["speaker_label"] for segment in segments], ["Speaker 1", "Speaker 2"])
        self.assertEqual([segment["text"] for segment in segments], ["text-1", "text-2"])
        self.assertEqual(len(calls), 2)

    async def test_build_speaker_transcript_segments_merges_same_speaker_turns_before_asr(self) -> None:
        audio = b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(80))
        calls: list[bytes] = []

        async def transcribe(chunk: bytes, sample_rate: int) -> str:
            calls.append(chunk)
            return "merged text"

        segments = await build_speaker_transcript_segments(
            audio,
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 1000},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 1600, "end_ms": 2500},
            ],
            sample_rate=10,
            transcribe_segment=transcribe,
        )

        self.assertEqual(len(calls), 1)
        self.assertEqual(segments[0]["start_ms"], 0)
        self.assertEqual(segments[0]["end_ms"], 2500)
        self.assertEqual(segments[0]["text"], "merged text")

    async def test_build_speaker_transcript_segments_merges_display_neighbors_after_asr(self) -> None:
        audio = b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(120))
        texts = iter(["hello", "world"])

        async def transcribe(chunk: bytes, sample_rate: int) -> str:
            return next(texts)

        segments = await build_speaker_transcript_segments(
            audio,
            [
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 0, "end_ms": 1000},
                {"speaker_label": "Speaker 1", "speaker_index": 0, "start_ms": 3000, "end_ms": 5200},
            ],
            sample_rate=10,
            transcribe_segment=transcribe,
        )

        self.assertEqual(len(segments), 1)
        self.assertEqual(segments[0]["text"], "hello world")
        self.assertEqual(segments[0]["start_ms"], 0)
        self.assertEqual(segments[0]["end_ms"], 5200)

    async def test_build_offline_speaker_transcript_segments_reuses_one_offline_session(self) -> None:
        audio = b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(80))
        session = AsyncMock()
        session.transcribe_pcm16.side_effect = ["offline-1", "offline-2"]

        with patch(
            "cccc.daemon.assistants.voice_speaker_transcripts.open_local_offline_asr_session",
            AsyncMock(return_value=session),
        ) as open_session:
            segments = await build_offline_speaker_transcript_segments(
                audio,
                [
                    {"speaker_label": "Speaker 1", "start_ms": 0, "end_ms": 1000},
                    {"speaker_label": "Speaker 2", "start_ms": 1000, "end_ms": 2000},
                ],
                selected_model_id="final_model",
                sample_rate=10,
            )

        open_session.assert_awaited_once_with("final_model", sample_rate=10)
        self.assertEqual(session.transcribe_pcm16.await_count, 2)
        session.close.assert_awaited_once()
        self.assertEqual([segment["text"] for segment in segments], ["offline-1", "offline-2"])


if __name__ == "__main__":
    unittest.main()
