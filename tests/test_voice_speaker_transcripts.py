from __future__ import annotations

import unittest

from cccc.daemon.assistants.voice_speaker_transcripts import (
    build_speaker_transcript_segments,
    normalized_speaker_turns,
    slice_pcm16_by_ms,
)


class VoiceSpeakerTranscriptsTests(unittest.IsolatedAsyncioTestCase):
    def test_slice_pcm16_by_ms_uses_sample_boundaries(self) -> None:
        audio = b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(10))

        sliced = slice_pcm16_by_ms(audio, start_ms=200, end_ms=500, sample_rate=10)

        self.assertEqual(sliced, b"".join(int(i).to_bytes(2, "little", signed=True) for i in range(2, 5)))

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


if __name__ == "__main__":
    unittest.main()
