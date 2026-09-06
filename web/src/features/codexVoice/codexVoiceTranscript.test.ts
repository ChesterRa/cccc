import { describe, expect, it } from "vitest";
import { RealtimeTranscriptAccumulator, realtimeTranscriptUpdate } from "./codexVoiceProtocol";

function receive(transcripts: RealtimeTranscriptAccumulator, event: unknown): void {
  transcripts.observeTurn(event);
  const update = realtimeTranscriptUpdate(event);
  if (update) transcripts.apply(update);
}

describe("Codex Voice transcript identity", () => {
  it.each([
    { role: "user", type: "input_transcript.added", prefix: "帮我", suffix: "查一下天气" },
    {
      role: "assistant",
      type: "output_transcript.added",
      prefix: "Hi!",
      suffix: " How can I help?",
    },
  ])(
    "keeps the $role prefix when turn.created arrives after its first delta",
    ({ role, type, prefix, suffix }) => {
      const transcripts = new RealtimeTranscriptAccumulator();
      receive(transcripts, { type, item: { text: prefix } });
      receive(transcripts, { type: "turn.created", turn: { id: "t1", role, transcript: prefix } });
      receive(transcripts, { type, item: { text: suffix } });
      expect(transcripts.history()).toEqual([
        { id: "t1", role, text: prefix + suffix, final: false },
      ]);
      receive(transcripts, {
        type: "turn.done",
        turn: { id: "t1", role, transcript: prefix + suffix },
      });
      expect(transcripts.history()).toEqual([
        { id: "t1", role, text: prefix + suffix, final: true },
      ]);
    },
  );

  it("binds a final-only turn and preserves a genuinely repeated sentence", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    for (const id of ["u1", "u2"]) {
      receive(transcripts, { type: "input_transcript.added", item: { text: "帮我" } });
      receive(transcripts, {
        type: "turn.done",
        turn: { id, role: "user", transcript: "帮我查一下天气" },
      });
    }
    expect(transcripts.history()).toEqual([
      { id: "u1", role: "user", text: "帮我查一下天气", final: true },
      { id: "u2", role: "user", text: "帮我查一下天气", final: true },
    ]);
  });

  it("keeps first-arrival order when both speakers receive their identities late", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    receive(transcripts, { type: "input_transcript.added", item: { text: "Please" } });
    receive(transcripts, { type: "output_transcript.added", item: { text: "Okay" } });
    receive(transcripts, { type: "turn.created", turn: { id: "a1", role: "assistant" } });
    receive(transcripts, { type: "turn.created", turn: { id: "u1", role: "user" } });
    receive(transcripts, { type: "output_transcript.added", item: { text: "." } });
    receive(transcripts, { type: "input_transcript.added", item: { text: " wait." } });
    expect(transcripts.history()).toEqual([
      { id: "u1", role: "user", text: "Please wait.", final: false },
      { id: "a1", role: "assistant", text: "Okay.", final: false },
    ]);
  });

  it("does not bind a new draft to an old turn's repeated final or late created event", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    receive(transcripts, {
      type: "turn.done",
      turn: { id: "u1", role: "user", transcript: "First." },
    });
    receive(transcripts, { type: "input_transcript.added", item: { text: "Next" } });
    receive(transcripts, {
      type: "turn.done",
      turn: { id: "u1", role: "user", transcript: "First." },
    });
    receive(transcripts, { type: "turn.created", turn: { id: "u1", role: "user" } });
    receive(transcripts, { type: "input_transcript.added", item: { text: " question." } });
    receive(transcripts, { type: "turn.created", turn: { id: "u2", role: "user" } });
    expect(transcripts.history()).toEqual([
      { id: "u1", role: "user", text: "First.", final: true },
      { id: "u2", role: "user", text: "Next question.", final: false },
    ]);
  });

  it("clears an anonymous draft when its authoritative final transcript is empty", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    receive(transcripts, { type: "input_transcript.added", item: { text: "嗯" } });
    receive(transcripts, { type: "turn.done", turn: { id: "u1", role: "user", transcript: "" } });
    expect(transcripts.history()).toEqual([]);
    receive(transcripts, { type: "input_transcript.added", item: { text: "新问题" } });
    receive(transcripts, { type: "turn.created", turn: { id: "u2", role: "user" } });
    expect(transcripts.history()).toEqual([
      { id: "u2", role: "user", text: "新问题", final: false },
    ]);
  });
});
