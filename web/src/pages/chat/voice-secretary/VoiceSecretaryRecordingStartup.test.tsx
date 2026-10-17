// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceSecretaryComposerControl } from "../VoiceSecretaryComposerControl";
import { useComposerStore, useGroupStore, useModalStore, useUIStore } from "../../../stores";
import type { AssistantStateResult } from "../../../types";
import type { BrowserSpeechRecognitionEvent } from "./voiceBrowserSpeechTypes";

const api = vi.hoisted(() => ({
  workspace: vi.fn(),
  status: vi.fn(),
  append: vi.fn(),
  lease: vi.fn(),
  microphone: vi.fn(),
  t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: api.t }) }));
vi.mock("../../../services/api", async (original) => ({
  ...(await original<typeof import("../../../services/api")>()),
  fetchVoiceAssistantWorkspace: api.workspace,
  fetchVoiceAssistantStatus: api.status,
  appendVoiceAssistantInput: api.append,
  updateVoiceAssistantRecordingLease: api.lease,
  fetchLatestVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
  fetchVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
}));

class Recognition {
  static latest: Recognition;
  onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null = null;
  onend: (() => void) | null = null;
  constructor() {
    Recognition.latest = this;
  }
  start() {}
  stop() {
    this.onend?.();
  }
  abort() {}
}
class Socket {
  static OPEN = 1;
  static latest: Socket;
  readyState = 1;
  onmessage: ((event: { data: string }) => void) | null = null;
  constructor() {
    Socket.latest = this;
  }
  send() {}
  close() {
    this.readyState = 3;
  }
}
class AudioContextFixture {
  sampleRate = 48000;
  destination = {};
  createMediaStreamSource() {
    return { connect() {}, disconnect() {} };
  }
  createScriptProcessor() {
    return { connect() {}, disconnect() {}, onaudioprocess: null };
  }
  close() {
    return Promise.resolve();
  }
}

function state(group: string, backend: string): AssistantStateResult {
  return {
    group_id: group,
    assistant: {
      assistant_id: "voice_secretary",
      kind: "voice_secretary",
      enabled: true,
      lifecycle: "ready",
      config: { recognition_backend: backend },
      health: { secretary: { configured: true, ready: true }, service: { ready: true } },
    },
    documents: [],
  };
}

describe("Voice Secretary recording startup ownership", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const render = (group: string) =>
    act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId={group}
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="prompt"
        />,
      ),
    );
  const recordButton = () => host.querySelector<HTMLButtonElement>("button[aria-pressed]")!;
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.useFakeTimers();
    vi.clearAllMocks();
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    vi.stubGlobal("SpeechRecognition", Recognition);
    vi.stubGlobal("AudioContext", AudioContextFixture);
    vi.stubGlobal("WebSocket", Socket);
    const track = {
      readyState: "live",
      stop: vi.fn(),
      addEventListener() {},
      removeEventListener() {},
    };
    api.microphone.mockResolvedValue({ getTracks: () => [track], getAudioTracks: () => [track] });
    vi.stubGlobal("navigator", {
      language: "en-US",
      mediaDevices: {
        getUserMedia: api.microphone,
        enumerateDevices: async () => [],
        addEventListener() {},
        removeEventListener() {},
      },
    });
    useUIStore.setState({ canAccessGlobalSettings: true, isSmallScreen: false, chatSessions: {} });
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: true });
    useGroupStore.setState({ chatByGroup: {} });
    useComposerStore.setState({ activeGroupId: "A", composerText: "Unsent A draft", drafts: {} });
    useModalStore.setState((store) => ({ modals: { ...store.modals, settings: false } }));
    api.append.mockResolvedValue({ ok: true, result: { request_id: "fixture-request" } });
    api.lease.mockResolvedValue({ ok: true, result: { leaseId: "fixture-lease" } });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.clearAllTimers();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it.each([
    ["browser_asr", "lease"],
    ["assistant_service_local_asr", "lease"],
    ["assistant_service_local_asr", "microphone"],
  ])("keeps A's draft when switching to B during %s %s startup", async (backend, gate) => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, backend),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, backend),
    }));
    let finish!: () => void;
    if (gate === "lease") {
      api.lease.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = () => resolve({ ok: true, result: { leaseId: "fixture-lease" } });
          }),
      );
    } else {
      const stream = await api.microphone();
      api.microphone.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = () => resolve(stream);
          }),
      );
    }
    await render("A");
    expect(recordButton().disabled).toBe(false);
    await act(async () => recordButton().click());
    expect(finish).toBeTypeOf("function");
    await act(async () => {
      useComposerStore.getState().switchGroup("A", "B");
      useComposerStore.getState().setComposerText("Private B draft");
    });
    await render("B");
    await act(async () => finish());
    expect(recordButton().getAttribute("aria-pressed")).toBe("true");
    if (backend === "browser_asr") {
      await act(async () =>
        Recognition.latest.onresult?.({
          resultIndex: 0,
          results: {
            length: 1,
            0: { isFinal: true, length: 1, 0: { transcript: "Please check the build" } },
          },
        }),
      );
      await act(async () => recordButton().click());
    } else {
      await act(async () =>
        Socket.latest.onmessage?.({
          data: JSON.stringify({ type: "final", text: "Please check the build" }),
        }),
      );
      await act(async () => recordButton().click());
      await act(async () =>
        Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }),
      );
    }
    expect(api.append).toHaveBeenCalledExactlyOnceWith(
      "A",
      expect.objectContaining({
        kind: "prompt_refine",
        composerText: "Unsent A draft",
        voiceTranscript: "Please check the build",
      }),
    );
    expect(useComposerStore.getState().composerText).toBe("Private B draft");
    expect(useComposerStore.getState().drafts.A?.composerText).toBe("Unsent A draft");
  });
});
