import { afterEach, expect, it, vi } from "vitest";
import { CodexVoiceProviderChannel } from "./codexVoiceProviderChannel";

afterEach(() => vi.useRealTimers());

function command(text: string, delegation?: string) {
  return {
    type: delegation ? "delegation.context.append" : "session.context.append",
    ...(delegation ? { delegation_item_id: delegation } : {}),
    channel: "speakable",
    content: [{ type: "input_text", text }],
  };
}

it("splits the full post-preflight notification below the provider limit without losing text", async () => {
  vi.useFakeTimers();
  const text =
    "消息来自 temp_task 组的管理员。乌鲁木齐 22°C，体感 16°C，北风 17 km/h，沙尘，湿度 16%。".repeat(
      20,
    );
  const prepare = vi.fn(async () => command(text));
  const submitted = vi.fn();
  const failed = vi.fn();
  const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
  const channel = new CodexVoiceProviderChannel(
    vi.fn(),
    failed,
    () => false,
    vi.fn(),
    submitted,
    prepare,
  );
  channel.bind(wire as unknown as RTCDataChannel);
  channel.send(command("Stale pre-preflight text"), "result-1");
  await vi.advanceTimersByTimeAsync(150);
  const frames = wire.send.mock.calls.map(([raw]) => JSON.parse(String(raw)));
  expect(frames.length).toBeGreaterThan(1);
  expect(frames.map((frame) => frame.content[0].text).join("")).toBe(text);
  for (const frame of frames) {
    expect(new TextEncoder().encode(frame.content[0].text).length).toBeLessThanOrEqual(500);
    expect(frame.type).toBe("session.context.append");
    expect(frame).not.toHaveProperty("_cccc_result_id");
  }
  expect(prepare).toHaveBeenCalledTimes(1);
  expect(submitted).toHaveBeenCalledExactlyOnceWith("result-1");
  expect(failed).not.toHaveBeenCalled();
  expect(channel.receipt()).toMatchObject({
    sent: frames.length,
    pending: frames.length,
    queued: 0,
  });
  channel.close();
});

it("bounds merged delegation fragments and sends the complete text in one scheduling turn", () => {
  vi.useFakeTimers();
  const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
  const channel = new CodexVoiceProviderChannel(vi.fn(), vi.fn(), () => false, vi.fn());
  channel.bind(wire as unknown as RTCDataChannel);
  const pieces = ["A".repeat(499), "🌦️長春 17–26°C\n".repeat(70), "End."];
  for (const piece of pieces) channel.send(command(piece, "delegation-1"));
  vi.advanceTimersByTime(150);
  const frames = wire.send.mock.calls.map(([raw]) => JSON.parse(String(raw)));
  expect(frames.map((frame) => frame.content[0].text).join("")).toBe(pieces.join(""));
  expect(frames.length).toBeGreaterThan(3);
  for (const frame of frames) {
    expect(new TextEncoder().encode(frame.content[0].text).length).toBeLessThanOrEqual(500);
    expect(frame.delegation_item_id).toBe("delegation-1");
    expect(frame.type).toBe("delegation.context.append");
  }
  expect(channel.receipt()).toMatchObject({ queued: 0, blocked: null });
  for (const _frame of frames) channel.observe({ type: "delegation.context.appended" });
  expect(channel.receipt()).toMatchObject({
    acknowledged: frames.length,
    pending: 0,
    speech_turns_completed: 0,
  });
  channel.close();
});

it.each([0, 1])("retains only definitely unsent output when fragment %s fails", (failureIndex) => {
  vi.useFakeTimers();
  let count = 0;
  let unsent: string[] = [];
  const submitted = vi.fn();
  const failed = vi.fn(() => {
    unsent = channel.unsentResultIds();
    channel.close();
  });
  const channel = new CodexVoiceProviderChannel(vi.fn(), failed, () => false, vi.fn(), submitted);
  channel.bind({
    readyState: "open",
    send: () => {
      if (count++ === failureIndex) throw new Error("fixture send failure");
    },
    close: vi.fn(),
  } as unknown as RTCDataChannel);
  channel.send(command("数值 22°C，不能丢失。".repeat(80)), "result-1");
  vi.advanceTimersByTime(150);
  expect(failed).toHaveBeenCalledExactlyOnceWith("provider_command_failed");
  expect(submitted).not.toHaveBeenCalled();
  expect(unsent).toEqual(failureIndex === 0 ? ["result-1"] : []);
  expect(vi.getTimerCount()).toBe(0);
});

it("rejects a whole output before its first fragment when receipt capacity is insufficient", () => {
  vi.useFakeTimers();
  let unsent: string[] = [];
  const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
  const failed = vi.fn(() => {
    unsent = channel.unsentResultIds();
    channel.close();
  });
  const channel = new CodexVoiceProviderChannel(vi.fn(), failed, () => false, vi.fn());
  channel.bind(wire as unknown as RTCDataChannel);
  for (let i = 0; i < 1023; i++) {
    channel.send({
      type: "session.context.append",
      content: [{ type: "input_text", text: "fixture" }],
    });
  }
  expect(wire.send).toHaveBeenCalledTimes(1023);
  channel.send(command("A".repeat(501)), "rejected-result");
  vi.advanceTimersByTime(150);
  expect(wire.send).toHaveBeenCalledTimes(1023);
  expect(failed).toHaveBeenCalledExactlyOnceWith("provider_command_overflow");
  expect(unsent).toEqual(["rejected-result"]);
  expect(vi.getTimerCount()).toBe(0);
});
