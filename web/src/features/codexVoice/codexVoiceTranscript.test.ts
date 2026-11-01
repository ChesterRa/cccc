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

describe("Chet history record of a call", () => {
  it("offers each finished turn once, whole, in the order the call first saw it", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    transcripts.bindCall("call-1", true);
    receive(transcripts, { type: "input_transcript.added", item: { text: "帮我" } });
    receive(transcripts, { type: "output_transcript.added", item: { text: "好" } });
    const long = `帮我${"查".repeat(5000)}`;
    receive(transcripts, { type: "turn.done", turn: { id: "u1", role: "user", transcript: long } });
    expect(transcripts.takeFinished()).toEqual([
      {
        type: "transcript",
        id: expect.stringMatching(/^[0-9a-f-]{36}$/),
        turn_id: "u1",
        seq: 1,
        role: "user",
        text: long,
        final_update: true,
        complete: true,
      },
    ]);
    // What is shown stays clipped; what is kept is not.
    const [shown] = transcripts.history();
    expect(shown).toMatchObject({ id: "u1", callId: "call-1", save: "pending" });
    expect(shown.text.length).toBeLessThanOrEqual(4000);
    receive(transcripts, {
      type: "turn.done",
      turn: { id: "a1", role: "assistant", transcript: "好的" },
    });
    expect(transcripts.takeFinished()).toMatchObject([{ turn_id: "a1", seq: 2, text: "好的" }]);
    expect(transcripts.takeFinished()).toEqual([]);
  });

  it("records a Grok call's turns from its own event names", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    transcripts.bindCall("call-g", true);
    const grok = (event: unknown) => {
      const update = realtimeTranscriptUpdate(event, "grok");
      if (update) transcripts.apply(update);
    };
    grok({
      type: "conversation.item.input_audio_transcription.updated",
      item_id: "i1",
      transcript: "明天",
    });
    grok({ type: "response.output_audio_transcript.delta", response_id: "r1", delta: "十点" });
    expect(transcripts.takeFinished()).toEqual([]);
    grok({
      type: "conversation.item.input_audio_transcription.completed",
      item_id: "i1",
      transcript: "明天几点开会？",
    });
    grok({
      type: "response.output_audio_transcript.done",
      response_id: "r1",
      transcript: "十点。",
    });
    expect(transcripts.takeFinished()).toMatchObject([
      { turn_id: "i1", seq: 1, role: "user", text: "明天几点开会？", final_update: true },
      { turn_id: "r1", seq: 2, role: "assistant", text: "十点。", final_update: true },
    ]);
  });

  it("keeps the streamed words of a turn that finished without a final transcript", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    transcripts.bindCall("call-1", true);
    receive(transcripts, { type: "output_transcript.added", item: { text: "Hi" } });
    receive(transcripts, { type: "turn.created", turn: { id: "a1", role: "assistant" } });
    receive(transcripts, { type: "output_transcript.added", item: { text: " there" } });
    transcripts.completeTurn("a1", "assistant");
    expect(transcripts.takeFinished()).toMatchObject([
      { turn_id: "a1", text: "Hi there", final_update: true, complete: true },
    ]);
  });

  it("clips a turn beyond the history limit without splitting a character, and says so", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    transcripts.bindCall("call-1", true);
    receive(transcripts, {
      type: "turn.done",
      turn: { id: "u1", role: "user", transcript: "🙂".repeat(9000) },
    });
    const [frame] = transcripts.takeFinished();
    expect(frame.text).toBe("🙂".repeat(8192));
    expect(frame).toMatchObject({ final_update: true, complete: false });
  });

  it("offers what an orderly end cut off as unfinished, and keeps nothing from a persona call", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    transcripts.bindCall("call-1", true);
    receive(transcripts, { type: "output_transcript.added", item: { text: "我正在" } });
    expect(transcripts.takeFinished()).toEqual([]);
    expect(transcripts.takeUnfinished()).toMatchObject([
      { role: "assistant", text: "我正在", seq: 1, final_update: false, complete: false },
    ]);
    expect(transcripts.takeUnfinished()).toEqual([]);

    const persona = new RealtimeTranscriptAccumulator();
    persona.bindCall("call-2", false);
    receive(persona, { type: "turn.done", turn: { id: "u1", role: "user", transcript: "你好" } });
    receive(persona, { type: "output_transcript.added", item: { text: "嗨" } });
    expect(persona.takeFinished()).toEqual([]);
    expect(persona.takeUnfinished()).toEqual([]);
    expect(persona.history().map((turn) => turn.save)).toEqual([undefined, undefined]);
  });

  it("shows history's answer for each turn, and an unanswered one as unconfirmed", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    transcripts.bindCall("call-1", true);
    for (const id of ["u1", "u2", "u3"]) {
      receive(transcripts, { type: "turn.done", turn: { id, role: "user", transcript: id } });
    }
    const [saved, unknown] = transcripts.takeFinished();
    expect(transcripts.receipt(saved.id, "saved")).toBe(true);
    expect(transcripts.receipt(saved.id, "not_saved")).toBe(false);
    expect(transcripts.receipt(unknown.id, "some future answer")).toBe(true);
    // The socket closed before the third was answered.
    expect(transcripts.abandonOffered()).toBe(true);
    expect(transcripts.history().map((turn) => turn.save)).toEqual([
      "saved",
      "unconfirmed",
      "unconfirmed",
    ]);
  });
});
