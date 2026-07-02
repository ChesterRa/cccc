import { afterEach, describe, expect, it, vi } from "vitest";

import {
  dispatchSlashMessageOptimistically,
} from "../../src/hooks/useSlashCommands";

afterEach(() => {
  vi.clearAllMocks();
});

describe("dispatchSlashMessageOptimistically", () => {
  it("clears the composer before the slash dispatch request settles", async () => {
    const calls: string[] = [];
    let resolveDispatch: (value: boolean) => void = () => {};
    const dispatchPromise = new Promise<boolean>((resolve) => {
      resolveDispatch = resolve;
    });

    const resultPromise = dispatchSlashMessageOptimistically({
      dispatchText: "/install https://github.com/obra/superpowers",
      originalText: "/install https://github.com/obra/superpowers",
      dispatchMessage: async () => {
        calls.push("dispatch-start");
        return dispatchPromise;
      },
      clearComposer: () => calls.push("clear"),
      restoreComposerText: (text) => calls.push(`restore:${text}`),
    });

    await Promise.resolve();

    expect(calls).toEqual(["clear", "dispatch-start"]);

    resolveDispatch(true);
    await expect(resultPromise).resolves.toMatchObject({ ok: true });
    expect(calls).toEqual(["clear", "dispatch-start"]);
  });

  it("passes reply context to the slash dispatch request", async () => {
    const replyTarget = {
      eventId: "evt-original",
      by: "foreman",
      text: "请基于这个失败日志继续处理",
    };
    const dispatchMessage = vi.fn(async () => true);

    await expect(dispatchSlashMessageOptimistically({
      dispatchText: "请使用已激活的 /using-superpowers skill 完成以下任务：\n\n开始执行",
      originalText: "/using-superpowers 开始执行",
      replyTarget,
      dispatchMessage,
      clearComposer: vi.fn(),
      restoreComposerText: vi.fn(),
    })).resolves.toMatchObject({ ok: true });

    expect(dispatchMessage).toHaveBeenCalledWith(
      "请使用已激活的 /using-superpowers skill 完成以下任务：\n\n开始执行",
      { replyTarget },
    );
  });

  it("passes slash skill metadata without turning it into visible chat text", async () => {
    const dispatchMessage = vi.fn(async () => true);

    await expect(dispatchSlashMessageOptimistically({
      dispatchText: "开始执行",
      originalText: "/using-superpowers 开始执行",
      command: "/using-superpowers",
      capabilityId: "skill:agent_self_proposed:using-superpowers",
      dispatchMessage,
      clearComposer: vi.fn(),
      restoreComposerText: vi.fn(),
    })).resolves.toMatchObject({ ok: true });

    expect(dispatchMessage).toHaveBeenCalledWith(
      "开始执行",
      {
        replyTarget: null,
        command: "/using-superpowers",
        capabilityId: "skill:agent_self_proposed:using-superpowers",
      },
    );
  });

  it("restores the original slash text when dispatch fails", async () => {
    const calls: string[] = [];

    await expect(dispatchSlashMessageOptimistically({
      dispatchText: "/install bad-target",
      originalText: "/install bad-target",
      dispatchMessage: async () => false,
      clearComposer: () => calls.push("clear"),
      restoreComposerText: (text) => calls.push(`restore:${text}`),
    })).resolves.toMatchObject({ ok: false });

    expect(calls).toEqual(["clear", "restore:/install bad-target"]);
  });
});
