// @vitest-environment happy-dom

import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
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
      realtime_credentials_available: true,
    },
    updatePreferences: vi.fn(),
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
    await clickText("codexVoiceHideAnalyst");
    expect(terminal?.getAttribute("data-visible")).toBe("false");
    expect(host.querySelector("[data-visible]")).toBe(terminal);
    await clickText("codexVoiceShowAnalyst");
    expect(terminal?.getAttribute("data-visible")).toBe("true");
    await act(async () => root.unmount());
  });

  it("opens settings in the same dialog without remounting or disconnecting its terminal", async () => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn(() => ({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: {
        enumerateDevices: vi.fn(async () => []),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      },
    });
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const onClose = vi.fn();

    await act(async () => {
      root.render(
        <CodexVoiceAnalystModal
          isOpen
          isDark={false}
          isSmallScreen={false}
          controller={controller()}
          onClose={onClose}
        />,
      );
    });
    const terminalBefore = host.querySelector("[data-visible='true']");
    expect(terminalBefore).not.toBeNull();

    const settingsButton = buttonByLabel(host, "codexVoiceSettings");
    await act(async () => settingsButton.click());

    const panel = host.querySelector("[data-codex-voice-settings-panel='true']");
    const consoleSurface = host.querySelector("[data-codex-voice-console='true']");
    expect(panel).not.toBeNull();
    expect(host.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(consoleSurface?.hasAttribute("hidden")).toBe(true);
    expect(consoleSurface?.hasAttribute("inert")).toBe(true);
    expect(consoleSurface?.getAttribute("aria-hidden")).toBe("true");
    expect(host.querySelector("[data-visible='true']")).toBe(terminalBefore);
    expect(document.activeElement?.id).toBe("codex-voice-settings-audio-tab");

    const analystTab = host.querySelector("#codex-voice-settings-analyst-tab");
    if (!(analystTab instanceof HTMLButtonElement)) throw new Error("analyst tab not found");
    await act(async () => analystTab.click());
    expect(analystTab.getAttribute("aria-selected")).toBe("true");
    expect(host.querySelector("[data-analyst-settings-active='true']")).not.toBeNull();

    const done = [...host.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === "codexVoiceBackToConversation",
    );
    if (!(done instanceof HTMLButtonElement)) throw new Error("done button not found");
    await act(async () => done.click());

    expect(host.querySelector("#codex-voice-settings-page")?.hasAttribute("hidden")).toBe(true);
    expect(consoleSurface?.hasAttribute("inert")).toBe(false);
    expect(host.querySelector("[data-visible='true']")).toBe(terminalBefore);
    expect(document.activeElement).toBe(settingsButton);

    await act(async () => settingsButton.click());
    expect(analystTab.getAttribute("aria-selected")).toBe("true");
    await act(async () => settingsButton.click());
    expect(host.querySelector("#codex-voice-settings-page")?.hasAttribute("hidden")).toBe(true);

    await act(async () => settingsButton.click());
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(host.querySelector("#codex-voice-settings-page")?.hasAttribute("hidden")).toBe(true);
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => root.unmount());
  });
});
