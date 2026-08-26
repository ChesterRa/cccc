// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const recoverDynamicImportError = vi.hoisted(() => vi.fn(() => true));

vi.mock("../../../src/utils/vitePreloadRecovery", () => ({ recoverDynamicImportError }));

vi.mock("../../../src/pages/chat/VoiceSecretaryComposerControl", () => {
  throw new TypeError("Failed to fetch dynamically imported module: /ui/voice-secretary.js");
});

describe("LazyVoiceSecretaryComposerControl load failure", () => {
  it("recovers a stale preload without throwing into the React tree", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { LazyVoiceSecretaryComposerControl } =
      await import("../../../src/pages/chat/LazyVoiceSecretaryComposerControl");
    const host = document.createElement("div");
    const root = createRoot(host);

    await act(async () =>
      root.render(
        <LazyVoiceSecretaryComposerControl
          isDark={false}
          selectedGroupId="g-1"
          busy=""
          variant="assistantRow"
        />,
      ),
    );

    await vi.waitFor(() => expect(recoverDynamicImportError).toHaveBeenCalledTimes(1));
    const fallback = host.querySelector<HTMLButtonElement>("button");
    expect(fallback?.getAttribute("aria-label")).toMatch(/failed|加载失败|読み込めません/i);
    expect(consoleError).not.toHaveBeenCalled();

    await act(async () => root.unmount());
    consoleError.mockRestore();
  });
});
