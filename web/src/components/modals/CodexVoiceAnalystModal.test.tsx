import { createRef } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { CodexVoiceAnalystModal } from "./CodexVoiceAnalystModal";
import { chetConversationStub } from "../../features/chet/chetTestSupport";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../hooks/useModalA11y", () => ({
  useModalA11y: () => ({ modalRef: createRef<HTMLDivElement>() }),
}));
vi.mock("../../features/codexVoice/VoiceAnalystTerminal", () => ({
  VoiceAnalystTerminal: ({ isVisible }: { isVisible: boolean }) => (
    <div data-visible={String(isVisible)}>embedded-analyst-terminal</div>
  ),
}));

afterEach(() => {
  vi.unstubAllGlobals();
});

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
    recoveryOwners: [],
    refreshError: "",
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

const readyAnalyst = {
  generation: "analyst-1",
  tui_ready: true,
  phase: "ready" as const,
  last_result: "",
  warning: "",
};

describe("CodexVoiceAnalystModal", () => {
  it.each([
    ["preparing", false],
    ["connecting", false],
    ["listening", true],
    ["responding", true],
    ["speaking", true],
    ["analysing", true],
    ["stopping", false],
    ["failed", false],
  ] as const)("marks %s as moving only while a voice operation is active", (phase, moving) => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({ phase, isEngaged: true })}
        onClose={vi.fn()}
      />,
    );
    expect(html.includes('data-status-motion="working"')).toBe(moving);
    const dot = html.match(/<span[^>]*data-status-dot="[^"]+"[^>]*>/)?.[0];
    expect(dot).toBeDefined();
    expect(dot?.includes("animate-pulse")).toBe(
      ["preparing", "connecting", "stopping"].includes(phase),
    );
  });

  it("does not animate muted, external or blocked voice activity", () => {
    const markup = (overrides: Partial<CodexVoiceSessionController>) =>
      renderToStaticMarkup(
        <CodexVoiceAnalystModal
          chet={chetConversationStub()}
          isOpen
          isDark={false}
          isSmallScreen={false}
          controller={controller({ phase: "listening", isEngaged: true, ...overrides })}
          onClose={vi.fn()}
        />,
      );
    expect(markup({})).toContain('data-status-motion="working"');
    expect(markup({ microphoneMuted: true })).not.toContain('data-status-motion="working"');
    expect(markup({ externalCall: true })).not.toContain('data-status-motion="working"');
    expect(markup({ phase: "speaking", playbackBlocked: true })).not.toContain(
      'data-status-motion="working"',
    );
    const readiness = controller().readiness!;
    const blocked = markup({
      phase: "preparing",
      isEngaged: false,
      readiness: { ...readiness, analyst_runtime_available: false },
    });
    const dot = blocked.match(/<span[^>]*data-status-dot="attention"[^>]*>/)?.[0];
    expect(dot).toBeDefined();
    expect(dot).not.toContain("animate-pulse");
  });

  it("shows Chet's head in the title, moving while Chet works as in the dock", () => {
    const markup = (working: boolean, analystWorking: boolean) =>
      renderToStaticMarkup(
        <CodexVoiceAnalystModal
          chet={chetConversationStub({ working })}
          isOpen
          isDark={false}
          isSmallScreen={false}
          controller={controller({ analystWorking })}
          onClose={vi.fn()}
        />,
      );
    expect(markup(false, false)).toContain('data-chet-head="idle"');
    expect(markup(true, false)).toContain('data-chet-head="working"');
    expect(markup(false, true)).toContain('data-chet-head="working"');
  });

  it.each([
    { provider: "grok" as const, voice: "eve", name: "Eve" },
    { provider: "codex" as const, voice: "himari", name: "Himari" },
  ])("shows the selected $provider voice before starting a call", ({ provider, voice, name }) => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          preferences: { provider, voice, inputDeviceId: "", outputDeviceId: "" },
        })}
        onClose={vi.fn()}
      />,
    );
    expect(html).toContain(`codexVoiceProviders.${provider}`);
    expect(html).toMatch(new RegExp(`<span[^>]*>${name}</span>`));
    expect(html).toContain('aria-label="codexVoiceProviderLabel"');
    expect(html).toContain('role="combobox"');
    expect(html).toContain('aria-label="codexVoiceVoiceLabel"');
    expect(html).toContain("codexVoicePhase.idle");
  });

  it("identifies the live provider and voice instead of the next-call preferences", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          phase: "listening",
          isEngaged: true,
          owned: true,
          call: {
            provider: "grok",
            generation: "call-1",
            analyst_generation: null,
            mode: "assistant",
            voice: "aurora",
            connected: true,
          },
          preferences: { provider: "codex", voice: "cove", inputDeviceId: "", outputDeviceId: "" },
        })}
        onClose={vi.fn()}
      />,
    );
    expect(html.match(/<h2[^>]*>(.*?)<\/h2>/)?.[1]).toBe("Chet");
    // The call line beside the composer names the live call, not the next one.
    expect(html).toContain("codexVoiceProviders.grok · Aurora · codexVoicePhase.listening");
    expect(html).not.toContain("Cove");
    expect(html).not.toContain('aria-label="codexVoiceProviderLabel"');
    expect(html).not.toContain('aria-label="codexVoiceVoiceLabel"');
    expect(html).toContain('aria-label="codexVoiceMute"');
    expect(html).toContain("codexVoiceStop</button>");
  });

  it("shows ACP setup guidance for first use instead of a missing native runtime", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          readiness: {
            supported_modes: ["assistant", "persona"],
            analyst_runtime: "antigravity",
            analyst_runtime_available: false,
            analyst_runtime_setup_required: true,
            realtime_credentials_available: true,
          },
        })}
        onClose={vi.fn()}
      />,
    );
    expect(html).toContain("codexVoiceAntigravityAcpSetupRequired");
    expect(html).not.toContain("codexVoiceAnalystRuntimeMissing");
  });

  it("shows the setup instruction once after a blocked start", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen
        controller={controller({
          phase: "failed",
          error: "codexVoiceAntigravityAcpSetupRequired",
          readiness: {
            supported_modes: ["assistant", "persona"],
            analyst_runtime: "antigravity",
            analyst_runtime_available: false,
            analyst_runtime_setup_required: true,
            realtime_credentials_available: true,
          },
        })}
        onClose={vi.fn()}
      />,
    );
    expect(html.match(/codexVoiceAntigravityAcpSetupRequired/g)).toHaveLength(1);
    expect(html).toContain('role="alert"');
  });

  it("does not connect the hidden terminal below the lg breakpoint", () => {
    vi.stubGlobal("window", {
      matchMedia: vi.fn(() => ({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({ analyst: readyAnalyst })}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain("embedded-analyst-terminal");
    expect(html).toContain('data-visible="false"');
  });

  it("presents one global console without a Focus Group surface", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller()}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain("chet.conversation");
    expect(html).toContain("chet.activity");
    expect(html).toContain('aria-label="codexVoiceSettings"');
    expect(html).toContain("codexVoiceStart");
    expect(html).toContain("lucide-audio-lines");
    expect(html).not.toContain("Alpha");
    expect(html).not.toContain("codexVoiceScope");
    expect(html).not.toContain("codexVoiceOpenTerminal");
  });

  it("notes a voice-only problem beside the voice controls without a banner over text", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          readiness: {
            supported_modes: ["assistant", "persona"],
            analyst_runtime: "claude",
            analyst_runtime_available: true,
            realtime_credentials_available: false,
          },
        })}
        onClose={vi.fn()}
      />,
    );
    expect(html).not.toContain("codexVoicePhase.idle");
    expect(html).toMatch(/aria-live="polite"[^>]*>codexVoiceCallBlocked</);
    // Text needs no Realtime login, so the call's setup stays with the voice controls.
    expect(html).not.toContain("data-codex-voice-readiness");
    expect(html).toMatch(
      /<button[^>]*title="codexVoiceCodexLoginRequired"[^>]*>.*?codexVoiceCallBlocked/,
    );
    expect(html).not.toContain("codexVoiceAnalystNotReady");
  });

  it("names call and Analyst readiness separately when the Analyst is not ready", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          readiness: {
            supported_modes: ["assistant", "persona"],
            analyst_runtime: "claude",
            analyst_runtime_available: false,
            realtime_credentials_available: false,
          },
        })}
        onClose={vi.fn()}
      />,
    );
    expect(html).toContain("data-codex-voice-readiness");
    expect(html).toContain("codexVoiceCodexLoginRequired");
    expect(html).toContain("codexVoiceAnalystRuntimeMissing");
    expect(html.match(/codexVoiceOpenSettings/g)).toHaveLength(1);
  });

  it("keeps the embedded Analyst terminal collapsed and disconnected by default", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark
        isSmallScreen={false}
        controller={controller({
          phase: "listening",
          owned: true,
          isEngaged: true,
          analyst: readyAnalyst,
        })}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="codexVoiceMute"');
    expect(html).toContain("codexVoiceStop");
    expect(html).toContain("embedded-analyst-terminal");
    expect(html).toContain('data-visible="false"');
    expect(html).toContain("data-codex-voice-analyst-bar");
    expect(html).not.toContain("codexVoiceHideAnalyst");
    expect(html).not.toContain("External terminal diagnostics");
    expect(html).not.toContain("Alpha");
  });

  it("shows accumulated current-turn captions", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          conversation: [
            { id: "user", role: "user", text: "今天天气怎么样", final: true },
            { id: "assistant", role: "assistant", text: "我来帮你查一下。", final: true },
          ],
        })}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain("今天天气怎么样");
    expect(html).toContain("我来帮你查一下。");
  });

  it("shows a concise terminal placeholder before the first investigation", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          analyst: { ...readyAnalyst, tui_ready: false, phase: "waiting" },
        })}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain("codexVoiceAnalystTerminalPending");
    expect(html).not.toContain("embedded-analyst-terminal");
  });

  it("keeps a call failure visible until the user dismisses it", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        chet={chetConversationStub()}
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({ phase: "failed", error: "CONTROL_STREAM_LOST" })}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain("CONTROL_STREAM_LOST");
    expect(html).toContain("codexVoiceDismissError");
  });
});
