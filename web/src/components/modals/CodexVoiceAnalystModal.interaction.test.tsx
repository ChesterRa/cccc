// @vitest-environment happy-dom

import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { useChetConversation } from "../../features/chet/useChetConversation";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { fetchChetUnattended } from "../../services/api/chet";
import { useModalStore } from "../../stores";
import { CodexVoiceAnalystModal } from "./CodexVoiceAnalystModal";
import { chetConversationStub } from "../../features/chet/chetTestSupport";

vi.mock("../../services/api/chet", async (original) => ({
  ...(await original<typeof import("../../services/api/chet")>()),
  fetchChetUnattended: vi.fn(async () => ({ ok: true, result: { enabled: false, since: null } })),
}));
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
    recoveryOwners: [],
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
          chet={chetConversationStub()}
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
    await clickText("chet.activity");
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
          chet={chetConversationStub()}
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
            chet={chetConversationStub()}
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

  it("remembers hiding only the optional ChatGPT voice login row", async () => {
    const preference = "cccc.chet.voiceLoginReminder.dismissed";
    localStorage.removeItem(preference);
    const host = document.createElement("div");
    document.body.append(host);
    let root = createRoot(host);
    const voice = controller();
    voice.phase = "idle";
    voice.isEngaged = false;
    voice.owned = false;
    voice.analyst = null;
    voice.readiness = {
      ...voice.readiness!,
      analyst_runtime: "opencode",
      analyst_runtime_available: false,
      realtime_credentials_available: false,
    };
    const close = vi.fn();
    const render = (isOpen = true) =>
      act(async () =>
        root.render(
          <CodexVoiceAnalystModal
            isOpen={isOpen}
            isDark={false}
            isSmallScreen={false}
            controller={voice}
            chet={chetConversationStub()}
            onClose={close}
          />,
        ),
      );
    const rows = () => host.querySelector("[data-codex-voice-readiness]")!;
    try {
      await render();
      expect(rows().textContent).toContain("codexVoiceCodexLoginRequired");
      expect(rows().textContent).toContain("codexVoiceAnalystRuntimeMissing");
      const dismiss = rows().querySelector<HTMLButtonElement>(
        '[aria-label="codexVoiceDismissLoginReminder"]',
      );
      expect(dismiss, "the optional login row has its own close control").not.toBeNull();
      await act(async () => dismiss!.click());
      expect(rows().textContent).not.toContain("codexVoiceCodexLoginRequired");
      expect(rows().textContent).toContain("codexVoiceAnalystRuntimeMissing");
      expect(close).not.toHaveBeenCalled();
      await render(false);
      await render();
      expect(rows().textContent).not.toContain("codexVoiceCodexLoginRequired");

      // A new app instance, as after a reload, reads the browser preference again.
      await act(async () => root.unmount());
      root = createRoot(host);
      await render();
      expect(rows().textContent).not.toContain("codexVoiceCodexLoginRequired");
      expect(rows().textContent).toContain("codexVoiceAnalystRuntimeMissing");
      await act(async () => buttonByLabel(host, "codexVoiceSettings").click());
      expect(useModalStore.getState().settingsTarget).toMatchObject({
        scope: "global",
        tab: "voice",
        voiceSection: "realtime",
      });

      voice.error = "codexVoiceErrors.codex_voice_realtime_auth_failed";
      await render();
      expect(host.querySelector('[role="alert"]')?.textContent).toContain(voice.error);
      voice.readiness = { ...voice.readiness!, realtime_provider: "grok" };
      await render();
      expect(rows().textContent).toContain("codexVoiceGrokLoginRequired");
      expect(voice.start).not.toHaveBeenCalled();
      expect(voice.disconnect).not.toHaveBeenCalled();
      expect(voice.clearError).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      host.remove();
      localStorage.removeItem(preference);
    }
  });
});

it("shows a saved unattended copy again after turning the mode off and reopening history", async () => {
  const createdAt = "2026-11-01T10:00:00Z";
  const copyText = "Please check the release notes.";
  const history = {
    records: [
      {
        kind: "group_input",
        id: "copy-1",
        created_at: createdAt,
        input: {
          group_id: "g1",
          group_generation: "generation",
          event_id: "source-1",
          sender: { kind: "actor", actor_id: "peer" },
        },
      },
    ],
    after: "1",
    sources: {
      "copy-1": {
        status: "available",
        group_title: "Release",
        sender_title: "Peer",
        text: copyText,
        sent_at: createdAt,
        has_attachments: false,
        truncated: false,
        reply_event_id: null,
        copy: true,
      },
    },
  };
  vi.mocked(fetchChetUnattended)
    .mockResolvedValueOnce({ ok: true, result: { enabled: true, since: "2026-11-01T09:00:00Z" } })
    .mockResolvedValueOnce({ ok: true, result: { enabled: false, since: null } });
  const reads: boolean[] = [];
  let unattendedEnabled = true;
  const respond = (result: unknown) => new Response(JSON.stringify({ ok: true, result }));
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const path = new URL(String(input), "http://localhost").pathname;
    if (path === "/api/v1/chet/status") {
      return respond({
        generation: null,
        preparing: [],
        queued: [],
        working: [],
        history_write_failed: false,
        start_problem: null,
      });
    }
    if (path === "/api/v1/chet/history") {
      reads.push(unattendedEnabled);
      return respond(history);
    }
    if (path === "/api/v1/chet/unattended" && init?.method === "PUT") {
      expect(init.body).toBe(JSON.stringify({ enabled: false }));
      unattendedEnabled = false;
      return respond({ unattended: { enabled: false } });
    }
    throw new Error(`Unexpected request: ${path}`);
  });

  const voice = controller();
  const close = vi.fn();
  function Panel() {
    const chet = useChetConversation({ enabled: true, open: true, currentContext: null });
    return (
      <CodexVoiceAnalystModal
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={voice}
        chet={chet}
        onClose={close}
      />
    );
  }

  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const openHistory = async () => {
    await act(async () => root.render(<Panel />));
    await act(async () => vi.advanceTimersByTimeAsync(0));
    const quote = host.querySelector('[data-chet-group-message="available"]');
    expect(quote?.textContent).toContain(copyText);
    expect(quote?.textContent).toContain("chet.groupMessageCopy");
    return host.querySelector<HTMLInputElement>('[role="switch"][aria-label="chet.unattended"]')!;
  };

  vi.useFakeTimers();
  try {
    const toggle = await openHistory();
    expect(toggle.checked).toBe(true);
    await act(async () => toggle.click());
    expect(toggle.checked).toBe(false);
    expect(unattendedEnabled).toBe(false);
    // Remount so the second view depends on a fresh history response.
    await act(async () => root.render(null));
    expect((await openHistory()).checked).toBe(false);
    expect(reads).toEqual([true, false]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  }
});
