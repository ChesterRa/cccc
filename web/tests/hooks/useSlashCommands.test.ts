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
