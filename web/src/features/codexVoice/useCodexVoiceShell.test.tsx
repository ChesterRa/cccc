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

vi.mock("./useCodexVoiceAnalystSettings", () => ({
  useCodexVoiceAnalystSettings: () => ({ hasChanges: false }),
}));

import { useCodexVoiceShell } from "./useCodexVoiceShell";
import { useModalStore } from "../../stores";

describe("useCodexVoiceShell", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    controller.analyst = null;
    controller.isEngaged = false;
    controller.start.mockClear();
    useModalStore.setState({ settingsTarget: null });
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
  it("opens the same settings editor from global management without starting a call", async () => {
    let voice!: ReturnType<typeof useCodexVoiceShell>;
    function Probe() {
      voice = useCodexVoiceShell(true);
      return <span>{voice.detailsOpen ? "open" : "closed"}</span>;
    }
    await act(async () => root.render(<Probe />));
    await act(async () => voice.openDetails());
    await act(async () => useModalStore.getState().openCodexVoiceSettings());
    expect(voice.detailsOpen).toBe(true);
    expect(useModalStore.getState().modals.settings).toBe(true);
    expect(useModalStore.getState().settingsTarget).toMatchObject({
      scope: "global",
      tab: "voice",
      voiceSection: "realtime",
    });
    expect(controller.start).not.toHaveBeenCalled();
    await act(async () => useModalStore.getState().closeModal("settings"));
    expect(voice.detailsOpen).toBe(true);
    await act(async () => voice.closeDetails());
    expect(voice.detailsOpen).toBe(false);
  });
});
