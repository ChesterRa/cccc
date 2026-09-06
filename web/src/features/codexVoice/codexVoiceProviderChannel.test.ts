import { afterEach, describe, expect, it, vi } from "vitest";
import { CodexVoiceProviderChannel } from "./codexVoiceProviderChannel";
import { failure } from "./codexVoiceProtocol";

afterEach(() => vi.useRealTimers());

function fixture(readyState: RTCDataChannelState = "open") {
  vi.useFakeTimers();
  const failed = vi.fn();
  const unconfirmed = vi.fn();
  const wire = { readyState, send: vi.fn(), close: vi.fn(), onopen: null as null | (() => void) };
  const channel = new CodexVoiceProviderChannel(vi.fn(), failed, () => false, unconfirmed);
  channel.bind(wire as unknown as RTCDataChannel);
  return { wire, channel, failed, unconfirmed };
}

describe("Realtime context delivery receipts", () => {
  it.each(["queued", "unconfirmed"])(
    "retains the rejected result on %s overflow before teardown",
    (stage) => {
      vi.useFakeTimers();
      let unsent: string[] = [];
      const failed = vi.fn(() => {
        unsent = channel.unsentResultIds();
        channel.close();
      });
      const channel = new CodexVoiceProviderChannel(vi.fn(), failed, () => false, vi.fn());
      const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
      channel.bind(wire as unknown as RTCDataChannel);
      for (let i = 0; i <= 1024; i++) {
        channel.send(speakable("Result"), `result-${i}`);
        if (stage === "unconfirmed") {
          vi.advanceTimersByTime(150);
          if (i < 1024) {
            channel.observe(turn("turn.created", `speech-${i}`));
            channel.observe(turn("turn.done", `speech-${i}`));
          }
        }
      }
      expect(failed).toHaveBeenCalledExactlyOnceWith("provider_command_overflow");
      expect(unsent).toEqual(
        stage === "queued"
          ? Array.from({ length: 1025 }, (_, i) => `result-${i}`)
          : ["result-1024"],
      );
      expect(wire.send).toHaveBeenCalledTimes(stage === "queued" ? 0 : 1024);
      expect(vi.getTimerCount()).toBe(0);
    },
  );

  it("checks policy and strips host metadata even when a user turn starts during the check", async () => {
    vi.useFakeTimers();
    let resolve: ((value: unknown) => void) | undefined;
    const prepare = vi.fn(
      () =>
        new Promise<unknown>((done) => {
          resolve = done;
        }),
    );
    const submitted = vi.fn();
    const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      vi.fn(),
      () => false,
      vi.fn(),
      submitted,
      prepare,
    );
    channel.bind(wire as unknown as RTCDataChannel);
    channel.send(speakable("Unreviewed text"), "result-1");
    vi.advanceTimersByTime(150);
    expect(channel.unsentResultIds()).toEqual(["result-1"]);
    channel.observe(turn("turn.created", "user", "user"));
    resolve?.(speakable("Latest permitted text"));
    await vi.advanceTimersByTimeAsync(0);
    expect(prepare).toHaveBeenCalledTimes(1);
    expect(JSON.parse(wire.send.mock.calls[0][0])).toEqual(speakable("Latest permitted text"));
    expect(submitted).toHaveBeenCalledWith("result-1");
    expect(channel.unsentResultIds()).toEqual([]);
    channel.close();
  });

  it("does not send suppressed output or a preflight response arriving after close", async () => {
    vi.useFakeTimers();
    const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
    let resolve: ((value: unknown) => void) | undefined;
    const prepare = vi.fn(
      () =>
        new Promise<unknown>((done) => {
          resolve = done;
        }),
    );
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      vi.fn(),
      () => false,
      vi.fn(),
      vi.fn(),
      prepare,
    );
    channel.bind(wire as unknown as RTCDataChannel);
    channel.send(speakable("Viewed"), "result-1");
    vi.advanceTimersByTime(150);
    resolve?.(null);
    await vi.advanceTimersByTimeAsync(0);
    expect(channel.unsentResultIds()).toEqual([]);
    channel.send(speakable("Not sent before stopping"), "result-2");
    vi.advanceTimersByTime(150);
    expect(channel.unsentResultIds()).toEqual(["result-2"]);
    channel.close();
    resolve?.(speakable("Too late"));
    await vi.advanceTimersByTimeAsync(0);
    expect(wire.send).not.toHaveBeenCalled();
  });
  const speakable = (text: string, delegation?: string) => ({
    type: delegation ? "delegation.context.append" : "session.context.append",
    ...(delegation ? { delegation_item_id: delegation } : {}),
    channel: "speakable",
    content: [{ type: "input_text", text }],
  });
  const turn = (type: "turn.created" | "turn.done", id: string, role = "assistant") => ({
    type,
    turn: { id, role },
  });

  it.each([true, false])(
    "advances new output after a silent provider without replaying the earlier context (receipt=%s)",
    (acknowledged) => {
      const { channel, wire, unconfirmed } = fixture();
      channel.send(speakable("Earlier context."), "earlier-result");
      vi.advanceTimersByTime(150);
      if (acknowledged) channel.observe({ type: "session.context.appended" });
      channel.send(speakable("New group result."), "new-result");
      vi.advanceTimersByTime(149);
      expect(wire.send).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(1);
      expect(wire.send).toHaveBeenCalledTimes(2);
      expect(wire.send.mock.calls.map(([value]) => JSON.parse(value).content[0].text)).toEqual([
        "Earlier context.",
        "New group result.",
      ]);
      expect(channel.unsentResultIds()).toEqual([]);
      expect(channel.receipt().speech_turns_completed).toBe(0);
      vi.advanceTimersByTime(60_000);
      expect(wire.send).toHaveBeenCalledTimes(2);
      expect(unconfirmed).toHaveBeenCalled();
      channel.close();
    },
  );

  it("keeps delivery independent of interrupted user and assistant turn observations", () => {
    const { channel, wire } = fixture();
    channel.send(speakable("First."));
    vi.advanceTimersByTime(150);
    channel.send(speakable("Second."));
    channel.observe(turn("turn.created", "user", "user"));
    vi.advanceTimersByTime(30_000);
    expect(channel.outputStatus()).toEqual({ queued: 0, blocked: null });
    channel.observe(turn("turn.done", "user", "user"));
    // Late turn observations must not replay either submitted context.
    channel.observe(turn("turn.created", "late"));
    vi.advanceTimersByTime(60_000);
    expect(wire.send).toHaveBeenCalledTimes(2);
    channel.observe(turn("turn.done", "late"));
    vi.advanceTimersByTime(150);
    expect(wire.send).toHaveBeenCalledTimes(2);
    channel.close();
  });

  it("retries a transient preflight with fresh policy and leaves the following result in order", async () => {
    vi.useFakeTimers();
    const prepare = vi
      .fn()
      .mockRejectedValueOnce(failure("network_error"))
      .mockResolvedValueOnce(null)
      .mockResolvedValue(speakable("Second permitted result."));
    const failed = vi.fn();
    const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      failed,
      () => false,
      vi.fn(),
      vi.fn(),
      prepare,
    );
    channel.bind(wire as unknown as RTCDataChannel);
    channel.send(speakable("Now viewed."), "first");
    channel.send(speakable("Old second text."), "second");
    await vi.advanceTimersByTimeAsync(150);
    expect(channel.outputStatus()).toEqual({ queued: 2, blocked: "retrying" });
    expect(channel.unsentResultIds()).toEqual(["first", "second"]);
    await vi.advanceTimersByTimeAsync(1300);
    expect(prepare.mock.calls.map(([id]) => id)).toEqual(["first", "first", "second"]);
    expect(wire.send).toHaveBeenCalledTimes(1);
    expect(JSON.parse(wire.send.mock.calls[0][0]).content[0].text).toBe("Second permitted result.");
    expect(failed).not.toHaveBeenCalled();
    channel.close();
  });

  it("aborts a stalled preflight and can recover without any user or provider event", async () => {
    vi.useFakeTimers();
    const prepare = vi
      .fn()
      .mockImplementationOnce(
        (_id, signal: AbortSignal) =>
          new Promise((_resolve, reject) => {
            signal.addEventListener("abort", () => reject(failure("network_error")), {
              once: true,
            });
          }),
      )
      .mockResolvedValue(speakable("Recovered result."));
    const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      vi.fn(),
      () => false,
      vi.fn(),
      vi.fn(),
      prepare,
    );
    channel.bind(wire as unknown as RTCDataChannel);
    channel.send(speakable("Result."), "result");
    await vi.advanceTimersByTimeAsync(11_300);
    expect(prepare).toHaveBeenCalledTimes(2);
    expect(prepare.mock.calls[0][1].aborted).toBe(true);
    expect(wire.send).toHaveBeenCalledTimes(1);
    channel.close();
  });

  it.each(["network_error", "unauthorized", "bad_request", "http_error"])(
    "ends an unrecoverable preflight without submitting or losing its sources (%s)",
    async (code) => {
      vi.useFakeTimers();
      const prepare = vi.fn().mockRejectedValue(failure(code));
      const failed = vi.fn();
      const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
      const channel = new CodexVoiceProviderChannel(
        vi.fn(),
        failed,
        () => false,
        vi.fn(),
        vi.fn(),
        prepare,
      );
      channel.bind(wire as unknown as RTCDataChannel);
      channel.send(speakable("Retained."), "result");
      await vi.advanceTimersByTimeAsync(30_000);
      expect(prepare).toHaveBeenCalledTimes(code === "network_error" ? 4 : 1);
      expect(failed).toHaveBeenCalledExactlyOnceWith("notification_output_prepare_failed");
      expect(channel.unsentResultIds()).toEqual(["result"]);
      expect(wire.send).not.toHaveBeenCalled();
      channel.close();
    },
  );

  it("cancels a pending preflight on close", async () => {
    vi.useFakeTimers();
    const failed = vi.fn();
    let signal: AbortSignal | undefined;
    const prepare = vi.fn(
      (_id, nextSignal: AbortSignal) =>
        new Promise((_resolve, reject) => {
          signal = nextSignal;
          nextSignal.addEventListener("abort", () => reject(failure("network_error")), {
            once: true,
          });
        }),
    );
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      failed,
      () => false,
      vi.fn(),
      vi.fn(),
      prepare,
    );
    channel.bind({
      readyState: "open",
      send: vi.fn(),
      close: vi.fn(),
    } as unknown as RTCDataChannel);
    channel.send(speakable("Pending."), "result");
    await vi.advanceTimersByTimeAsync(150);
    channel.close();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(signal?.aborted).toBe(true);
    expect(prepare).toHaveBeenCalledTimes(1);
    expect(failed).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels a scheduled preflight retry on close", async () => {
    vi.useFakeTimers();
    const prepare = vi.fn().mockRejectedValue(failure("network_error"));
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      vi.fn(),
      () => false,
      vi.fn(),
      vi.fn(),
      prepare,
    );
    channel.bind({
      readyState: "open",
      send: vi.fn(),
      close: vi.fn(),
    } as unknown as RTCDataChannel);
    channel.send(speakable("Pending."), "result");
    await vi.advanceTimersByTimeAsync(150);
    expect(channel.outputStatus().blocked).toBe("retrying");
    channel.close();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(prepare).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not report an already submitted source as unsent when its host receipt fails", () => {
    vi.useFakeTimers();
    let unsent: string[] | undefined;
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      () => {
        unsent = channel.unsentResultIds();
        channel.close();
      },
      () => false,
      vi.fn(),
      () => {
        throw new Error("host receipt failed");
      },
    );
    const send = vi.fn();
    channel.bind({ readyState: "open", send, close: vi.fn() } as unknown as RTCDataChannel);
    channel.send(speakable("Already submitted."), "result");
    vi.advanceTimersByTime(150);
    expect(send).toHaveBeenCalledTimes(1);
    expect(unsent).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("retains a positively unsent source before handling data-channel failure", () => {
    vi.useFakeTimers();
    let unsent: string[] = [];
    const channel = new CodexVoiceProviderChannel(
      vi.fn(),
      () => {
        unsent = channel.unsentResultIds();
        channel.close();
      },
      () => false,
      vi.fn(),
    );
    channel.bind({
      readyState: "open",
      send: () => {
        throw new Error("closed");
      },
      close: vi.fn(),
    } as unknown as RTCDataChannel);
    channel.send(speakable("Retained."), "source-result");
    vi.advanceTimersByTime(150);
    expect(unsent).toEqual(["source-result"]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("delivers mid-speech updates as complete coalesced context", () => {
    const { channel, wire, failed } = fixture();
    channel.send(speakable("Amber 17."));
    vi.advanceTimersByTime(150);
    channel.observe(turn("turn.created", "first"));
    channel.send(speakable("Birch 29. "));
    channel.send(speakable("Coral 43."));
    channel.observe({ type: "session.context.appended" });
    vi.advanceTimersByTime(150);
    expect(wire.send).toHaveBeenCalledTimes(2);
    expect(channel.receipt().queued).toBe(0);
    channel.observe(turn("turn.done", "first"));
    vi.advanceTimersByTime(150);
    expect(wire.send).toHaveBeenCalledTimes(2);
    expect(JSON.parse(wire.send.mock.calls[1][0]).content[0].text).toBe("Birch 29. Coral 43.");
    expect(failed).not.toHaveBeenCalled();
    channel.close();
  });

  it("does not turn user speech into a barrier for context or control commands", () => {
    const { channel, wire } = fixture();
    channel.observe(turn("turn.created", "user-1", "user"));
    channel.send(speakable("A result."));
    channel.send({ type: "unrelated_control" });
    expect(wire.send).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(150);
    expect(wire.send).toHaveBeenCalledTimes(2);
    channel.observe(turn("turn.done", "user-1", "user"));
    // The provider controls speech without changing the delivered context.
    channel.observe(turn("turn.created", "answer"));
    vi.advanceTimersByTime(150);
    expect(wire.send).toHaveBeenCalledTimes(2);
    channel.observe(turn("turn.done", "answer"));
    vi.advanceTimersByTime(150);
    expect(wire.send).toHaveBeenCalledTimes(2);
    channel.close();
  });

  it("preserves delegation targets and does not replay results on duplicate completion events", () => {
    const { channel, wire, unconfirmed } = fixture();
    channel.send(speakable("First.", "request-1"));
    vi.advanceTimersByTime(150);
    channel.observe(turn("turn.created", "old"));
    channel.send(speakable("Second.", "request-2"));
    channel.observe(turn("turn.done", "old"));
    vi.advanceTimersByTime(150);
    channel.send(speakable("Third."));
    channel.observe({ type: "delegation.context.appended" });
    channel.observe({ type: "delegation.context.appended" });
    channel.observe(turn("turn.done", "old"));
    vi.advanceTimersByTime(60_000);
    expect(wire.send).toHaveBeenCalledTimes(3);
    expect(channel.receipt().queued).toBe(0);
    expect(unconfirmed).toHaveBeenCalledTimes(1);
    expect(JSON.parse(wire.send.mock.calls[1][0]).delegation_item_id).toBe("request-2");
    channel.close();
  });

  it("preserves split words and UTF-8 text and waits for the channel to open", () => {
    const { channel, wire, unconfirmed } = fixture("connecting");
    channel.send(speakable("The Am"));
    channel.send(speakable("ber 是 17。"));
    vi.advanceTimersByTime(10_000);
    expect(unconfirmed).not.toHaveBeenCalled();
    expect(wire.send).not.toHaveBeenCalled();
    wire.readyState = "open";
    wire.onopen?.();
    vi.advanceTimersByTime(150);
    expect(wire.send).toHaveBeenCalledTimes(1);
    expect(JSON.parse(wire.send.mock.calls[0][0]).content[0].text).toBe("The Amber 是 17。");
    channel.close();
  });

  it("immediately sends a burst, keeping receipt and speech completion distinct", () => {
    const { wire, channel, failed, unconfirmed } = fixture();
    for (let i = 0; i < 256; i += 1) {
      channel.send({
        type: "session.context.append",
        content: [{ type: "input_text", text: `fact ${i}` }],
      });
    }
    expect(wire.send).toHaveBeenCalledTimes(256);
    expect(channel.receipt()).toMatchObject({
      sent: 256,
      acknowledged: 0,
      pending: 256,
      speech_turns_completed: 0,
      queued: 0,
    });
    for (let i = 0; i < 256; i += 1) channel.observe({ type: "session.context.appended" });
    expect(channel.receipt()).toMatchObject({
      sent: 256,
      acknowledged: 256,
      pending: 0,
      speech_turns_completed: 0,
      queued: 0,
    });
    channel.observe({ type: "turn.done", turn: { role: "user" } });
    channel.observe({ type: "turn.done", turn: { role: "assistant" } });
    expect(channel.receipt().speech_turns_completed).toBe(1);
    vi.advanceTimersByTime(60_000);
    expect(unconfirmed).not.toHaveBeenCalled();
    expect(failed).not.toHaveBeenCalled();
    channel.close();
  });

  it("warns about a missing receipt without replaying or disconnecting", () => {
    const { wire, channel, failed, unconfirmed } = fixture();
    channel.send({ type: "delegation.context.append" });
    expect(channel.observe({ type: "session.context.appended" })).toBe(false);
    vi.advanceTimersByTime(30_000);
    expect(unconfirmed).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(60_000);
    expect(unconfirmed).toHaveBeenCalledTimes(1);
    expect(wire.send).toHaveBeenCalledTimes(1);
    expect(wire.close).not.toHaveBeenCalled();
    expect(failed).not.toHaveBeenCalled();
    channel.observe({ type: "delegation.context.appended" });
    expect(channel.receipt().pending).toBe(0);
    channel.send({ type: "session.context.append" });
    vi.advanceTimersByTime(30_000);
    expect(unconfirmed).toHaveBeenCalledTimes(2);
    channel.close();
  });

  it("starts receipt timeout only when the command is sent and cancels it on close", () => {
    const { wire, channel, unconfirmed } = fixture("connecting");
    channel.send({ type: "session.context.append" });
    vi.advanceTimersByTime(10_000);
    expect(unconfirmed).not.toHaveBeenCalled();
    expect(channel.receipt().sent).toBe(0);
    wire.readyState = "open";
    wire.onopen?.();
    expect(channel.receipt().pending).toBe(1);
    channel.close();
    vi.advanceTimersByTime(60_000);
    expect(unconfirmed).not.toHaveBeenCalled();
  });

  it("ages the actual oldest pending command rather than postponing warnings on every send", () => {
    const { channel, unconfirmed } = fixture();
    channel.send({ type: "session.context.append" });
    vi.advanceTimersByTime(20_000);
    channel.send({ type: "delegation.context.append" });
    channel.observe({ type: "session.context.appended" });
    vi.advanceTimersByTime(29_999);
    expect(unconfirmed).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(unconfirmed).toHaveBeenCalledTimes(1);
    channel.close();
  });
});
