import { afterEach, describe, expect, it, vi } from "vitest";
import { CodexVoiceProviderChannel } from "./codexVoiceProviderChannel";

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
  it("immediately sends a burst, keeping receipt and speech completion distinct", () => {
    const { wire, channel, failed, unconfirmed } = fixture();
    for (let i = 0; i < 256; i += 1) {
      channel.send({
        type: "session.context.append",
        content: [{ type: "input_text", text: `fact ${i}` }],
      });
    }
    expect(wire.send).toHaveBeenCalledTimes(256);
    expect(channel.receipt()).toEqual({
      sent: 256,
      acknowledged: 0,
      pending: 256,
      speech_turns_completed: 0,
    });
    for (let i = 0; i < 256; i += 1) channel.observe({ type: "session.context.appended" });
    expect(channel.receipt()).toEqual({
      sent: 256,
      acknowledged: 256,
      pending: 0,
      speech_turns_completed: 0,
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
    vi.advanceTimersByTime(60_000);
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
