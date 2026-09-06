import { afterEach, expect, it, vi } from "vitest";
import { CodexVoiceProviderChannel } from "./codexVoiceProviderChannel";

afterEach(() => vi.useRealTimers());

const context = (text: string, delegation?: string) => ({
  type: delegation ? "delegation.context.append" : "session.context.append",
  ...(delegation ? { delegation_item_id: delegation } : {}),
  channel: "speakable",
  content: [{ type: "input_text", text }],
});

function fixture(prepare?: (id: string, signal: AbortSignal) => Promise<unknown | null>) {
  vi.useFakeTimers();
  const wire = {
    readyState: "open" as RTCDataChannelState,
    bufferedAmount: 0,
    send: vi.fn(),
    close: vi.fn(),
    onbufferedamountlow: null as null | (() => void),
    onopen: null as null | (() => void),
  };
  let unsent: string[] = [];
  const submitted = vi.fn();
  const failed = vi.fn(() => {
    unsent = channel.unsentResultIds();
    channel.close();
  });
  const channel = new CodexVoiceProviderChannel(
    vi.fn(),
    failed,
    () => false,
    vi.fn(),
    submitted,
    prepare,
  );
  channel.bind(wire as unknown as RTCDataChannel);
  return { channel, wire, failed, submitted, unsent: () => unsent };
}

it.each(["user", "assistant"])(
  "delivers later results even when an earlier %s turn never ends",
  async (role) => {
    const { channel, wire, submitted } = fixture();
    channel.observe({ type: "turn.created", turn: { id: "unfinished", role } });
    channel.observe({ type: "turn.created", turn: { id: "later", role: "assistant" } });
    channel.observe({ type: "turn.done", turn: { id: "later", role: "assistant" } });
    for (let i = 0; i < 5; i++) channel.send(context(`Result ${i}.`), `source-${i}`);
    await vi.advanceTimersByTimeAsync(1000);
    expect(wire.send).toHaveBeenCalledTimes(5);
    expect(submitted.mock.calls.flat()).toEqual(Array.from({ length: 5 }, (_, i) => `source-${i}`));
    expect(channel.outputStatus()).toEqual({ queued: 0, blocked: null });
    channel.close();
  },
);

it("returns delegated results while their Realtime turn is waiting for the Analyst", async () => {
  const { channel, wire } = fixture();
  channel.observe({ type: "turn.created", turn: { id: "answer", role: "assistant" } });
  channel.observe({ type: "delegation.created", item: { id: "request" } });
  channel.send(context("Result needed to finish this turn.", "request"));
  await vi.advanceTimersByTimeAsync(150);
  expect(JSON.parse(wire.send.mock.calls[0]?.[0] || "null")).toEqual(
    context("Result needed to finish this turn.", "request"),
  );
  channel.close();
});

it("continues delivery without receipts, speech starts or speech completion events", async () => {
  const { channel, wire, submitted } = fixture();
  channel.send(context("First."), "first");
  await vi.advanceTimersByTimeAsync(150);
  channel.send(context("Second."), "second");
  await vi.advanceTimersByTimeAsync(150);
  expect(wire.send).toHaveBeenCalledTimes(2);
  expect(submitted.mock.calls.flat()).toEqual(["first", "second"]);
  expect(channel.receipt()).toMatchObject({ pending: 2, speech_turns_completed: 0, queued: 0 });
  await vi.advanceTimersByTimeAsync(60_000);
  expect(wire.send).toHaveBeenCalledTimes(2);
  channel.close();
});

it("enforces preflight's deadline even when the request ignores abort; late text is never sent", async () => {
  let finishOld: ((value: unknown) => void) | undefined;
  const prepare = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishOld = resolve;
        }),
    )
    .mockResolvedValue(context("Current permitted result."));
  const { channel, wire, submitted } = fixture(prepare);
  channel.send(context("Unreviewed."), "source");
  await vi.advanceTimersByTimeAsync(11_500);
  expect(prepare).toHaveBeenCalledTimes(2);
  expect(prepare.mock.calls[0][1].aborted).toBe(true);
  expect(submitted).toHaveBeenCalledExactlyOnceWith("source");
  finishOld?.(context("Obsolete result."));
  await vi.advanceTimersByTimeAsync(1000);
  expect(wire.send).toHaveBeenCalledTimes(1);
  expect(JSON.parse(wire.send.mock.calls[0][0]).content[0].text).toBe("Current permitted result.");
  channel.close();
});

it("waits for actual transport capacity, then rechecks current policy and delivers in order", async () => {
  const prepare = vi.fn().mockResolvedValueOnce(null).mockResolvedValue(context("Allowed."));
  const { channel, wire, submitted } = fixture(prepare);
  wire.bufferedAmount = 512 * 1024;
  channel.send(context("Now viewed."), "viewed");
  channel.send(context("Allowed."), "allowed");
  await vi.advanceTimersByTimeAsync(2000);
  expect(wire.send).not.toHaveBeenCalled();
  expect(prepare).not.toHaveBeenCalled();
  expect(channel.outputStatus()).toEqual({ queued: 2, blocked: "backpressure" });
  wire.bufferedAmount = 0;
  wire.onbufferedamountlow?.();
  await vi.advanceTimersByTimeAsync(500);
  expect(submitted).toHaveBeenCalledExactlyOnceWith("allowed");
  expect(prepare.mock.calls.map(([id]) => id)).toEqual(["viewed", "allowed"]);
  channel.close();
  expect(vi.getTimerCount()).toBe(0);
});

it.each(["connection", "backpressure"])(
  "fails explicitly and releases positively unsent results if %s never recovers",
  async (reason) => {
    const { channel, wire, failed, unsent } = fixture();
    if (reason === "connection") wire.readyState = "connecting";
    else wire.bufferedAmount = 512 * 1024;
    channel.send(context("Retained."), "first");
    await vi.advanceTimersByTimeAsync(9000);
    channel.send(context("Also retained."), "second");
    await vi.advanceTimersByTimeAsync(7000);
    expect(failed).toHaveBeenCalledExactlyOnceWith("provider_output_stalled");
    expect(unsent()).toEqual(["first", "second"]);
    expect(wire.send).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    wire.readyState = "open";
    wire.bufferedAmount = 0;
    wire.onopen?.();
    wire.onbufferedamountlow?.();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(wire.send).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  },
);

it("stops after bounded retries when every preflight ignores cancellation", async () => {
  const prepare = vi.fn(() => new Promise<unknown>(() => {}));
  const { channel, wire, failed, unsent } = fixture(prepare);
  channel.send(context("First retained."), "first");
  channel.send(context("Second retained."), "second");
  await vi.advanceTimersByTimeAsync(60_000);
  expect(prepare).toHaveBeenCalledTimes(4);
  expect(failed).toHaveBeenCalledExactlyOnceWith("notification_output_prepare_failed");
  expect(unsent()).toEqual(["first", "second"]);
  expect(wire.send).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});

it("rechecks suppression if capacity is lost during preflight and then recovers", async () => {
  let prepared: ((result: unknown) => void) | undefined;
  const prepare = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          prepared = resolve;
        }),
    )
    .mockResolvedValueOnce(null)
    .mockResolvedValue(context("Next result."));
  const { channel, wire, submitted } = fixture(prepare);
  channel.send(context("First."), "viewed-later");
  channel.send(context("Next."), "next");
  await vi.advanceTimersByTimeAsync(150);
  wire.bufferedAmount = 512 * 1024;
  prepared?.(context("Allowed at the previous check."));
  await vi.advanceTimersByTimeAsync(0);
  expect(channel.outputStatus()).toEqual({ queued: 2, blocked: "backpressure" });
  expect(channel.unsentResultIds()).toEqual(["viewed-later", "next"]);
  wire.bufferedAmount = 0;
  wire.onbufferedamountlow?.();
  await vi.advanceTimersByTimeAsync(500);
  expect(prepare.mock.calls.map(([id]) => id)).toEqual(["viewed-later", "viewed-later", "next"]);
  expect(submitted).toHaveBeenCalledExactlyOnceWith("next");
  expect(wire.send).toHaveBeenCalledTimes(1);
  channel.close();
});

it("a stalled buffer never releases an earlier submitted result as unsent", async () => {
  const { channel, wire, submitted, unsent } = fixture();
  wire.send.mockImplementation(() => {
    wire.bufferedAmount = 512 * 1024;
  });
  channel.send(context("Already submitted."), "first");
  channel.send(context("Still pending."), "second");
  await vi.advanceTimersByTimeAsync(16_000);
  expect(wire.send).toHaveBeenCalledTimes(1);
  expect(submitted).toHaveBeenCalledExactlyOnceWith("first");
  expect(unsent()).toEqual(["second"]);
  expect(vi.getTimerCount()).toBe(0);
});
