import { createRef } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { CodexVoiceAnalystModal } from "./CodexVoiceAnalystModal";

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
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller()}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain("codexVoiceConversation");
    expect(html).toContain("codexVoiceAnalystTitle");
    expect(html).toContain('aria-label="codexVoiceSettings"');
    expect(html).toContain("codexVoiceStart");
    expect(html).toContain("lucide-audio-lines");
    expect(html).not.toContain("Alpha");
    expect(html).not.toContain("codexVoiceScope");
    expect(html).not.toContain("codexVoiceOpenTerminal");
  });

  it("puts owned call controls in the header and embeds the Analyst terminal", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
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
    expect(html).toContain('data-visible="true"');
    expect(html).not.toContain("External terminal diagnostics");
    expect(html).not.toContain("Alpha");
  });

  it("shows accumulated current-turn captions at readable hierarchy", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={controller({
          userTranscript: "今天天气怎么样",
          assistantTranscript: "我来帮你查一下。",
        })}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain("今天天气怎么样");
    expect(html).toContain("我来帮你查一下。");
    expect(html).toContain("text-[15px]");
  });

  it("shows a concise terminal placeholder before the first investigation", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceAnalystModal
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
