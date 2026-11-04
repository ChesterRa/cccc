// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceSecretaryComposerControl } from "../VoiceSecretaryComposerControl";
import { useComposerStore, useGroupStore, useModalStore, useUIStore } from "../../../stores";
import { useVoiceAudioStore } from "../../../stores/useVoiceAudioStore";
import { usePendingPageWork } from "../../../stores/usePendingPageWork";
import type { AssistantStateResult } from "../../../types";
import type { BrowserSpeechRecognitionEvent } from "./voiceBrowserSpeechTypes";
import {
  beginFrameAccess,
  endFrameAccess,
  pauseFrameAccess,
} from "../../../features/connect/frameAccess";

const api = vi.hoisted(() => ({
  workspace: vi.fn(),
  status: vi.fn(),
  append: vi.fn(),
  segment: vi.fn(),
  cancel: vi.fn(),
  lease: vi.fn(),
  microphone: vi.fn(),
  showError: vi.fn(),
  showNotice: vi.fn(),
  t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: api.t }) }));
vi.mock("../../../services/api", async (original) => ({
  ...(await original<typeof import("../../../services/api")>()),
  fetchVoiceAssistantWorkspace: api.workspace,
  fetchVoiceAssistantStatus: api.status,
  appendVoiceAssistantInput: api.append,
  appendVoiceAssistantTranscriptSegment: api.segment,
  cancelSecretaryTask: api.cancel,
  updateVoiceAssistantRecordingLease: api.lease,
  fetchLatestVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
  fetchVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
}));

class Recognition {
  static latest: Recognition;
  static autoEndOnStop = true;
  onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null = null;
  onend: (() => void) | null = null;
  onerror: ((event: { error: string }) => void) | null = null;
  constructor() {
    Recognition.latest = this;
  }
  start() {}
  stop() {
    if (Recognition.autoEndOnStop) this.onend?.();
  }
  abort() {}
}
class Socket {
  static CONNECTING = 0;
  static OPEN = 1;
  static latest: Socket;
  static initialReadyState = 1;
  readyState = Socket.initialReadyState;
  bufferedAmount = 0;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: ((event: { code: number; reason: string }) => void) | null = null;
  constructor() {
    Socket.latest = this;
    if (this.readyState === Socket.OPEN) {
      void Promise.resolve().then(() => this.onopen?.());
    }
  }
  send = vi.fn<(data: string | Uint8Array) => void>();
  close() {
    this.readyState = 3;
  }
}
class AudioContextFixture {
  static latest: AudioContextFixture;
  static modulePromise: Promise<void> | null = null;
  static resumePromise: Promise<void> | null = null;
  static initialState = "running";
  static closeSpy = vi.fn();
  sampleRate = 48000;
  destination = {};
  state = AudioContextFixture.initialState;
  onstatechange: (() => void) | null = null;
  constructor() {
    AudioContextFixture.latest = this;
  }
  get audioWorklet() {
    return { addModule: () => AudioContextFixture.modulePromise || Promise.resolve() };
  }
  createMediaStreamSource() {
    return { connect() {}, disconnect() {} };
  }
  resume() {
    return AudioContextFixture.resumePromise || Promise.resolve();
  }
  close() {
    AudioContextFixture.closeSpy();
    return Promise.resolve();
  }
}
class WorkletNodeFixture {
  static latest: WorkletNodeFixture;
  static autoFlush = true;
  onprocessorerror: (() => void) | null = null;
  port = {
    onmessage: null as ((event: { data: unknown }) => void) | null,
    postMessage: vi.fn((message: { type: string }) => {
      if (message.type === "flush" && WorkletNodeFixture.autoFlush) {
        void Promise.resolve().then(() => this.port.onmessage?.({ data: { type: "flushed" } }));
      }
    }),
    close() {},
  };
  constructor() {
    WorkletNodeFixture.latest = this;
  }
  connect() {}
  disconnect() {}
  emitSamples(samples: Float32Array) {
    this.port.onmessage?.({ data: { type: "samples", buffer: samples.buffer } });
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
  let rootMounted: boolean;
  let trackStop: ReturnType<typeof vi.fn>;
  let capturedTrack: { stop: ReturnType<typeof vi.fn>; onended?: ((event: Event) => void) | null };
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
    usePendingPageWork.setState({ owners: new Set() });
    Recognition.autoEndOnStop = true;
    Socket.initialReadyState = 1;
    WorkletNodeFixture.autoFlush = true;
    AudioContextFixture.modulePromise = null;
    AudioContextFixture.resumePromise = null;
    AudioContextFixture.initialState = "running";
    window.localStorage.clear();
    window.sessionStorage.clear();
    useVoiceAudioStore.setState({ preferences: { inputDeviceId: "", outputDeviceId: "" } });
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    vi.stubGlobal("SpeechRecognition", Recognition);
    vi.stubGlobal("AudioContext", AudioContextFixture);
    vi.stubGlobal("AudioWorkletNode", WorkletNodeFixture);
    vi.stubGlobal("WebSocket", Socket);
    trackStop = vi.fn();
    const track = {
      readyState: "live",
      stop: trackStop,
      addEventListener() {},
      removeEventListener() {},
    };
    capturedTrack = track;
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
    useUIStore.setState({
      canAccessGlobalSettings: true,
      isSmallScreen: false,
      chatSessions: {},
      showError: api.showError,
      showNotice: api.showNotice,
    });
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: true });
    useGroupStore.setState({ chatByGroup: {} });
    useComposerStore.setState({ activeGroupId: "A", composerText: "Unsent A draft", drafts: {} });
    useModalStore.setState((store) => ({ modals: { ...store.modals, settings: false } }));
    api.append.mockResolvedValue({ ok: true, result: { request_id: "fixture-request" } });
    api.segment.mockResolvedValue({ ok: true, result: {} });
    api.cancel.mockResolvedValue({ ok: true, result: {} });
    api.lease.mockResolvedValue({ ok: true, result: { leaseId: "fixture-lease" } });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    rootMounted = true;
  });
  afterEach(async () => {
    if (rootMounted) await act(async () => root.unmount());
    endFrameAccess();
    expect(usePendingPageWork.getState().owners.size).toBe(0);
    host.remove();
    vi.clearAllTimers();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("warns for absent or all-zero service audio and clears the hint on even quiet input", async () => {
    api.workspace.mockResolvedValue({
      ok: true,
      result: state("A", "assistant_service_local_asr"),
    });
    api.status.mockResolvedValue({ ok: true, result: state("A", "assistant_service_local_asr") });
    await render("A");
    await act(async () => recordButton().click());
    expect(usePendingPageWork.getState().owners.size).toBe(1);
    await act(async () => vi.advanceTimersByTimeAsync(5_999));
    expect(host.querySelector("[data-voice-input-silence]")).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(host.querySelector("[data-voice-input-silence]")).not.toBeNull();
    await act(async () => WorkletNodeFixture.latest.emitSamples(new Float32Array(960)));
    expect(host.querySelector("[data-voice-input-silence]")).not.toBeNull();
    await act(async () => WorkletNodeFixture.latest.emitSamples(new Float32Array([0.000001])));
    expect(host.querySelector("[data-voice-input-silence]")).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(6_000));
    expect(host.querySelector("[data-voice-input-silence]")).not.toBeNull();
    await act(async () => recordButton().click());
    expect(host.querySelector("[data-voice-input-silence]")).toBeNull();
    expect(usePendingPageWork.getState().owners.size).toBe(1);
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(usePendingPageWork.getState().owners.size).toBe(0);
  });

  it("does not claim to inspect browser speech recognition audio", async () => {
    api.workspace.mockResolvedValue({ ok: true, result: state("A", "browser_asr") });
    api.status.mockResolvedValue({ ok: true, result: state("A", "browser_asr") });
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => vi.advanceTimersByTimeAsync(7_000));
    expect(host.querySelector("[data-voice-input-silence]")).toBeNull();
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

  it.each(["browser_asr", "assistant_service_local_asr"])(
    "releases a %s lease that is acquired after the recording component unmounts",
    async (backend) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      let finish!: () => void;
      api.lease.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = () => resolve({ ok: true, result: { leaseId: "late-lease" } });
          }),
      );
      await render("A");
      await act(async () => recordButton().click());
      expect(finish).toBeTypeOf("function");
      await act(async () => root.unmount());
      rootMounted = false;
      await act(async () => finish());
      expect(api.lease).toHaveBeenCalledWith("A", {
        action: "release",
        ownerId: expect.any(String),
        leaseId: "late-lease",
      });
      expect(api.microphone).not.toHaveBeenCalled();
      expect(window.localStorage.getItem("cccc.voiceSecretary.activeCapture")).toBeNull();
    },
  );

  it.each(["browser_asr", "assistant_service_local_asr"])(
    "clears a %s advisory lock claimed after the recording component unmounts",
    async (backend) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      vi.stubGlobal(
        "BroadcastChannel",
        class {
          addEventListener() {}
          removeEventListener() {}
          postMessage() {}
          close() {}
        },
      );
      window.localStorage.setItem(
        "cccc.voiceSecretary.activeCapture",
        JSON.stringify({ ownerId: "old-tab", groupId: "B", updatedAt: Date.now() }),
      );
      await render("A");
      await act(async () => recordButton().click());
      await act(async () => root.unmount());
      rootMounted = false;
      await act(async () => vi.advanceTimersByTimeAsync(300));
      expect(window.localStorage.getItem("cccc.voiceSecretary.activeCapture")).toBeNull();
      expect(api.lease).not.toHaveBeenCalled();
      expect(api.microphone).not.toHaveBeenCalled();
    },
  );

  it.each([
    ["final", "嗯"],
    ["final_asr_text", "I."],
  ])("puts a short service %s answer in the composer without sending it", async (type, text) => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: false });
    const onPromptDraft = vi.fn();
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="prompt"
          onPromptDraft={onPromptDraft}
        />,
      ),
    );
    await act(async () => recordButton().click());
    expect(recordButton().getAttribute("aria-pressed")).toBe("true");
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type, text }) }));
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(onPromptDraft).toHaveBeenCalledExactlyOnceWith(text, { mode: "append" });
    expect(api.append).not.toHaveBeenCalled();
  });

  it("keeps a stopped browser recording in its original Group until its final result arrives", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: false });
    Recognition.autoEndOnStop = false;
    await render("A");
    await act(async () => recordButton().click());
    const recognition = Recognition.latest;
    await act(async () => recordButton().click());
    expect(recordButton().disabled).toBe(true);
    expect(host.querySelector('[data-voice-composer-activity="finishing"]')).not.toBeNull();
    await act(async () => {
      useComposerStore.getState().switchGroup("A", "B");
      useComposerStore.getState().setComposerText("Private B draft");
    });
    await render("B");
    await act(async () => {
      recognition.onresult?.({
        resultIndex: 0,
        results: { length: 1, 0: { isFinal: true, length: 1, 0: { transcript: "好" } } },
      });
      recognition.onend?.();
    });
    expect(useComposerStore.getState().composerText).toBe("Private B draft");
    expect(useComposerStore.getState().drafts.A?.composerText).toBe("Unsent A draft 好");
    expect(api.showNotice).toHaveBeenCalledWith({
      message: "Speech transcript appended to the composer in {{group}}.",
    });
    expect(host.querySelector('[data-voice-composer-activity="finishing"]')).toBeNull();
    expect(recordButton().disabled).toBe(false);
    expect(api.append).not.toHaveBeenCalled();
  });

  it("normally sends completed speech when stopped between browser recognition cycles", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: true, length: 1, 0: { transcript: "check the configuration" } },
        },
      });
      Recognition.latest.onend?.();
    });
    await act(async () => recordButton().click());
    expect(api.append).toHaveBeenCalledExactlyOnceWith(
      "A",
      expect.objectContaining({
        kind: "prompt_refine",
        voiceTranscript: "check the configuration",
      }),
    );
    expect(api.showError).not.toHaveBeenCalled();
  });

  it.each(["browser_asr", "assistant_service_local_asr"])(
    "submits a new %s Ask independently of an earlier pending Ask",
    async (backend) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      window.sessionStorage.setItem(
        "cccc_voice_ask_notice:A",
        JSON.stringify({ requestId: "earlier-ask", hidden: false }),
      );
      await act(async () =>
        root.render(
          <VoiceSecretaryComposerControl
            selectedGroupId="A"
            isDark={false}
            busy=""
            variant="assistantRow"
            captureMode="instruction"
          />,
        ),
      );
      await act(async () => recordButton().click());
      await act(async () => {
        if (backend === "browser_asr") {
          Recognition.latest.onresult?.({
            resultIndex: 0,
            results: {
              length: 1,
              0: { isFinal: true, length: 1, 0: { transcript: "check the configuration" } },
            },
          });
        } else {
          const final = {
            type: "final",
            segment_id: "same-event",
            text: "check the configuration",
          };
          Socket.latest.onmessage?.({ data: JSON.stringify(final) });
          Socket.latest.onmessage?.({ data: JSON.stringify(final) });
        }
      });
      await act(async () => recordButton().click());
      if (backend !== "browser_asr") {
        await act(async () =>
          Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }),
        );
      }
      expect(api.append).toHaveBeenCalledTimes(1);
      const [group, input] = api.append.mock.calls[0];
      expect(group).toBe("A");
      expect(input).toMatchObject({
        kind: "voice_instruction",
        instruction: "check the configuration",
      });
      expect(input.requestId).toBeTruthy();
      expect(input.requestId).not.toBe("earlier-ask");
    },
  );

  it("keeps repeated browser words from distinct results without replaying one result", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: false });
    const onPromptDraft = vi.fn();
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="prompt"
          onPromptDraft={onPromptDraft}
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: { length: 1, 0: { isFinal: true, length: 1, 0: { transcript: "yes" } } },
      });
      const second = {
        resultIndex: 1,
        results: { length: 2, 1: { isFinal: true, length: 1, 0: { transcript: "yes" } } },
      } as BrowserSpeechRecognitionEvent;
      Recognition.latest.onresult?.(second);
      Recognition.latest.onresult?.(second);
    });
    await act(async () => recordButton().click());
    expect(onPromptDraft).toHaveBeenCalledExactlyOnceWith("yes yes", { mode: "append" });
    expect(api.append).not.toHaveBeenCalled();
  });

  it("preserves a browser transcript after a fatal error without sending it or mixing it into the next run", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: true, length: 1, 0: { transcript: "first recording" } },
        },
      });
      Recognition.latest.onerror?.({ error: "not-allowed" });
    });
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft first recording");
    expect(api.append).not.toHaveBeenCalled();
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: true, length: 1, 0: { transcript: "second recording" } },
        },
      });
    });
    await act(async () => recordButton().click());
    expect(api.append).toHaveBeenCalledExactlyOnceWith(
      "A",
      expect.objectContaining({ kind: "prompt_refine", voiceTranscript: "second recording" }),
    );
  });

  it.each(["browser_asr", "assistant_service_local_asr"])(
    "preserves %s recognized text in the original Group when the control unmounts",
    async (backend) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      await render("A");
      await act(async () => recordButton().click());
      await act(async () => {
        if (backend === "browser_asr") {
          Recognition.latest.onresult?.({
            resultIndex: 0,
            results: {
              length: 2,
              0: { isFinal: true, length: 1, 0: { transcript: "first" } },
              1: { isFinal: false, length: 1, 0: { transcript: "second" } },
            },
          });
        } else {
          Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "first" }) });
          Socket.latest.onmessage?.({ data: JSON.stringify({ type: "partial", text: "second" }) });
        }
        useComposerStore.getState().switchGroup("A", "B");
        useComposerStore.getState().setComposerText("Private B draft");
      });
      await render("B");
      await act(async () => root.unmount());
      rootMounted = false;

      expect(useComposerStore.getState().drafts.A?.composerText).toBe(
        "Unsent A draft first second",
      );
      expect(useComposerStore.getState().composerText).toBe("Private B draft");
      expect(api.append).not.toHaveBeenCalled();
    },
  );

  it.each(["browser_asr", "assistant_service_local_asr"])(
    "stops %s on Connect suspension and keeps recognized speech in its original Group",
    async (backend) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      beginFrameAccess(new Date(Date.now() + 120000).toISOString());
      await render("A");
      await act(async () => recordButton().click());
      await act(async () => {
        if (backend === "browser_asr") {
          Recognition.latest.onresult?.({
            resultIndex: 0,
            results: {
              length: 1,
              0: { isFinal: true, length: 1, 0: { transcript: "keep these words" } },
            },
          });
        } else {
          Socket.latest.onmessage?.({
            data: JSON.stringify({ type: "final", text: "keep these words" }),
          });
        }
        useComposerStore.getState().switchGroup("A", "B");
        useComposerStore.getState().setComposerText("Private B draft");
      });
      await render("B");
      await act(async () => pauseFrameAccess());
      expect(trackStop).toHaveBeenCalled();
      expect(useComposerStore.getState().drafts.A?.composerText).toContain("keep these words");
      expect(useComposerStore.getState().composerText).toBe("Private B draft");
      expect(api.append).not.toHaveBeenCalled();
      if (backend !== "browser_asr") expect(Socket.latest.readyState).toBe(3);
    },
  );

  it.each(["browser_asr", "assistant_service_local_asr"])(
    "keeps %s speech once when its refinement submission is still pending at unmount",
    async (backend) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, backend),
      }));
      let finishRequest!: (value: unknown) => void;
      api.append.mockImplementationOnce(() => new Promise((resolve) => (finishRequest = resolve)));
      await render("A");
      await act(async () => recordButton().click());
      await act(async () => {
        if (backend === "browser_asr") {
          Recognition.latest.onresult?.({
            resultIndex: 0,
            results: {
              length: 1,
              0: { isFinal: true, length: 1, 0: { transcript: "spoken request" } },
            },
          });
        } else {
          Socket.latest.onmessage?.({
            data: JSON.stringify({ type: "final", text: "spoken request" }),
          });
        }
      });
      await act(async () => recordButton().click());
      if (backend !== "browser_asr") {
        await act(async () =>
          Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }),
        );
      }
      expect(api.append).toHaveBeenCalledTimes(1);
      await act(async () => {
        useComposerStore.getState().switchGroup("A", "B");
        useComposerStore.getState().setComposerText("Private B draft");
      });
      await render("B");
      await act(async () => root.unmount());
      rootMounted = false;
      expect(useComposerStore.getState().drafts.A?.composerText).toBe(
        "Unsent A draft spoken request",
      );
      await act(async () => finishRequest({ ok: false, error: { message: "network" } }));
      expect(useComposerStore.getState().drafts.A?.composerText).toBe(
        "Unsent A draft spoken request",
      );
      expect(useComposerStore.getState().composerText).toBe("Private B draft");
    },
  );

  it("keeps the original speech when an accepted refinement is still awaiting a result at unmount", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: true, length: 1, 0: { transcript: "spoken request" } },
        },
      });
    });
    await act(async () => recordButton().click());
    expect(api.append).toHaveBeenCalledTimes(1);
    expect(recordButton().disabled).toBe(false);
    await act(async () => root.unmount());
    rootMounted = false;
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft spoken request");
  });

  it("keeps browser Ask speech if submission is unconfirmed when the control unmounts", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.append.mockImplementationOnce(() => new Promise(() => {}));
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="instruction"
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: true, length: 1, 0: { transcript: "check the configuration" } },
        },
      });
    });
    await act(async () => recordButton().click());
    expect(api.append).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    rootMounted = false;
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft check the configuration");
  });

  it("preserves two separate recordings even when their words match at unmount", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    await render("A");
    for (let run = 0; run < 2; run++) {
      await act(async () => recordButton().click());
      await act(async () => {
        Recognition.latest.onresult?.({
          resultIndex: 0,
          results: { length: 1, 0: { isFinal: true, length: 1, 0: { transcript: "same words" } } },
        });
      });
      if (run === 0) await act(async () => recordButton().click());
    }
    expect(api.append).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    rootMounted = false;
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft same words same words");
  });

  it("keeps browser dictation punctuation at unmount as it does on a normal stop", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: false });
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: { length: 1, 0: { isFinal: true, length: 1, 0: { transcript: "." } } },
      });
    });
    await act(async () => root.unmount());
    rootMounted = false;
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft.");
    expect(api.append).not.toHaveBeenCalled();
  });

  it("keeps unsent browser document speech when the control unmounts", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="document"
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: true, length: 1, 0: { transcript: "document speech" } },
        },
      });
    });
    expect(api.append).not.toHaveBeenCalled();
    expect(api.segment).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    rootMounted = false;
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft document speech");
    expect(api.append).not.toHaveBeenCalled();
  });

  it.each(["failed", "pending"])(
    "keeps browser document speech when its segment request is %s",
    async (outcome) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, "browser_asr"),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, "browser_asr"),
      }));
      let finishSegment!: (value: unknown) => void;
      if (outcome === "pending") {
        api.segment.mockImplementationOnce(
          () => new Promise((resolve) => (finishSegment = resolve)),
        );
      } else {
        api.segment.mockResolvedValueOnce({ ok: false, error: { message: "network" } });
      }
      await act(async () =>
        root.render(
          <VoiceSecretaryComposerControl
            selectedGroupId="A"
            isDark={false}
            busy=""
            variant="assistantRow"
            captureMode="document"
          />,
        ),
      );
      await act(async () => recordButton().click());
      await act(async () => {
        Recognition.latest.onresult?.({
          resultIndex: 0,
          results: {
            length: 1,
            0: { isFinal: true, length: 1, 0: { transcript: "document speech" } },
          },
        });
      });
      await act(async () => recordButton().click());
      expect(api.segment).toHaveBeenCalledTimes(1);
      if (outcome === "pending") {
        await act(async () => root.unmount());
        rootMounted = false;
        expect(useComposerStore.getState().composerText).toBe("Unsent A draft document speech");
        const errorsBeforeLateResult = api.showError.mock.calls.length;
        await act(async () => finishSegment({ ok: false, error: { message: "network" } }));
        expect(api.showError).toHaveBeenCalledTimes(errorsBeforeLateResult);
      }
      expect(useComposerStore.getState().composerText).toBe("Unsent A draft document speech");
      expect(api.append).not.toHaveBeenCalled();
    },
  );

  it("appends only new document speech at successive local ASR checkpoints", async () => {
    const documentState = (group: string) => {
      const result = state(group, "assistant_service_local_asr");
      result.assistant!.config = {
        ...result.assistant!.config,
        auto_document_max_window_seconds: 10,
      };
      return result;
    };
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: documentState(group),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: documentState(group),
    }));
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="document"
        />,
      ),
    );
    await act(async () => recordButton().click());
    for (const text of ["we will also", "so we can start", "now"]) {
      await act(async () =>
        Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text }) }),
      );
      await act(async () => vi.advanceTimersByTimeAsync(10_000));
    }
    expect(api.segment.mock.calls.map(([, segment]) => segment.text)).toEqual([
      "we will also",
      "so we can start",
      "now",
    ]);
  });

  it.each([true, false])(
    "serializes document checkpoints and continues without replay after a failure (first succeeds: %s)",
    async (firstSucceeds) => {
      const documentState = (group: string) => {
        const result = state(group, "assistant_service_local_asr");
        result.assistant!.config = {
          ...result.assistant!.config,
          auto_document_max_window_seconds: 10,
        };
        return result;
      };
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: documentState(group),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: documentState(group),
      }));
      let finishFirst!: (value: unknown) => void;
      api.segment.mockImplementationOnce(() => new Promise((resolve) => (finishFirst = resolve)));
      await act(async () =>
        root.render(
          <VoiceSecretaryComposerControl
            selectedGroupId="A"
            isDark={false}
            busy=""
            variant="assistantRow"
            captureMode="document"
          />,
        ),
      );
      await act(async () => recordButton().click());
      await act(async () =>
        Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "first" }) }),
      );
      await act(async () => vi.advanceTimersByTimeAsync(10_000));
      expect(api.segment.mock.calls.map(([, segment]) => segment.text)).toEqual(["first"]);
      await act(async () =>
        Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "second" }) }),
      );
      await act(async () => vi.advanceTimersByTimeAsync(10_000));
      expect(api.segment.mock.calls.map(([, segment]) => segment.text)).toEqual(["first"]);
      await act(async () =>
        finishFirst(
          firstSucceeds ? { ok: true, result: {} } : { ok: false, error: { message: "network" } },
        ),
      );
      expect(api.segment.mock.calls.map(([, segment]) => segment.text)).toEqual([
        "first",
        "second",
      ]);
      if (!firstSucceeds) {
        expect(useComposerStore.getState().composerText).toBe("Unsent A draft first");
        await act(async () =>
          Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "third" }) }),
        );
        await act(async () => vi.advanceTimersByTimeAsync(10_000));
        expect(api.segment.mock.calls.map(([, segment]) => segment.text)).toEqual([
          "first",
          "second",
          "third",
        ]);
        expect(useComposerStore.getState().composerText).toBe("Unsent A draft first");
      }
    },
  );

  it("preserves an unconfirmed service document checkpoint when leaving the view", async () => {
    const documentState = (group: string) => {
      const result = state(group, "assistant_service_local_asr");
      result.assistant!.config = {
        ...result.assistant!.config,
        auto_document_max_window_seconds: 10,
      };
      return result;
    };
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: documentState(group),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: documentState(group),
    }));
    let finishCheckpoint!: (value: unknown) => void;
    api.segment.mockImplementationOnce(
      () => new Promise((resolve) => (finishCheckpoint = resolve)),
    );
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="document"
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () =>
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "first" }) }),
    );
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(api.segment).toHaveBeenCalledTimes(1);
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "second" }) });
      useComposerStore.getState().switchGroup("A", "B");
      useComposerStore.getState().setComposerText("Private B draft");
    });
    await render("B");
    await act(async () => root.unmount());
    rootMounted = false;
    expect(useComposerStore.getState().drafts.A?.composerText).toBe("Unsent A draft first second");
    expect(useComposerStore.getState().composerText).toBe("Private B draft");
    await act(async () => finishCheckpoint({ ok: true, result: {} }));
    expect(useComposerStore.getState().drafts.A?.composerText).toBe("Unsent A draft first second");
  });

  it("keeps an unconfirmed document checkpoint when Connect pauses without unmounting", async () => {
    const documentState = (group: string) => {
      const result = state(group, "assistant_service_local_asr");
      result.assistant!.config = {
        ...result.assistant!.config,
        auto_document_max_window_seconds: 10,
      };
      return result;
    };
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: documentState(group),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: documentState(group),
    }));
    let finish!: (value: unknown) => void;
    api.segment.mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
    beginFrameAccess(new Date(Date.now() + 120000).toISOString());
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="document"
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () =>
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "pending words" }) }),
    );
    await act(async () => vi.advanceTimersByTimeAsync(10000));
    expect(api.segment).toHaveBeenCalledTimes(1);
    await act(async () => pauseFrameAccess());
    expect(useComposerStore.getState().composerText).toContain("pending words");
    const recovered = useComposerStore.getState().composerText;
    await act(async () => finish({ ok: true, result: {} }));
    expect(useComposerStore.getState().composerText).toBe(recovered);
  });

  it("keeps distinct service segments even when they contain the same words", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: false });
    const onPromptDraft = vi.fn();
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="prompt"
          onPromptDraft={onPromptDraft}
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () => {
      for (const segment_id of ["one", "one", "two"]) {
        Socket.latest.onmessage?.({
          data: JSON.stringify({ type: "final", segment_id, text: "是" }),
        });
      }
    });
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(onPromptDraft).toHaveBeenCalledExactlyOnceWith("是是", { mode: "append" });
    expect(api.append).not.toHaveBeenCalled();
  });

  it("preserves service text after a connection error without sending it to an agent", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "first" }) });
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "partial", text: "second" }) });
      Socket.latest.onerror?.();
    });
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft first second");
    expect(api.append).not.toHaveBeenCalled();
    expect(api.showError).toHaveBeenCalled();
  });

  it("keeps live text for review when the final service transcription fails", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "live text" }) });
      Socket.latest.onmessage?.({
        data: JSON.stringify({
          type: "final_asr_text",
          ok: false,
          error: { code: "asr_busy", message: "Final recognition is busy" },
        }),
      });
    });
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(api.showError).toHaveBeenCalledWith("Final recognition is busy");
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft live text");
    expect(api.append).not.toHaveBeenCalled();
  });

  it("keeps an unfinalized service hypothesis for review when the stream closes", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({
        data: JSON.stringify({ type: "partial", text: "unfinished idea" }),
      });
    });
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft unfinished idea");
    expect(api.append).not.toHaveBeenCalled();
    expect(api.showNotice).not.toHaveBeenCalledWith({ message: "No text was recognized." });
  });

  it("treats a clean empty final transcription as no recognized text, not an error", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({
        data: JSON.stringify({
          type: "final_asr_text",
          ok: false,
          error: { code: "empty_transcript", message: "final ASR returned no transcript" },
        }),
      });
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) });
    });
    expect(api.showError).not.toHaveBeenCalled();
    expect(api.showNotice).toHaveBeenCalledWith({ message: "No text was recognized." });
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft");
  });

  it("keeps a short service transcript for review instead of silently dropping or sending it", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "yes" }) });
    });
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft yes");
    expect(api.append).not.toHaveBeenCalled();
  });

  it("stops the microphone immediately and sends buffered audio after a connecting socket opens", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: false });
    Socket.initialReadyState = Socket.CONNECTING;
    const onPromptDraft = vi.fn();
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="prompt"
          onPromptDraft={onPromptDraft}
        />,
      ),
    );
    await act(async () => recordButton().click());
    const socket = Socket.latest;
    await act(async () => WorkletNodeFixture.latest.emitSamples(new Float32Array(4096).fill(0.25)));
    await act(async () => recordButton().click());
    expect(trackStop).toHaveBeenCalled();
    expect(socket.send).not.toHaveBeenCalled();
    expect(recordButton().disabled).toBe(true);
    await act(async () => {
      socket.readyState = Socket.OPEN;
      socket.onopen?.();
    });
    const sent = socket.send.mock.calls.map(([data]) => data);
    expect(JSON.parse(sent[0] as string).type).toBe("start");
    expect(sent[1]).toBeInstanceOf(Uint8Array);
    expect((sent[1] as Uint8Array).byteLength).toBeGreaterThan(0);
    expect(JSON.parse(sent[2] as string).type).toBe("stop");
    await act(async () => {
      socket.onmessage?.({ data: JSON.stringify({ type: "final", text: "saved" }) });
      socket.onmessage?.({ data: JSON.stringify({ type: "closed" }) });
    });
    expect(onPromptDraft).toHaveBeenCalledExactlyOnceWith("saved", { mode: "append" });
    expect(api.showError).not.toHaveBeenCalled();
  });

  it("waits for the worklet flush before sending stop when the socket opens in the meantime", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    Socket.initialReadyState = Socket.CONNECTING;
    WorkletNodeFixture.autoFlush = false;
    await render("A");
    await act(async () => recordButton().click());
    const socket = Socket.latest;
    const worklet = WorkletNodeFixture.latest;
    await act(async () => worklet.emitSamples(new Float32Array(960).fill(0.25)));
    await act(async () => recordButton().click());
    expect(trackStop).toHaveBeenCalled();
    expect(worklet.port.postMessage).toHaveBeenCalledWith({ type: "flush" });
    await act(async () => {
      socket.readyState = Socket.OPEN;
      socket.onopen?.();
      worklet.emitSamples(new Float32Array(320).fill(0.5));
    });
    const beforeFlush = socket.send.mock.calls.map(([data]) => data);
    expect(JSON.parse(beforeFlush[0] as string).type).toBe("start");
    expect(beforeFlush.slice(1).every((data) => data instanceof Uint8Array)).toBe(true);
    expect(beforeFlush).toHaveLength(3);
    await act(async () => worklet.port.onmessage?.({ data: { type: "flushed" } }));
    const afterFlush = socket.send.mock.calls.map(([data]) => data);
    expect(JSON.parse(afterFlush.at(-1) as string).type).toBe("stop");
    await act(async () => worklet.emitSamples(new Float32Array(320).fill(0.75)));
    expect(socket.send).toHaveBeenCalledTimes(afterFlush.length);
  });

  it("does not send audio or stop before the socket start message, even if readyState is already OPEN", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    Socket.initialReadyState = Socket.CONNECTING;
    await render("A");
    await act(async () => recordButton().click());
    const socket = Socket.latest;
    await act(async () => {
      socket.readyState = Socket.OPEN;
      WorkletNodeFixture.latest.emitSamples(new Float32Array(960).fill(0.25));
    });
    await act(async () => recordButton().click());
    expect(socket.send).not.toHaveBeenCalled();
    await act(async () => socket.onopen?.());
    const sent = socket.send.mock.calls.map(([data]) => data);
    expect(JSON.parse(sent[0] as string).type).toBe("start");
    expect(sent[1]).toBeInstanceOf(Uint8Array);
    expect(JSON.parse(sent[2] as string).type).toBe("stop");
  });

  it("warns and keeps recognized text for review when worklet flushing is unconfirmed", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    WorkletNodeFixture.autoFlush = false;
    await render("A");
    await act(async () => recordButton().click());
    const socket = Socket.latest;
    await act(async () => recordButton().click());
    expect(trackStop).toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(api.showError).toHaveBeenCalledWith(
      "Audio processing did not confirm the last buffered samples; the result may be incomplete.",
    );
    expect(JSON.parse(socket.send.mock.calls.at(-1)?.[0] as string).type).toBe("stop");
    await act(async () => {
      socket.onmessage?.({ data: JSON.stringify({ type: "ready" }) });
      socket.onmessage?.({ data: JSON.stringify({ type: "final", text: "captured" }) });
    });
    expect(host.querySelector("[data-voice-recognition-error]")?.textContent).toContain(
      "Audio processing did not confirm the last buffered samples",
    );
    await act(async () => {
      socket.onmessage?.({ data: JSON.stringify({ type: "closed" }) });
    });
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft captured");
    expect(api.append).not.toHaveBeenCalled();
  });

  it("preserves recognized text and stops capture when the audio processor fails", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "captured" }) });
    });
    await act(async () => WorkletNodeFixture.latest.onprocessorerror?.());
    expect(trackStop).toHaveBeenCalled();
    expect(api.showError).toHaveBeenCalledWith(
      "Audio processing stopped; this recording may be incomplete.",
    );
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft captured");
    expect(api.append).not.toHaveBeenCalled();
    expect(recordButton().disabled).toBe(false);
  });

  it("stops and preserves recognized text when the audio context is interrupted", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "captured" }) });
    });
    await act(async () => {
      AudioContextFixture.latest.state = "suspended";
      AudioContextFixture.latest.onstatechange?.();
    });
    expect(trackStop).toHaveBeenCalled();
    expect(api.showError).toHaveBeenCalledWith(
      "Audio processing stopped; this recording may be incomplete.",
    );
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft captured");
    expect(api.append).not.toHaveBeenCalled();
  });

  it("stops capture if the audio context cannot start running", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    AudioContextFixture.initialState = "suspended";
    AudioContextFixture.resumePromise = new Promise(() => {});
    await render("A");
    await act(async () => recordButton().click());
    expect(trackStop).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(trackStop).toHaveBeenCalled();
    expect(api.showError).toHaveBeenCalledWith(
      "Audio processing stopped; this recording may be incomplete.",
    );
    expect(recordButton().disabled).toBe(false);
    expect(api.append).not.toHaveBeenCalled();
  });

  it("releases a stalled worklet startup without starting the microphone", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    AudioContextFixture.modulePromise = new Promise(() => {});
    await render("A");
    await act(async () => recordButton().click());
    expect(recordButton().disabled).toBe(true);
    expect(api.microphone).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(AudioContextFixture.closeSpy).toHaveBeenCalledOnce();
    expect(api.showError).toHaveBeenCalledWith(
      "Audio processing could not be started. Refresh the page and try again.",
    );
    expect(recordButton().disabled).toBe(false);
    expect(api.microphone).not.toHaveBeenCalled();
  });

  it("closes a worklet that finishes loading after the component has unmounted", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    let finish!: () => void;
    AudioContextFixture.modulePromise = new Promise((resolve) => {
      finish = resolve;
    });
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => root.unmount());
    rootMounted = false;
    await act(async () => finish());
    expect(AudioContextFixture.closeSpy).toHaveBeenCalledOnce();
    expect(api.microphone).not.toHaveBeenCalled();
  });

  it("stops a microphone stream that arrives after the component has unmounted", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    const stream = await api.microphone();
    let finish!: () => void;
    api.microphone.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = () => resolve(stream);
        }),
    );
    await render("A");
    await act(async () => recordButton().click());
    expect(finish).toBeTypeOf("function");
    await act(async () => root.unmount());
    rootMounted = false;
    await act(async () => finish());
    expect(trackStop).toHaveBeenCalled();
    expect(AudioContextFixture.closeSpy).toHaveBeenCalledOnce();
  });

  it("explains a missing AudioWorklet and still permits browser speech recognition", async () => {
    let backend = "assistant_service_local_asr";
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, backend),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, backend),
    }));
    vi.stubGlobal("AudioWorkletNode", undefined);
    await render("A");
    expect(recordButton().disabled).toBe(true);
    expect(host.textContent).toContain(
      "This browser does not support the audio processing needed for local or external speech recognition.",
    );
    backend = "browser_asr";
    // Remount with browser ASR to exercise its independent capability check.
    await act(async () => root.unmount());
    root = createRoot(host);
    await render("A");
    expect(recordButton().disabled).toBe(false);
    await act(async () => recordButton().click());
    expect(Recognition.latest).toBeInstanceOf(Recognition);
  });

  it("lets the user explicitly switch an unavailable microphone to the shared system default", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    useVoiceAudioStore
      .getState()
      .update({ inputDeviceId: "missing-mic", outputDeviceId: "speaker" });
    api.microphone.mockRejectedValueOnce(new DOMException("device unavailable", "NotFoundError"));
    await render("A");
    await act(async () => recordButton().click());
    expect(useVoiceAudioStore.getState().preferences.inputDeviceId).toBe("missing-mic");
    const useDefault = host.querySelector<HTMLButtonElement>("[data-voice-use-default-mic]");
    expect(useDefault).not.toBeNull();
    await act(async () => useDefault?.click());
    expect(useVoiceAudioStore.getState().preferences).toEqual({
      inputDeviceId: "",
      outputDeviceId: "speaker",
    });
    expect(api.microphone).toHaveBeenCalledTimes(1);
    await act(async () => recordButton().click());
    expect(api.microphone).toHaveBeenCalledTimes(2);
    expect(api.microphone.mock.calls[1][0].audio).not.toHaveProperty("deviceId");
  });

  it("does not replace a different microphone selected after the failure", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    useVoiceAudioStore.getState().update({ inputDeviceId: "missing-mic" });
    api.microphone.mockRejectedValueOnce(new DOMException("device unavailable", "NotFoundError"));
    await render("A");
    await act(async () => recordButton().click());
    expect(host.querySelector("[data-voice-use-default-mic]")).not.toBeNull();
    await act(async () => useVoiceAudioStore.getState().update({ inputDeviceId: "new-mic" }));
    expect(host.querySelector("[data-voice-use-default-mic]")).toBeNull();
    expect(useVoiceAudioStore.getState().preferences.inputDeviceId).toBe("new-mic");
    await act(async () => recordButton().click());
    expect(api.microphone.mock.calls[1][0].audio.deviceId).toEqual({ exact: "new-mic" });
  });

  it("ends a stopped recording with an explicit error if its socket never connects", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    Socket.initialReadyState = Socket.CONNECTING;
    await render("A");
    await act(async () => recordButton().click());
    const socket = Socket.latest;
    await act(async () => recordButton().click());
    await act(async () => vi.advanceTimersByTime(10000));
    expect(trackStop).toHaveBeenCalled();
    expect(api.showError).toHaveBeenCalledWith(
      "Could not complete the speech-recognition connection; this recording may not have been fully transcribed.",
    );
    expect(recordButton().disabled).toBe(false);
    await act(async () => {
      socket.readyState = Socket.OPEN;
      socket.onopen?.();
    });
    expect(socket.send).not.toHaveBeenCalled();
  });

  it("stops on microphone loss and keeps recognized text without automatically sending it", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "captured" }) });
    });
    await act(async () => capturedTrack.onended?.(new Event("ended")));
    expect(recordButton().getAttribute("aria-pressed")).toBe("false");
    expect(recordButton().disabled).toBe(true);
    const last = Socket.latest.send.mock.calls.at(-1)?.[0];
    expect(JSON.parse(last as string).type).toBe("stop");
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft captured");
    expect(api.append).not.toHaveBeenCalled();
  });

  it("restores the dictated text once when prompt refinement returns no change", async () => {
    let finished = false;
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string, opts?: { promptRequestId?: string }) => ({
      ok: true,
      result: {
        ...state(group, "assistant_service_local_asr"),
        prompt_draft:
          finished && opts?.promptRequestId
            ? { request_id: opts.promptRequestId, status: "no_change", draft_text: "" }
            : undefined,
      },
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "my idea" }) });
    });
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(api.append).toHaveBeenCalledTimes(1);
    finished = true;
    await act(async () => vi.advanceTimersByTimeAsync(4000));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft my idea");
    await act(async () => vi.advanceTimersByTimeAsync(4000));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft my idea");
  });

  it("keeps the original dictated text when the user cancels a pending refinement", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string, opts?: { promptRequestId?: string }) => ({
      ok: true,
      result: {
        ...state(group, "assistant_service_local_asr"),
        secretary_tasks: opts?.promptRequestId
          ? [{ task_id: "task-1", phase: "running", target: { request_id: opts.promptRequestId } }]
          : [],
      },
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "my idea" }) });
    });
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    const cancel = host.querySelector<HTMLButtonElement>("[data-voice-prompt-cancel]");
    expect(cancel).not.toBeNull();
    await act(async () => cancel?.click());
    expect(api.cancel).toHaveBeenCalledWith("A", "task-1");
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft my idea");
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft my idea");
  });

  it("preserves an unconfirmed request after its timeout without replaying it", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    let resolveRequest!: (value: unknown) => void;
    api.append.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveRequest = resolve;
        }),
    );
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "my request" }) });
    });
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(recordButton().disabled).toBe(true);
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft my request");
    expect(recordButton().disabled).toBe(false);
    expect(api.append).toHaveBeenCalledTimes(1);
    await act(async () => resolveRequest({ ok: true, result: { request_id: "late" } }));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft my request");
    expect(api.append).toHaveBeenCalledTimes(1);
  });

  it("keeps an unfinished browser hypothesis for review instead of sending it", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: false, length: 1, 0: { transcript: "unfinished thought" } },
        },
      });
    });
    await act(async () => recordButton().click());
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft unfinished thought");
    expect(api.append).not.toHaveBeenCalled();
    expect(api.showNotice).not.toHaveBeenCalledWith({ message: "No text was recognized." });
  });

  it("preserves browser text without sending when stop never receives an end event", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "browser_asr"),
    }));
    Recognition.autoEndOnStop = false;
    await render("A");
    await act(async () => recordButton().click());
    await act(async () => {
      Recognition.latest.onresult?.({
        resultIndex: 0,
        results: {
          length: 1,
          0: { isFinal: true, length: 1, 0: { transcript: "partial result" } },
        },
      });
    });
    await act(async () => recordButton().click());
    await act(async () => vi.advanceTimersByTimeAsync(2000));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft partial result");
    expect(api.append).not.toHaveBeenCalled();
    expect(api.showError).toHaveBeenCalledWith(
      "The browser did not confirm that speech recognition finished; the result may be incomplete.",
    );
  });

  it("keeps a later recording editable while an earlier prompt request is pending", async () => {
    let draftReady = false;
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string, opts?: { promptRequestId?: string }) => ({
      ok: true,
      result: {
        ...state(group, "assistant_service_local_asr"),
        prompt_draft:
          draftReady && opts?.promptRequestId
            ? {
                request_id: opts.promptRequestId,
                status: "pending",
                draft_text: "polished first idea",
              }
            : undefined,
      },
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () =>
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "first idea" }) }),
    );
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(api.append).toHaveBeenCalledTimes(1);
    await act(async () => recordButton().click());
    await act(async () =>
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "another idea" }) }),
    );
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    expect(api.append).toHaveBeenCalledTimes(1);
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft another idea");
    draftReady = true;
    await act(async () => vi.advanceTimersByTimeAsync(2000));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft another idea");
    expect(host.querySelector("[data-voice-prompt-review]")).not.toBeNull();
  });

  it("requires review if a polished draft arrives after its original speech was restored", async () => {
    let draftReady = false;
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string, opts?: { promptRequestId?: string }) => ({
      ok: true,
      result: {
        ...state(group, "assistant_service_local_asr"),
        prompt_draft:
          draftReady && opts?.promptRequestId
            ? { request_id: opts.promptRequestId, status: "pending", draft_text: "polished idea" }
            : undefined,
      },
    }));
    await render("A");
    await act(async () => recordButton().click());
    await act(async () =>
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "my idea" }) }),
    );
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(useComposerStore.getState().composerText).toBe("Unsent A draft my idea");
    await act(async () => useComposerStore.getState().setComposerText("User edited the draft"));
    draftReady = true;
    await act(async () => vi.advanceTimersByTimeAsync(2000));
    expect(useComposerStore.getState().composerText).toBe("User edited the draft");
    expect(host.querySelector("[data-voice-prompt-review]")).not.toBeNull();
    expect(api.append).toHaveBeenCalledTimes(1);
  });

  it.each(["", "recognized words"])(
    "lets the user stop waiting for a non-document final with text %j",
    async (recognized) => {
      api.workspace.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, "assistant_service_local_asr"),
      }));
      api.status.mockImplementation(async (group: string) => ({
        ok: true,
        result: state(group, "assistant_service_local_asr"),
      }));
      await render("A");
      await act(async () => recordButton().click());
      const socket = Socket.latest;
      if (recognized) {
        await act(async () =>
          socket.onmessage?.({ data: JSON.stringify({ type: "final", text: recognized }) }),
        );
      }
      await act(async () => recordButton().click());
      await act(async () => vi.advanceTimersByTimeAsync(19_999));
      expect(host.querySelector("[data-voice-stop-waiting]")).toBeNull();
      await act(async () => vi.advanceTimersByTimeAsync(1));
      const button = host.querySelector<HTMLButtonElement>("[data-voice-stop-waiting]");
      expect(button).not.toBeNull();
      expect(host.textContent).toContain("voiceSecretaryStopWaitingHint");
      await act(async () => button!.click());
      expect(useComposerStore.getState().composerText).toBe(
        recognized ? `Unsent A draft ${recognized}` : "Unsent A draft",
      );
      expect(recordButton().disabled).toBe(false);
      await act(async () => socket.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
      expect(api.append).not.toHaveBeenCalled();
    },
  );

  it("does not offer to discard a document recording while waiting for its final result", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="document"
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () => recordButton().click());
    await act(async () => vi.advanceTimersByTimeAsync(20_000));
    expect(recordButton().disabled).toBe(true);
    expect(host.querySelector("[data-voice-stop-waiting]")).toBeNull();
  });

  it("does not write a late failed request into a different page after unmount", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    let rejectRequest!: (value: unknown) => void;
    api.append.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          rejectRequest = resolve;
        }),
    );
    await render("A");
    await act(async () => recordButton().click());
    await act(async () =>
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "final", text: "private A" }) }),
    );
    await act(async () => recordButton().click());
    await act(async () => Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) }));
    await act(async () => root.unmount());
    rootMounted = false;
    await act(async () => {
      useComposerStore.getState().switchGroup("A", "B");
      useComposerStore.getState().setComposerText("Private B draft");
    });
    await act(async () => rejectRequest({ ok: false, error: { message: "network" } }));
    expect(useComposerStore.getState().composerText).toBe("Private B draft");
  });

  it("joins fixed ASR segment boundaries while preserving a newline inside a segment", async () => {
    api.workspace.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    api.status.mockImplementation(async (group: string) => ({
      ok: true,
      result: state(group, "assistant_service_local_asr"),
    }));
    useUIStore.getState().setChatVoicePreferences("A", { voicePromptAutoRefine: false });
    const onPromptDraft = vi.fn();
    await act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId="A"
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="prompt"
          onPromptDraft={onPromptDraft}
        />,
      ),
    );
    await act(async () => recordButton().click());
    await act(async () => recordButton().click());
    await act(async () => {
      Socket.latest.onmessage?.({
        data: JSON.stringify({
          type: "final_asr_text",
          ok: true,
          text: "你好，\nworld\nAlpha\nBeta",
          segments: [
            { ok: true, text: "你好，" },
            { ok: true, text: "world" },
            { ok: true, text: "Alpha\nBeta" },
          ],
        }),
      });
      Socket.latest.onmessage?.({ data: JSON.stringify({ type: "closed" }) });
    });
    expect(onPromptDraft).toHaveBeenCalledExactlyOnceWith("你好，world Alpha\nBeta", {
      mode: "append",
    });
  });
});
