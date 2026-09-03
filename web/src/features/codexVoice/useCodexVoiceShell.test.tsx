// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

const controller = vi.hoisted(() => ({
  analyst: null as { group_id: string } | null,
  isEngaged: false,
  start: vi.fn(async () => undefined),
}));

vi.mock("./useCodexVoiceSessionController", () => ({
  useCodexVoiceSessionController: () => controller,
}));

import { useCodexVoiceShell } from "./useCodexVoiceShell";

describe("useCodexVoiceShell", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    controller.analyst = null;
    controller.isEngaged = false;
    controller.start.mockClear();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("starts voice directly without opening the details surface", async () => {
    function Probe() {
      const voice = useCodexVoiceShell(true);
      return (
        <button type="button" data-open={String(voice.detailsOpen)} onClick={voice.start}>
          start
        </button>
      );
    }

    await act(async () => root.render(<Probe />));
    const button = host.querySelector<HTMLButtonElement>("button");
    await act(async () => button?.click());

    expect(controller.start).toHaveBeenCalledWith();
    expect(button?.dataset.open).toBe("false");
  });
});
