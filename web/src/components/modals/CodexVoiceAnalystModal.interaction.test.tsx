// @vitest-environment happy-dom

import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { useModalStore } from "../../stores";
import { CodexVoiceAnalystModal } from "./CodexVoiceAnalystModal";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../../services/api/codexVoice", () => ({
  fetchVoicePreferences: vi.fn(async () => ({
    ok: true,
    result: {
      preferences: {
        revision: 0,
        groups: {},
        suppress_viewed: true,
        verbosity: "standard",
        style: "natural",
      },
    },
  })),
  fetchVoiceNotifications: vi.fn(async () => ({
    ok: true,
    result: { messages: [], pending_count: 0, unconfirmed_count: 0 },
  })),
}));
vi.mock("../../features/codexVoice/VoiceAnalystTerminal", () => ({
  VoiceAnalystTerminal: ({ isVisible }: { isVisible: boolean }) => (
    <div data-visible={String(isVisible)}>embedded-analyst-terminal</div>
  ),
}));
vi.mock("../../features/codexVoice/CodexVoiceAnalystSettings", () => ({
  CodexVoiceAnalystSettings: ({ active }: { active: boolean }) => (
    <div data-analyst-settings-active={String(active)}>analyst-settings</div>
  ),
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function controller(): CodexVoiceSessionController {
  return {
    audioDeviceSnapshot: null,
    audioRef: createRef<HTMLAudioElement>(),
    phase: "listening",
    call: null,
    analyst: {
      generation: "analyst-1",
      tui_ready: true,
      phase: "ready",
      last_result: "",
      warning: "",
    },
    owned: true,
    checking: false,
    conversation: [],
    notificationPaused: false,
    microphoneMuted: false,
    playbackBlocked: false,
    outputStatus: { queued: 0, blocked: null },
    error: "",
    refreshError: "",
    isStarting: false,
    isEngaged: true,
    externalCall: false,
    analystWorking: false,
    analystWarning: "",
    preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
    supportedVoices: ["cove"],
    readiness: {
      analyst_runtime: "codex",
      analyst_runtime_available: true,
      supported_modes: ["assistant", "persona"],
      realtime_credentials_available: true,
    },
    updatePreferences: vi.fn(),
    updateAnalystSnapshot: vi.fn(),
    refresh: vi.fn(async () => undefined),
    start: vi.fn(async () => undefined),
    disconnect: vi.fn(async () => undefined),
    cancelInvestigation: vi.fn(async () => true),
    toggleMicrophone: vi.fn(),
    resumeAudio: vi.fn(async () => undefined),
    startNewAnalyst: vi.fn(async () => true),
    clearError: vi.fn(),
  };
}

function buttonByLabel(host: HTMLElement, label: string): HTMLButtonElement {
  const button = host.querySelector(`button[aria-label="${label}"]`);
  if (!(button instanceof HTMLButtonElement)) throw new Error(`button not found: ${label}`);
  return button;
}

describe("CodexVoiceAnalystModal settings navigation", () => {
  it("disconnects a collapsed desktop terminal even after the Analyst phone tab was selected", async () => {
    let desktop = false;
    let changed = () => {};
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: () => ({
        get matches() {
          return desktop;
        },
        addEventListener: (_: string, listener: () => void) => {
          changed = listener;
        },
        removeEventListener: vi.fn(),
      }),
    });
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () =>
      root.render(
        <CodexVoiceAnalystModal
          isOpen
          isDark={false}
          isSmallScreen={false}
          controller={controller()}
          onClose={vi.fn()}
        />,
      ),
    );
    const terminal = host.querySelector("[data-visible]");
    const clickText = async (text: string) => {
      const button = [...host.querySelectorAll("button")].find(
        (button) => button.textContent === text,
      );
      if (!button) throw new Error(`Missing button: ${text}`);
      await act(async () => button.click());
    };
    expect(terminal?.getAttribute("data-visible")).toBe("false");
    await clickText("codexVoiceAnalystTitle");
    expect(terminal?.getAttribute("data-visible")).toBe("true");
    await act(async () => {
      desktop = true;
      changed();
    });
    expect(terminal?.getAttribute("data-visible")).toBe("false");
    expect(host.querySelector("[data-visible]")).toBe(terminal);
    await clickText("codexVoiceShowAnalyst");
    expect(terminal?.getAttribute("data-visible")).toBe("true");
    await clickText("codexVoiceHideAnalyst");
    expect(terminal?.getAttribute("data-visible")).toBe("false");
    await act(async () => root.unmount());
  });

  it("opens the common realtime settings without starting or stopping the call", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const voice = controller();
    const onClose = vi.fn();
    await act(async () =>
      root.render(
        <CodexVoiceAnalystModal
          isOpen
          isDark={false}
          isSmallScreen={false}
          controller={voice}
          onClose={onClose}
        />,
      ),
    );
    await act(async () => buttonByLabel(host, "codexVoiceSettings").click());
    expect(useModalStore.getState().modals.settings).toBe(true);
    expect(useModalStore.getState().settingsTarget).toMatchObject({
      scope: "global",
      tab: "voice",
      voiceSection: "realtime",
    });
    expect(useModalStore.getState().settingsTarget?.voiceAnalyst).toBeUndefined();
    expect(host.querySelector("[data-codex-voice-settings-panel]")).toBeNull();
    expect(voice.start).not.toHaveBeenCalled();
    expect(voice.disconnect).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    await act(async () => root.unmount());
  });

  it.each(["", "codexVoiceAnalystRuntimeMissing"])(
    "takes an Analyst setup problem directly to settings after error=%s without changing the gear",
    async (error) => {
      const host = document.createElement("div");
      document.body.append(host);
      const root = createRoot(host);
      const voice = {
        ...controller(),
        phase: error ? ("failed" as const) : ("idle" as const),
        error,
        isEngaged: false,
        owned: false,
        analyst: null,
        readiness: {
          analyst_runtime: "codex",
          analyst_runtime_available: false,
          supported_modes: ["assistant", "persona"] as ("assistant" | "persona")[],
          realtime_credentials_available: true,
        },
      };
      await act(async () =>
        root.render(
          <CodexVoiceAnalystModal
            isOpen
            isDark={false}
            isSmallScreen={false}
            controller={voice}
            onClose={vi.fn()}
          />,
        ),
      );
      const settings = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
        (button) => button.textContent === "codexVoiceOpenSettings",
      );
      expect(settings).toBeDefined();
      if (error) expect(host.textContent?.split(error)).toHaveLength(2);
      await act(async () => settings!.click());
      expect(useModalStore.getState().settingsTarget).toMatchObject({
        scope: "global",
        tab: "voice",
        voiceSection: "realtime",
        voiceAnalyst: true,
      });
      await act(async () => buttonByLabel(host, "codexVoiceSettings").click());
      expect(useModalStore.getState().settingsTarget?.voiceAnalyst).toBeUndefined();
      expect(voice.start).not.toHaveBeenCalled();
      expect(voice.disconnect).not.toHaveBeenCalled();
      await act(async () => root.unmount());
    },
  );
});
