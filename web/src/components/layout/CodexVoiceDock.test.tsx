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
    audioRef: createRef<HTMLAudioElement>(),
    phase: "idle",
    call: null,
    analyst: null,
    owned: false,
    checking: false,
    userTranscript: "",
    assistantTranscript: "",
    microphoneMuted: false,
    playbackBlocked: false,
    error: "",
    isStarting: false,
    isEngaged: false,
    externalCall: false,
    analystWorking: false,
    analystWarning: "",
    preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
    supportedVoices: ["cove"],
    readiness: { codex_cli_available: true, codex_credentials_available: true },
    updatePreferences: vi.fn(),
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
  it("keeps two stable global actions: open console and start voice", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller()}
        selectedGroupId="g_alpha"
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
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
        selectedGroupId="g_alpha"
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
      <CodexVoiceDock
        controller={controller()}
        selectedGroupId="g_alpha"
        collapsed
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="layout:codexVoiceOpenConsole"');
    expect(html).toContain('aria-label="layout:codexVoiceStart"');
  });

  it("reports a warm Analyst without exposing its bound Group", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({
          analyst: {
            group_id: "g_alpha",
            group_title: "Alpha",
            generation: "analyst-1",
            tui_ready: true,
            phase: "ready",
            last_result: "",
            warning: "",
          },
        })}
        selectedGroupId="g_beta"
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );

    expect(html).toContain("layout:codexVoiceAnalystReady");
    expect(html).not.toContain("Alpha");
    expect(html).not.toContain("g_beta");
  });
});
