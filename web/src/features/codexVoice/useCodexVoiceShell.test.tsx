// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

const controller = vi.hoisted(() => ({
  analyst: null as { generation: string; group_id?: string } | null,
  owned: false,
  isEngaged: false,
  start: vi.fn(async () => undefined),
  refresh: vi.fn(async () => undefined),
}));

const chetState = vi.hoisted(() => ({
  status: null as {
    generation: string | null;
    preparing: string[];
    queued: string[];
    working: string[];
    history_write_failed: boolean;
    start_problem: null;
    owner_busy: boolean;
  } | null,
  appliedStatusSeq: 0,
}));

vi.mock("./useCodexVoiceSessionController", () => ({
  useCodexVoiceSessionController: () => controller,
}));

vi.mock("./useCodexVoiceAnalystSettings", () => ({
  useCodexVoiceAnalystSettings: () => ({ hasChanges: false }),
}));

// The shell hosts Chet, whose polling would otherwise leave real requests open at teardown.
vi.mock("../chet/useChetConversation", async () => {
  const { chetConversationStub } = await import("../chet/chetTestSupport");
  return {
    useChetConversation: () =>
      chetConversationStub({
        status: chetState.status,
        appliedStatusSeq: chetState.appliedStatusSeq,
      }),
  };
});

import { useCodexVoiceShell } from "./useCodexVoiceShell";
import { useModalStore } from "../../stores";

describe("useCodexVoiceShell", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    controller.analyst = null;
    controller.owned = false;
    controller.isEngaged = false;
    controller.start.mockClear();
    controller.refresh.mockClear();
    chetState.status = null;
    chetState.appliedStatusSeq = 0;
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

  it("refreshes the foreground controller when Chet starts in the background and retries after transient refresh failure", async () => {
    function Probe() {
      useCodexVoiceShell(true);
      return null;
    }

    // Initial null status (network/before first read) must never trigger refresh.
    await act(async () => root.render(<Probe />));
    expect(controller.refresh).not.toHaveBeenCalled();

    // Background unattended start reports generation "gen-1"; first refresh fails silently (analyst stays null).
    chetState.status = {
      generation: "gen-1",
      preparing: [],
      queued: [],
      working: ["i1"],
      history_write_failed: false,
      start_problem: null,
      owner_busy: false,
    };
    chetState.appliedStatusSeq = 1;
    await act(async () => root.render(<Probe />));
    expect(controller.refresh).toHaveBeenCalledTimes(1);
    expect(controller.refresh).toHaveBeenLastCalledWith(false);

    // Next status tick still sees "gen-1" while controller.analyst is still null: must retry!
    chetState.appliedStatusSeq = 2;
    await act(async () => root.render(<Probe />));
    expect(controller.refresh).toHaveBeenCalledTimes(2);

    // Once controller catches up to "gen-1", further ticks do not refresh again.
    controller.analyst = { generation: "gen-1" };
    chetState.appliedStatusSeq = 3;
    await act(async () => root.render(<Probe />));
    expect(controller.refresh).toHaveBeenCalledTimes(2);

    // Transient /chet/status failure sets status to null: must NOT trigger refresh.
    chetState.status = null;
    chetState.appliedStatusSeq = 4;
    await act(async () => root.render(<Probe />));
    expect(controller.refresh).toHaveBeenCalledTimes(2);

    // owner_busy reporting generation: null must NOT clear analyst; free owner reporting null DOES refresh.
    chetState.status = {
      generation: null,
      preparing: [],
      queued: [],
      working: [],
      history_write_failed: false,
      start_problem: null,
      owner_busy: true,
    };
    chetState.appliedStatusSeq = 5;
    await act(async () => root.render(<Probe />));
    expect(controller.refresh).toHaveBeenCalledTimes(2);

    chetState.status = { ...chetState.status, owner_busy: false };
    chetState.appliedStatusSeq = 6;
    await act(async () => root.render(<Probe />));
    expect(controller.refresh).toHaveBeenCalledTimes(3);
  });
});
