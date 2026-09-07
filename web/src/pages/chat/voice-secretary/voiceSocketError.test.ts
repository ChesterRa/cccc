import { describe, expect, it, vi } from "vite-plus/test";
import { queueVoiceSocketError } from "./voiceSocketError";

describe("voice socket error ordering", () => {
  it("does not report a transport error while a received closed event is finalizing", async () => {
    let finish!: () => void;
    const finishing = new Promise<void>((resolve) => {
      finish = resolve;
    });
    let expectedClose = false;
    const closed = Promise.resolve().then(async () => {
      expectedClose = true;
      await finishing;
    });
    const report = vi.fn();
    const error = queueVoiceSocketError(closed, () => expectedClose, report);
    await Promise.resolve();
    expect(report).not.toHaveBeenCalled();
    finish();
    await error;
    expect(report).not.toHaveBeenCalled();
  });

  it("waits for earlier async transcript work before checking the closed marker", async () => {
    let finish!: () => void;
    let expectedClose = false;
    const transcript = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const closed = transcript.then(() => {
      expectedClose = true;
    });
    const report = vi.fn();
    const error = queueVoiceSocketError(closed, () => expectedClose, report);
    expect(expectedClose).toBe(false);
    expect(report).not.toHaveBeenCalled();
    finish();
    await error;
    expect(report).not.toHaveBeenCalled();
  });

  it("still reports unexpected connection failures", async () => {
    const report = vi.fn();
    await queueVoiceSocketError(Promise.resolve(), () => false, report);
    expect(report).toHaveBeenCalledOnce();
  });

  it("ignores errors from a run which ended while earlier messages were processing", async () => {
    let active = true;
    const pending = Promise.resolve().then(() => {
      active = false;
    });
    const report = vi.fn();
    await queueVoiceSocketError(pending, () => !active, report);
    expect(report).not.toHaveBeenCalled();
  });
});
