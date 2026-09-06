from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from unittest.mock import patch

import pytest


SCRIPT = (
    Path(__file__).resolve().parents[1] / "scripts/tests/probe_codex_voice_results.py"
)
SPEC = importlib.util.spec_from_file_location("cccc_voice_probe", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)


def complete_events(role="assistant"):
    transcript = " ".join(
        f"The {name} result is {number}." for name, number in PROBE.FACTS
    )
    return [{"type": "session.context.appended"} for _ in range(6)] + [
        {"type": "turn.done", "turn": {"role": role, "transcript": transcript}}
    ]


def report(events, audio=None):
    return PROBE.summarize_probe(
        events,
        PROBE.probe_commands("burst"),
        audio if audio is not None else {"bytes": 1024, "energy": 1},
    )


def test_probe_is_opt_in_and_safe_to_import(monkeypatch, capsys):
    monkeypatch.delenv("CCCC_CODEX_VOICE_RESULTS_LIVE", raising=False)
    with patch.object(
        PROBE.subprocess, "Popen", side_effect=AssertionError("must not launch")
    ):
        assert PROBE.main() == 0
    assert "Skipped" in capsys.readouterr().out


def test_invalid_timing_is_rejected_before_any_browser_or_credential_access(
    monkeypatch,
):
    monkeypatch.setenv("CCCC_CODEX_VOICE_RESULTS_LIVE", "1")
    monkeypatch.setenv("PROBE_TIMING", "invalid")
    with patch.object(
        PROBE.subprocess, "Popen", side_effect=AssertionError("must not launch")
    ):
        with pytest.raises(ValueError, match="PROBE_TIMING"):
            PROBE.main()


def test_burst_and_summary_contain_the_same_independent_facts():
    burst = PROBE.probe_commands("burst")
    assert burst == PROBE.probe_commands("mid_turn")
    summary = PROBE.probe_commands("summary")
    assert len(burst) == 6 and len(summary) == 1
    assert (
        " ".join(c["content"][0]["text"] for c in burst)
        == summary[0]["content"][0]["text"]
    )
    after_turn = PROBE.probe_commands("after_turn")
    assert len(after_turn) == 2
    assert (
        " ".join(c["content"][0]["text"] for c in after_turn)
        == summary[0]["content"][0]["text"]
    )


@pytest.mark.parametrize(
    "url", ["https://example.com", "http://example.com", "file:///tmp/test"]
)
def test_browser_scheduler_never_navigates_to_an_external_probe_page(monkeypatch, url):
    monkeypatch.setenv("CCCC_CODEX_VOICE_RESULTS_LIVE", "1")
    monkeypatch.setenv("PROBE_TIMING", "after_turn")
    monkeypatch.setenv("PROBE_SCHEDULER_URL", url)
    with patch.object(
        PROBE.subprocess, "Popen", side_effect=AssertionError("must not launch")
    ):
        with pytest.raises(ValueError, match="loopback"):
            PROBE.main()


def test_interruption_requires_the_actual_browser_scheduler(monkeypatch):
    monkeypatch.setenv("CCCC_CODEX_VOICE_RESULTS_LIVE", "1")
    monkeypatch.setenv("PROBE_TIMING", "after_turn")
    monkeypatch.delenv("PROBE_SCHEDULER_URL", raising=False)
    monkeypatch.setenv("PROBE_INTERRUPT_WAV", "unused-synthetic.wav")
    with patch.object(
        PROBE.subprocess, "Popen", side_effect=AssertionError("must not launch")
    ):
        with pytest.raises(ValueError, match="actual browser scheduler"):
            PROBE.main()


def test_receipts_transcript_audio_and_hearing_are_separate():
    observed = report(complete_events())
    assert observed["observed_delivery"] is True
    assert observed["echoed_client_ids"] == []
    assert observed["per_source_speech_confirmed"] is False
    assert observed["human_heard_confirmed"] is False
    assert observed["user_interruption_tested"] is False
    assert (
        report(complete_events(), {"bytes": 1024, "energy": 0})["observed_delivery"]
        is False
    )
    assert report(complete_events(), {})["observed_delivery"] is False
    assert (
        report(complete_events(), {"bytes": 1024, "energy": 0, "decoded_power": 5})[
            "observed_delivery"
        ]
        is True
    )
    assert report(complete_events()[:-1])["observed_delivery"] is False
    assert report(complete_events("user"))["observed_delivery"] is False


def test_generic_range_or_wrong_fact_value_does_not_pass():
    events = complete_events()
    events[-1]["turn"]["transcript"] = "Checkpoints one through six all passed."
    assert len(report(events)["synthetic_facts_absent_from_completed_transcripts"]) == 6
    events = complete_events()
    events[-1]["turn"]["transcript"] = events[-1]["turn"]["transcript"].replace(
        "amber result is 17", "amber result is 76"
    )
    assert report(events)["synthetic_facts_absent_from_completed_transcripts"] == [
        "amber"
    ]


def test_number_words_are_accepted_but_each_fact_must_survive():
    events = complete_events()
    events[-1]["turn"]["transcript"] = " ".join(
        f"The {name} result is {word.replace(' ', '-')} ."
        for (name, _), word in zip(PROBE.FACTS, PROBE.NUMBER_WORDS)
    )
    assert report(events)["observed_delivery"] is True


def test_provider_error_blocks_pass_and_diagnostics_do_not_expose_content():
    event = {
        "type": "error",
        "event_id": "provider-1",
        "error": {
            "code": "rate_limit",
            "message": "PRIVATE_ERROR_BODY",
            "token": "PRIVATE_TOKEN",
        },
        "content": "PRIVATE_CONTENT",
    }
    observed = report(complete_events() + [event])
    assert observed["observed_delivery"] is False
    encoded = json.dumps(observed)
    assert "rate_limit" in encoded
    assert "PRIVATE" not in encoded
    transcript = PROBE.protocol_observation(complete_events()[-1])
    assert "transcript" in transcript["turn_fields"]
    assert "amber" not in json.dumps(transcript)


def test_receipt_ids_are_reported_only_when_they_match_sent_ids():
    events = complete_events()
    events[0]["event_id"] = "server-generated-id"
    events[1]["client_event_id"] = "probe-2"
    events[2]["event_id"] = {"not": "an identifier"}
    assert report(events)["echoed_client_ids"] == ["probe-2"]
