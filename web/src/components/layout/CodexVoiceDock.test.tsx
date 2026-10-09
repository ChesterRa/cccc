import { createRef } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { CodexVoiceDock } from "./CodexVoiceDock";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function controller(
  overrides: Partial<CodexVoiceSessionController> = {},
): CodexVoiceSessionController {
  return {
    audioDeviceSnapshot: null,
    audioRef: createRef<HTMLAudioElement>(),
    phase: "idle",
    call: null,
    analyst: null,
    owned: false,
    checking: false,
    conversation: [],
    notificationPaused: false,
    microphoneMuted: false,
    playbackBlocked: false,
    outputStatus: { queued: 0, blocked: null },
    error: "",
    isStarting: false,
    isEngaged: false,
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
    ...overrides,
  };
}

describe("CodexVoiceDock", () => {
  it("does not advertise Ready when Realtime credentials are unavailable", () => {
    const state = controller();
    state.readiness = { ...state.readiness!, realtime_credentials_available: false };
    const html = renderToStaticMarkup(
      <CodexVoiceDock controller={state} onOpen={vi.fn()} onStart={vi.fn()} />,
    );
    expect(html).toContain("modals:codexVoiceCallBlocked");
    expect(html).not.toContain("modals:codexVoicePhase.idle");
  });

  it("shows a queued or running warm investigation instead of Analyst ready", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({
          analyst: {
            generation: "A",
            phase: "ready",
            tui_ready: false,
            last_result: "",
            warning: "",
          },
          analystWorking: true,
        })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );
    expect(html).toContain("modals:codexVoiceAnalystPhase.working");
    expect(html).not.toContain("layout:codexVoiceAnalystReady");
  });

  it("waits for readiness rather than claiming Ready before the first snapshot", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({ readiness: null })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );
    expect(html).toContain("modals:codexVoiceChecking");
    expect(html).not.toContain("modals:codexVoicePhase.idle");
  });
  it("keeps two stable global actions: open console and start voice", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock controller={controller()} onOpen={vi.fn()} onStart={vi.fn()} />,
    );

    expect(html).toContain('aria-label="layout:codexVoiceOpenConsole"');
    expect(html).toContain('aria-label="layout:codexVoiceStart"');
    expect(html).toContain("lucide-audio-lines");
    expect(html).not.toContain("lucide-mic");
    expect(html).not.toContain("g_alpha");
    expect(html).not.toContain("Settings");
  });

  it("changes only the call control to stop while voice is active", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({ phase: "listening", owned: true, isEngaged: true })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="layout:codexVoiceOpenConsole"');
    expect(html).toContain('aria-label="modals:codexVoiceStop"');
    expect(html).not.toContain("modals:codexVoiceMute");
  });

  it("preserves the same two semantics in the collapsed sidebar", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock controller={controller()} collapsed onOpen={vi.fn()} onStart={vi.fn()} />,
    );

    expect(html).toContain('aria-label="layout:codexVoiceOpenConsole"');
    expect(html).toContain('aria-label="layout:codexVoiceStart"');
  });

  it("reports the global warm Analyst without exposing Group state", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({
          analyst: {
            generation: "analyst-1",
            tui_ready: true,
            phase: "ready",
            last_result: "",
            warning: "",
          },
        })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );

    expect(html).toContain("layout:codexVoiceAnalystReady");
    expect(html).not.toContain("Alpha");
    expect(html).not.toContain("g_beta");
  });
});
