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
    userTranscript: "",
    assistantTranscript: "",
    microphoneMuted: false,
    playbackBlocked: false,
    error: "",
    isStarting: false,
    isEngaged: true,
    externalCall: false,
    analystWorking: false,
    analystWarning: "",
    preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
    supportedVoices: ["cove"],
    readiness: { codex_cli_available: true, realtime_credentials_available: true },
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

describe("CodexVoiceAnalystModal settings drawer", () => {
  it("overlays an inert console without remounting or disconnecting its terminal", async () => {
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

    const drawer = host.querySelector("[data-codex-voice-settings-drawer='true']");
    const overlay = host.querySelector("[data-codex-voice-settings-overlay='true']");
    const backdrop = overlay?.querySelector("button");
    const consoleSurface = host.querySelector("[data-codex-voice-console='true']");
    expect(drawer).not.toBeNull();
    expect(drawer?.getAttribute("aria-modal")).toBe("true");
    expect(drawer?.className).toContain("w-full");
    expect(drawer?.className).toContain("bg-[var(--color-bg-primary)]");
    expect(drawer?.className).not.toContain("glass-modal");
    expect(drawer?.className).not.toContain("h-full");
    expect(overlay?.className).toContain("absolute");
    expect(overlay?.className).toContain("items-start");
    expect(overlay?.className).not.toContain("justify-end");
    expect(backdrop?.className).not.toContain("backdrop-blur");
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
      (button) => button.textContent?.trim() === "codexVoiceSettingsClose",
    );
    if (!(done instanceof HTMLButtonElement)) throw new Error("done button not found");
    await act(async () => done.click());

    expect(host.querySelector("[data-codex-voice-settings-drawer='true']")).toBeNull();
    expect(consoleSurface?.hasAttribute("inert")).toBe(false);
    expect(host.querySelector("[data-visible='true']")).toBe(terminalBefore);
    expect(document.activeElement).toBe(settingsButton);

    await act(async () => settingsButton.click());
    const reopenedBackdrop = host.querySelector(
      "[data-codex-voice-settings-overlay='true'] > button",
    );
    if (!(reopenedBackdrop instanceof HTMLButtonElement)) {
      throw new Error("settings backdrop not found");
    }
    await act(async () => reopenedBackdrop.click());
    expect(host.querySelector("[data-codex-voice-settings-drawer='true']")).toBeNull();

    await act(async () => settingsButton.click());
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(host.querySelector("[data-codex-voice-settings-drawer='true']")).toBeNull();
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => root.unmount());
  });
});
