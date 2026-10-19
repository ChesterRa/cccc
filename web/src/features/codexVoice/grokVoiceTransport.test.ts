import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { GrokVoiceTransport } from "./grokVoiceTransport";
import {
  RealtimeTranscriptAccumulator,
  realtimeTranscriptUpdate,
  shouldForwardProviderEvent,
} from "./codexVoiceProtocol";
import type { GrokVoiceConnection } from "../../services/api";

const state = vi.hoisted(() => ({
  playing: new Set<string>(),
  chunks: [] as string[],
  capture: null as ((buffer: ArrayBuffer) => void) | null,
  drained: null as ((id: string) => void) | null,
  closed: false,
}));
vi.mock("./grokVoiceAudio", () => ({
  GrokVoiceAudio: class {
    constructor(
      _element: unknown,
      private speaking: (value: boolean) => void,
      drained: (id: string) => void,
    ) {
      state.drained = drained;
    }
    async startCapture(_stream: unknown, capture: (buffer: ArrayBuffer) => void) {
      state.capture = capture;
    }
    append(_bytes: string, id: string) {
      state.chunks.push(id);
      state.playing.add(id);
      this.speaking(true);
    }
    hasResponse(id: string) {
      return state.playing.has(id);
    }
    isPlaying() {
      return state.playing.size > 0;
    }
    interrupt() {
      state.playing.clear();
      this.speaking(false);
    }
    async resume() {
      return true;
    }
    close() {
      state.closed = true;
      state.playing.clear();
    }
  },
}));

class FakeSocket {
  static OPEN = 1;
  static latest: FakeSocket;
  readyState = 1;
  bufferedAmount = 0;
  frames: Record<string, unknown>[] = [];
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(
    readonly url: string,
    readonly protocols: string[],
  ) {
    FakeSocket.latest = this;
  }
  send(data: string) {
    this.frames.push(JSON.parse(data));
  }
  close() {
    this.readyState = 3;
  }
  emit(event: unknown) {
    this.onmessage?.({ data: JSON.stringify(event) });
  }
}
const connection: GrokVoiceConnection = {
  transport: "websocket_pcm",
  url: "wss://fixture.invalid/voice",
  client_secret: "fixture-ephemeral",
  expires_at: 9_999_999_999,
  session: { voice: "eve" },
};
const live: GrokVoiceTransport[] = [];

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", FakeSocket);
  state.playing.clear();
  state.chunks.length = 0;
  state.capture = null;
  state.closed = false;
});

it("retains buffered answers but ignores audio from a superseded response", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  socket.emit({ type: "response.created", response: { id: "first" } });
  socket.emit({ type: "response.output_audio.delta", response_id: "first", delta: "AAAA" });
  socket.emit({ type: "response.done", response: { id: "first", status: "completed" } });
  socket.emit({ type: "response.created", response: { id: "second" } });
  socket.emit({ type: "response.output_audio.delta", response_id: "second", delta: "AAAA" });
  socket.emit({ type: "response.output_audio.delta", response_id: "first", delta: "AAAA" });
  expect(state.chunks).toEqual(["first", "second"]);
  // A new response must not discard audio that was already accepted for playback.
  expect([...state.playing]).toEqual(["first", "second"]);
});
afterEach(() => {
  for (const transport of live.splice(0)) transport.close();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function fixture(prepare?: (id: string, signal: AbortSignal) => Promise<unknown | null>) {
  const failed = vi.fn();
  const submitted = vi.fn();
  const unconfirmed = vi.fn();
  const transport = new GrokVoiceTransport(
    [
      (data) => transport.observe(JSON.parse(String(data))),
      failed,
      () => false,
      unconfirmed,
      submitted,
      prepare,
    ],
    vi.fn(),
    vi.fn(),
  );
  live.push(transport);
  transport.prepareAudio({} as HTMLAudioElement);
  return { transport, failed, submitted, unconfirmed };
}
async function connect(transport: GrokVoiceTransport) {
  const connected = transport.connect(connection, {} as MediaStream);
  const socket = FakeSocket.latest;
  socket.emit({ type: "session.created" });
  expect(state.capture).toBeNull();
  expect(socket.frames).toEqual([{ type: "session.update", session: { voice: "eve" } }]);
  socket.emit({ type: "session.updated" });
  await connected;
  return socket;
}
function context(text = "Investigation result") {
  return {
    type: "session.context.append",
    channel: "speakable",
    content: [{ type: "input_text", text }],
  };
}
function report(text: string, id: string) {
  return { ...context(text), _cccc_report_id: id };
}
function items(socket: FakeSocket) {
  return socket.frames
    .filter((frame) => frame.type === "conversation.item.create")
    .map((frame) => frame.item as Record<string, unknown>);
}
function functionDone(id = "tool-1") {
  return {
    type: "response.function_call_arguments.done",
    name: "cccc_investigate",
    call_id: id,
    arguments: '{"request":"Inspect this project"}',
  };
}

it("configures before capturing and uses real item receipts before requesting speech", async () => {
  const { transport, submitted } = fixture();
  transport.send(context());
  const socket = await connect(transport);
  expect(socket.protocols).toEqual(["xai-client-secret.fixture-ephemeral"]);
  state.capture!(new Int16Array([100, -100]).buffer);
  expect(socket.frames.at(-1)?.type).toBe("input_audio_buffer.append");
  await vi.advanceTimersByTimeAsync(150);
  const item = items(socket)[0];
  expect(item.role).toBe("user");
  expect(transport.receipt()).toMatchObject({
    sent: 1,
    acknowledged: 0,
    pending: 1,
    speech_turns_completed: 0,
  });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
  socket.emit({ type: "session.context.appended" });
  expect(transport.receipt().pending).toBe(1);
  socket.emit({ type: "conversation.item.added", item: { id: item.id } });
  socket.emit({ type: "conversation.item.created", item: { id: item.id } });
  await vi.advanceTimersByTimeAsync(150);
  expect(transport.receipt()).toMatchObject({ acknowledged: 1, pending: 0 });
  expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(1);
  expect(submitted).not.toHaveBeenCalled();
});

it("returns function acceptance only after host admission and does not re-open duplicate calls", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  socket.emit(functionDone());
  expect(items(socket)).toHaveLength(0);
  transport.send({ type: "cccc.delegation.receipt", call_id: "unobserved", accepted: true });
  expect(items(socket)).toHaveLength(0);
  transport.send({ type: "cccc.delegation.receipt", call_id: "tool-1", accepted: true });
  expect(items(socket)[0]).toMatchObject({
    type: "function_call_output",
    call_id: "tool-1",
    output: '{"status":"received"}',
  });
  socket.emit(functionDone());
  transport.send({ type: "cccc.delegation.receipt", call_id: "tool-1", accepted: true });
  expect(items(socket)).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(1);
});

it.each([true, false])(
  "speaks a tool receipt independently of an older missing context ACK (accepted=%s)",
  async (accepted) => {
    const { transport, failed } = fixture();
    const socket = await connect(transport);
    transport.send(context("Older unconfirmed update"));
    await vi.advanceTimersByTimeAsync(150);
    socket.emit({ type: "response.created", response: { id: "tool-response" } });
    socket.emit(functionDone());
    socket.emit({ type: "response.done", response: { id: "tool-response", status: "completed" } });
    transport.send({ type: "cccc.delegation.receipt", call_id: "tool-1", accepted });
    await vi.advanceTimersByTimeAsync(450);
    expect(items(socket).at(-1)).toMatchObject({
      type: "function_call_output",
      output: JSON.stringify({ status: accepted ? "received" : "rejected" }),
    });
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(1);
    expect(transport.receipt()).toMatchObject({ acknowledged: 0, pending: 1 });
    expect(failed).not.toHaveBeenCalled();
  },
);

it("reports a hundred rapidly queued results separately, in order, after prior playback drains", async () => {
  const { transport, failed } = fixture();
  const configured = {
    ...connection,
    session: { voice: "eve", instructions: "Keep the original authorization and language rules." },
  };
  const connected = transport.connect(configured, {} as MediaStream);
  const socket = FakeSocket.latest;
  socket.emit({ type: "session.created" });
  socket.emit({ type: "session.updated" });
  await connected;
  socket.emit({ type: "response.created", response: { id: "previous-answer" } });
  socket.emit({
    type: "response.output_audio.delta",
    response_id: "previous-answer",
    delta: "AAAA",
  });
  for (let index = 0; index < 100; index += 1)
    transport.send(report(`Independent completed result #${index}#`, `report-${index}`));
  await vi.advanceTimersByTimeAsync(20_000);
  expect(items(socket)).toHaveLength(0);
  socket.emit({ type: "response.done", response: { id: "previous-answer", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
  state.playing.delete("previous-answer");
  state.drained!("previous-answer");
  for (let index = 0; index < 100; index += 1) {
    await vi.advanceTimersByTimeAsync(150);
    expect(items(socket)).toHaveLength(index + 1);
    const item = items(socket).at(-1)!;
    expect(JSON.stringify(item)).toContain(`Independent completed result #${index}#`);
    socket.emit({ type: "conversation.item.added", item: { id: item.id } });
    await vi.advanceTimersByTimeAsync(150);
    const requests = socket.frames.filter((frame) => frame.type === "response.create");
    expect(requests).toHaveLength(index + 1);
    const instructions = (requests.at(-1)!.response as { instructions: string }).instructions;
    expect(instructions).toContain(configured.session.instructions);
    expect(instructions).toContain(`Independent completed result #${index}#`);
    if (index < 99)
      expect(instructions).not.toContain(`Independent completed result #${index + 1}#`);
    const id = `report-answer-${index}`;
    socket.emit({ type: "response.created", response: { id } });
    socket.emit({ type: "response.output_audio.delta", response_id: id, delta: "AAAA" });
    socket.emit({ type: "response.done", response: { id, status: "completed" } });
    await vi.advanceTimersByTimeAsync(150);
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(
      index + 1,
    );
    state.playing.delete(id);
    state.drained!(id);
  }
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(100);
  expect(transport.receipt()).toMatchObject({
    sent: 100,
    acknowledged: 100,
    pending: 0,
    speech_turns_completed: 101,
  });
  expect(failed).not.toHaveBeenCalled();
});

it("does not let an automatic conversational answer consume a queued report", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  transport.send(report("The completed result is TOPAZ.", "report-a"));
  await vi.advanceTimersByTimeAsync(150);
  socket.emit({ type: "conversation.item.added", item: { id: items(socket)[0].id } });
  socket.emit({ type: "response.created", response: { id: "automatic" } });
  socket.emit({ type: "response.done", response: { id: "automatic", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  const requests = socket.frames.filter((frame) => frame.type === "response.create");
  expect(requests).toHaveLength(1);
  expect(JSON.stringify(requests[0])).toContain("The completed result is TOPAZ.");
});

it("continues a burst of reports after one report ACK is lost without replay on its late ACK", async () => {
  const { transport, failed } = fixture();
  const socket = await connect(transport);
  transport.send(report("Unconfirmed older result", "missing"));
  await vi.advanceTimersByTimeAsync(150);
  const missing = items(socket)[0];
  for (let index = 0; index < 100; index += 1)
    transport.send(report(`Later completed result ${index}`, `later-${index}`));
  for (let index = 0; index < 100; index += 1) {
    await vi.advanceTimersByTimeAsync(150);
    socket.emit({ type: "conversation.item.added", item: { id: items(socket).at(-1)!.id } });
    await vi.advanceTimersByTimeAsync(150);
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(
      index + 1,
    );
    const id = `later-answer-${index}`;
    socket.emit({ type: "response.created", response: { id } });
    socket.emit({ type: "response.output_audio.delta", response_id: id, delta: "AAAA" });
    socket.emit({ type: "response.done", response: { id, status: "completed" } });
    state.playing.delete(id);
    state.drained!(id);
  }
  expect(transport.receipt()).toMatchObject({
    sent: 101,
    acknowledged: 100,
    pending: 1,
    speech_turns_completed: 100,
  });
  socket.emit({ type: "conversation.item.added", item: { id: missing.id } });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(100);
  expect(failed).not.toHaveBeenCalled();
});

it("continues the next queued report after a user interruption without replaying the interrupted one", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  transport.send(report("First result", "first"));
  await vi.advanceTimersByTimeAsync(150);
  socket.emit({ type: "conversation.item.added", item: { id: items(socket)[0].id } });
  await vi.advanceTimersByTimeAsync(150);
  socket.emit({ type: "response.created", response: { id: "first-answer" } });
  socket.emit({ type: "response.output_audio.delta", response_id: "first-answer", delta: "AAAA" });
  transport.send(report("Second result", "second"));
  socket.emit({ type: "input_audio_buffer.speech_started" });
  socket.emit({ type: "response.done", response: { id: "first-answer", status: "cancelled" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(items(socket)).toHaveLength(1);
  socket.emit({ type: "input_audio_buffer.speech_stopped" });
  socket.emit({ type: "response.created", response: { id: "user-reply" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(items(socket)).toHaveLength(1);
  socket.emit({ type: "response.done", response: { id: "user-reply", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(items(socket)).toHaveLength(2);
  socket.emit({ type: "conversation.item.added", item: { id: items(socket)[1].id } });
  await vi.advanceTimersByTimeAsync(150);
  const requests = socket.frames.filter((frame) => frame.type === "response.create");
  expect(requests).toHaveLength(2);
  expect(JSON.stringify(requests[1])).toContain("Second result");
  expect(JSON.stringify(requests[1])).not.toContain("First result");
  expect(transport.receipt().speech_turns_completed).toBe(0);
});

it.each([false, true])(
  "waits for a delayed VAD response before announcing a report (already delivered=%s)",
  async (delivered) => {
    const { transport, failed } = fixture();
    const socket = await connect(transport);
    transport.send(report("The manual investigation is complete", "manual-input:delayed"));
    if (delivered) {
      await vi.advanceTimersByTimeAsync(150);
      socket.emit({ type: "conversation.item.added", item: { id: items(socket)[0].id } });
    }
    socket.emit({ type: "input_audio_buffer.speech_started" });
    socket.emit({ type: "input_audio_buffer.speech_stopped" });
    await vi.advanceTimersByTimeAsync(340);
    expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
    expect(items(socket)).toHaveLength(delivered ? 1 : 0);
    socket.emit({ type: "response.created", response: { id: "vad-answer" } });
    socket.emit({ type: "response.output_audio.delta", response_id: "vad-answer", delta: "AAAA" });
    socket.emit({ type: "response.done", response: { id: "vad-answer", status: "completed" } });
    await vi.advanceTimersByTimeAsync(150);
    expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
    state.playing.delete("vad-answer");
    state.drained!("vad-answer");
    await vi.advanceTimersByTimeAsync(150);
    if (!delivered) {
      socket.emit({ type: "conversation.item.added", item: { id: items(socket)[0].id } });
      await vi.advanceTimersByTimeAsync(150);
    }
    const requests = socket.frames.filter((frame) => frame.type === "response.create");
    expect(requests).toHaveLength(1);
    expect(JSON.stringify(requests[0])).toContain("The manual investigation is complete");
    expect(failed).not.toHaveBeenCalled();
  },
);

it("rechecks a report preflight when VAD starts while policy is being read", async () => {
  let release: (command: unknown) => void = () => {};
  const prepare = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    )
    .mockResolvedValue(context("Fresh authorized manual result"));
  const { transport, submitted, failed } = fixture(prepare);
  const socket = await connect(transport);
  transport.send(context("Queued manual result"), "manual-result");
  await vi.advanceTimersByTimeAsync(150);
  expect(prepare).toHaveBeenCalledTimes(1);
  socket.emit({ type: "input_audio_buffer.speech_started" });
  socket.emit({ type: "input_audio_buffer.speech_stopped" });
  release(context("First policy result"));
  await vi.advanceTimersByTimeAsync(340);
  expect(items(socket)).toHaveLength(0);
  expect(submitted).not.toHaveBeenCalled();
  expect(transport.unsentResultIds()).toEqual(["manual-result"]);
  socket.emit({ type: "response.created", response: { id: "vad-answer" } });
  socket.emit({ type: "response.done", response: { id: "vad-answer", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(prepare).toHaveBeenCalledTimes(2);
  expect(JSON.stringify(items(socket)[0])).toContain("Fresh authorized manual result");
  expect(submitted).toHaveBeenCalledExactlyOnceWith("manual-result");
  expect(failed).not.toHaveBeenCalled();
});

it("waits for VAD to start before speaking a tool receipt", async () => {
  const { transport, failed } = fixture();
  const socket = await connect(transport);
  socket.emit(functionDone());
  socket.emit({ type: "input_audio_buffer.speech_started" });
  socket.emit({ type: "input_audio_buffer.speech_stopped" });
  transport.send({ type: "cccc.delegation.receipt", call_id: "tool-1", accepted: true });
  await vi.advanceTimersByTimeAsync(340);
  expect(items(socket)[0]).toMatchObject({ type: "function_call_output", call_id: "tool-1" });
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
  socket.emit({ type: "response.created", response: { id: "vad-answer" } });
  socket.emit({ type: "response.done", response: { id: "vad-answer", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
  expect(failed).not.toHaveBeenCalled();
});

it("does not release the VAD wait when the interrupted response finishes late", async () => {
  const { transport, failed } = fixture();
  const socket = await connect(transport);
  socket.emit({ type: "response.created", response: { id: "old-answer" } });
  socket.emit({ type: "input_audio_buffer.speech_started" });
  transport.send(report("Next result", "next"));
  socket.emit({ type: "input_audio_buffer.speech_stopped" });
  socket.emit({ type: "response.done", response: { id: "old-answer", status: "cancelled" } });
  await vi.advanceTimersByTimeAsync(340);
  expect(items(socket)).toHaveLength(0);
  socket.emit({ type: "response.created", response: { id: "vad-answer" } });
  socket.emit({ type: "response.done", response: { id: "vad-answer", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(items(socket)).toHaveLength(1);
  expect(failed).not.toHaveBeenCalled();
});

it("fails explicitly if the VAD response never starts, preserving unsent work", async () => {
  const { transport, failed } = fixture();
  const socket = await connect(transport);
  socket.emit({ type: "input_audio_buffer.speech_started" });
  socket.emit({ type: "input_audio_buffer.speech_stopped" });
  transport.send(context("Still unsent"), "unsent-result");
  await vi.advanceTimersByTimeAsync(15_001);
  expect(failed).toHaveBeenCalledExactlyOnceWith("provider_output_stalled");
  expect(transport.unsentResultIds()).toEqual(["unsent-result"]);
  expect(items(socket)).toHaveLength(0);
});

it("clears the VAD start deadline on response creation and on close", async () => {
  const { transport, failed } = fixture();
  const socket = await connect(transport);
  socket.emit({ type: "input_audio_buffer.speech_started" });
  socket.emit({ type: "input_audio_buffer.speech_stopped" });
  await vi.advanceTimersByTimeAsync(340);
  socket.emit({ type: "response.created", response: { id: "long-answer" } });
  await vi.advanceTimersByTimeAsync(20_000);
  expect(failed).not.toHaveBeenCalled();
  socket.emit({ type: "input_audio_buffer.speech_started" });
  socket.emit({ type: "input_audio_buffer.speech_stopped" });
  transport.close();
  await vi.advanceTimersByTimeAsync(20_000);
  expect(failed).not.toHaveBeenCalled();
});

it("preflights a queued report when the current response finishes, just before delivery", async () => {
  const prepare = vi.fn(async () => context("Fresh permitted result"));
  const { transport, submitted } = fixture(prepare);
  const socket = await connect(transport);
  socket.emit({ type: "response.created", response: { id: "old-response" } });
  transport.send(context("stale text"), "result-1");
  await vi.advanceTimersByTimeAsync(150);
  expect(prepare).not.toHaveBeenCalled();
  expect(items(socket)).toHaveLength(0);
  socket.emit({ type: "response.done", response: { id: "old-response", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(prepare).toHaveBeenCalledWith("result-1", expect.any(AbortSignal));
  expect(JSON.stringify(items(socket))).toContain("Fresh permitted result");
  expect(JSON.stringify(items(socket))).not.toContain("stale text");
  expect(submitted).toHaveBeenCalledExactlyOnceWith("result-1");
  expect(transport.unsentResultIds()).toEqual([]);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
});

it.each(["starting", "active"])(
  "speaks a newer investigation result after the %s response drains",
  async (phase) => {
    const { transport } = fixture();
    const socket = await connect(transport);
    transport.send(context("Investigation is in progress"));
    await vi.advanceTimersByTimeAsync(150);
    socket.emit({ type: "conversation.item.added", item: { id: items(socket)[0].id } });
    await vi.advanceTimersByTimeAsync(150);
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(1);
    if (phase === "active") socket.emit({ type: "response.created", response: { id: "progress" } });

    transport.send(context("The authoritative final result is ready"));
    await vi.advanceTimersByTimeAsync(150);
    socket.emit({ type: "conversation.item.added", item: { id: items(socket)[1].id } });
    if (phase === "starting")
      socket.emit({ type: "response.created", response: { id: "progress" } });
    socket.emit({ type: "response.output_audio.delta", response_id: "progress", delta: "AAAA" });
    socket.emit({ type: "response.done", response: { id: "progress", status: "completed" } });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(transport.receipt()).toMatchObject({ sent: 2, acknowledged: 2, pending: 0 });
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(1);

    state.playing.delete("progress");
    state.drained!("progress");
    await vi.advanceTimersByTimeAsync(150);
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(2);
    socket.emit({ type: "response.created", response: { id: "final" } });
    socket.emit({ type: "response.output_audio.delta", response_id: "final", delta: "AAAA" });
    socket.emit({ type: "response.done", response: { id: "final", status: "completed" } });
    state.playing.delete("final");
    state.drained!("final");
    await vi.advanceTimersByTimeAsync(1_000);
    expect(transport.receipt().speech_turns_completed).toBe(2);
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(2);
    expect(items(socket)).toHaveLength(2);
  },
);

it("lets an automatic provider response consume context that was already delivered", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  transport.send(context());
  await vi.advanceTimersByTimeAsync(150);
  socket.emit({ type: "conversation.item.added", item: { id: items(socket)[0].id } });
  // Server VAD starts a response before the scheduled local response request.
  socket.emit({ type: "response.created", response: { id: "automatic" } });
  socket.emit({ type: "response.done", response: { id: "automatic", status: "completed" } });
  await vi.advanceTimersByTimeAsync(1_000);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
  expect(items(socket)).toHaveLength(1);
});

it("keeps a result pending when its acknowledgement follows an automatic response start", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  transport.send(context("A newer authoritative result"));
  await vi.advanceTimersByTimeAsync(150);
  const item = items(socket)[0];
  // The server's automatic response started before this context was accepted.
  socket.emit({ type: "response.created", response: { id: "automatic" } });
  socket.emit({ type: "conversation.item.added", item: { id: item.id } });
  socket.emit({ type: "response.done", response: { id: "automatic", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(transport.receipt()).toMatchObject({ sent: 1, acknowledged: 1, pending: 0 });
  expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(1);
  expect(items(socket)).toHaveLength(1);
});

it("counts playback after queue drain and discards buffered and late audio on VAD interruption", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  socket.emit({ type: "response.created", response: { id: "a1" } });
  socket.emit({ type: "response.output_audio.delta", response_id: "a1", delta: "AAAA" });
  socket.emit({ type: "response.done", response: { id: "a1", status: "completed" } });
  expect(transport.receipt().speech_turns_completed).toBe(0);
  state.playing.delete("a1");
  state.drained!("a1");
  expect(transport.receipt().speech_turns_completed).toBe(1);
  socket.emit({ type: "response.created", response: { id: "a2" } });
  socket.emit({ type: "response.output_audio.delta", response_id: "a2", delta: "AAAA" });
  socket.emit({ type: "input_audio_buffer.speech_started" });
  socket.emit({ type: "response.output_audio.delta", response_id: "a2", delta: "AAAA" });
  socket.emit({ type: "response.done", response: { id: "a2", status: "cancelled" } });
  expect(state.playing.size).toBe(0);
  expect(transport.receipt().speech_turns_completed).toBe(1);
  expect(socket.frames.some((frame) => frame.type === "response.cancel")).toBe(false);
});

it("keeps positively unsent IDs available before a failed wire starts teardown", async () => {
  const { transport, failed } = fixture(async () => context());
  const socket = await connect(transport);
  let unsent: string[] = [];
  failed.mockImplementation(() => {
    unsent = transport.unsentResultIds();
    transport.close();
  });
  transport.send(context(), "result-2");
  socket.bufferedAmount = 300_000;
  await vi.advanceTimersByTimeAsync(150);
  expect(failed).toHaveBeenCalledWith("provider_output_stalled");
  expect(unsent).toEqual(["result-2"]);
});

it("warns without replay when context confirmation is missing and cleans up pending connection", async () => {
  const { transport, unconfirmed } = fixture();
  const socket = await connect(transport);
  transport.send(context());
  await vi.advanceTimersByTimeAsync(31_000);
  expect(unconfirmed).toHaveBeenCalledTimes(1);
  expect(items(socket)).toHaveLength(1);
  const next = fixture().transport;
  const promise = next.connect(connection, {} as MediaStream);
  next.close();
  await expect(promise).rejects.toMatchObject({ code: "provider_event_channel_closed" });
  expect(state.closed).toBe(true);
});

it("keeps a missing context receipt without blocking a hundred later spoken results", async () => {
  const { transport, unconfirmed, failed } = fixture();
  const socket = await connect(transport);
  transport.send(context("An older update with no receipt"));
  await vi.advanceTimersByTimeAsync(150);
  const missing = items(socket)[0];
  for (let index = 0; index < 100; index += 1) {
    transport.send(context(`Confirmed result ${index}`));
    await vi.advanceTimersByTimeAsync(150);
    socket.emit({ type: "conversation.item.added", item: { id: items(socket).at(-1)!.id } });
    await vi.advanceTimersByTimeAsync(150);
    expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(
      index + 1,
    );
    const id = `answer-${index}`;
    socket.emit({ type: "response.created", response: { id } });
    socket.emit({ type: "response.output_audio.delta", response_id: id, delta: "AAAA" });
    socket.emit({ type: "response.done", response: { id, status: "completed" } });
    state.playing.delete(id);
    state.drained!(id);
    await vi.advanceTimersByTimeAsync(150);
  }
  expect(transport.receipt()).toMatchObject({
    sent: 101,
    acknowledged: 100,
    pending: 1,
    speech_turns_completed: 100,
  });
  expect(items(socket)).toHaveLength(101);
  expect(unconfirmed).toHaveBeenCalledTimes(1);
  expect(failed).not.toHaveBeenCalled();
  socket.emit({ type: "conversation.item.added", item: { id: missing.id } });
  await vi.advanceTimersByTimeAsync(1_000);
  expect(transport.receipt()).toMatchObject({ acknowledged: 101, pending: 0 });
  expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(100);
});

it("does not let an old receipt release a newer unconfirmed context", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  transport.send(context("Older update"));
  await vi.advanceTimersByTimeAsync(150);
  const older = items(socket)[0];
  transport.send(context("Newer update"));
  await vi.advanceTimersByTimeAsync(150);
  const newer = items(socket)[1];
  socket.emit({ type: "conversation.item.added", item: { id: older.id } });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
  socket.emit({ type: "conversation.item.added", item: { id: newer.id } });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.filter((frame) => frame.type === "response.create")).toHaveLength(1);
});

it("does not repeat an automatic answer to new context because an older receipt is missing", async () => {
  const { transport } = fixture();
  const socket = await connect(transport);
  transport.send(context("Older update without a receipt"));
  await vi.advanceTimersByTimeAsync(150);
  transport.send(context("Newer update with a receipt"));
  await vi.advanceTimersByTimeAsync(150);
  socket.emit({ type: "conversation.item.added", item: { id: items(socket)[1].id } });
  socket.emit({ type: "response.created", response: { id: "automatic" } });
  socket.emit({ type: "response.done", response: { id: "automatic", status: "completed" } });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
  expect(transport.receipt()).toMatchObject({ acknowledged: 1, pending: 1 });
  socket.emit({ type: "conversation.item.added", item: { id: items(socket)[0].id } });
  await vi.advanceTimersByTimeAsync(150);
  expect(socket.frames.some((frame) => frame.type === "response.create")).toBe(false);
});

it("reconciles correctable cumulative ASR and finalizes an interrupted partial response", () => {
  const accumulator = new RealtimeTranscriptAccumulator();
  for (const transcript of ["I said red", "I said read"]) {
    const update = realtimeTranscriptUpdate(
      { type: "conversation.item.input_audio_transcription.updated", item_id: "u1", transcript },
      "grok",
    )!;
    accumulator.apply(update);
  }
  accumulator.apply(
    realtimeTranscriptUpdate(
      {
        type: "conversation.item.input_audio_transcription.completed",
        item_id: "u1",
        transcript: "I said read.",
      },
      "grok",
    )!,
  );
  accumulator.apply(
    realtimeTranscriptUpdate(
      { type: "response.output_audio_transcript.delta", response_id: "r1", delta: "Understood" },
      "grok",
    )!,
  );
  accumulator.completeTurn("r1");
  expect(accumulator.history()).toEqual([
    { id: "u1", role: "user", text: "I said read.", final: true },
    { id: "r1", role: "assistant", text: "Understood", final: true },
  ]);
  expect(shouldForwardProviderEvent(functionDone(), "grok")).toBe(true);
  expect(shouldForwardProviderEvent(functionDone(), "codex")).toBe(false);
});
