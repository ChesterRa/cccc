import { afterEach, describe, expect, it, vi } from "vitest";
import { createInstance } from "i18next";
import en from "../../i18n/locales/en/modals.json";
import ja from "../../i18n/locales/ja/modals.json";
import zh from "../../i18n/locales/zh/modals.json";
import { codexVoiceErrorText } from "./codexVoiceControllerText";
import { realtimeProviderError } from "./codexVoiceProtocol";
import { CodexVoiceBrowserSession } from "./codexVoiceSession";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Realtime provider error diagnostics", () => {
  it("extracts nested or flat error identifiers without copying arbitrary payloads", () => {
    expect(realtimeProviderError({ type: "turn.done" })).toBeNull();
    expect(
      realtimeProviderError({
        type: "error",
        event_id: "server-event",
        error: {
          code: "rate_limit_exceeded",
          type: "invalid_request_error",
          event_id: "client-event",
          param: "content[0].text",
          message: "provider explanation",
          authorization: "must not be copied",
        },
      }),
    ).toEqual({
      code: "rate_limit_exceeded",
      type: "invalid_request_error",
      event_id: "client-event",
      param: "content[0].text",
      message: "provider explanation",
    });
    expect(realtimeProviderError({ type: "error", code: "HTTP:429", message: "limited" })).toEqual({
      code: "HTTP:429",
      type: "",
      event_id: "",
      param: "",
      message: "limited",
    });
  });

  it("bounds explanations and rejects unsafe or oversized diagnostic identifiers", () => {
    expect(
      realtimeProviderError({
        type: "error",
        event_id: "server-event",
        error: {
          code: "bad\ninjected",
          type: "x".repeat(129),
          param: "https://example.com/?token=private-value",
          message: "x".repeat(5_000),
        },
      }),
    ).toEqual({
      code: "",
      type: "",
      event_id: "server-event",
      param: "",
      message: "x".repeat(2_048),
    });
  });

  it.each(["rate_limit_exceeded", "invalid_event", "HTTP:429", undefined])(
    "reports provider %s before stopping without turning it into an Analyst error",
    async (code) => {
      const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);
      const onError = vi.fn();
      const onPhase = vi.fn();
      const onAnalyst = vi.fn();
      const session = new CodexVoiceBrowserSession({
        audio: { pause: vi.fn(), srcObject: null } as unknown as HTMLAudioElement,
        preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
        callbacks: {
          onError,
          onPhase,
          onAnalyst,
          onCall: vi.fn(),
          onUserTranscript: vi.fn(),
          onAssistantTranscript: vi.fn(),
          onAnalystProgress: vi.fn(),
          onAnalystResult: vi.fn(),
          onPlaybackBlocked: vi.fn(),
        },
      });
      // Inject only transport doubles; exercise the real message handler and stop path.
      const transport = session as unknown as {
        handleProviderMessage(data: string): Promise<void>;
        eventSocket: { send: (value: unknown) => boolean; close: () => void };
        stream: MediaStream;
      };
      const messages: unknown[] = [];
      const close = vi.fn();
      const stopTrack = vi.fn();
      transport.eventSocket = {
        send(value) {
          messages.push(value);
          return true;
        },
        close,
      };
      transport.stream = { getTracks: () => [{ stop: stopTrack }] } as unknown as MediaStream;
      const event = JSON.stringify({
        type: "error",
        error: { code, message: "provider explanation quoting private input" },
      });
      await transport.handleProviderMessage(event);
      await transport.handleProviderMessage(event);

      expect(onError).toHaveBeenCalledExactlyOnceWith("provider_error", code);
      expect(onPhase).toHaveBeenCalledExactlyOnceWith("failed");
      expect(onAnalyst).not.toHaveBeenCalled();
      expect(messages[0]).toEqual({
        type: "provider_error",
        error: { code: code || "", type: "", event_id: "", param: "" },
      });
      expect(messages.at(-1)).toEqual({ type: "stop" });
      expect(JSON.stringify(messages)).not.toContain("private input");
      expect(warning).toHaveBeenCalledTimes(1);
      expect(warning).toHaveBeenCalledWith(
        "Codex Realtime Voice provider error",
        expect.objectContaining({ message: "provider explanation quoting private input" }),
      );
      expect(close).toHaveBeenCalledTimes(1);
      expect(stopTrack).toHaveBeenCalledTimes(1);
    },
  );

  it.each(["en", "zh", "ja"])("preserves provider codes in the %s UI", async (language) => {
    const i18n = createInstance();
    await i18n.init({
      lng: language,
      resources: { en: { translation: en }, zh: { translation: zh }, ja: { translation: ja } },
    });
    const translate = (key: string, options?: Record<string, unknown>) =>
      String(i18n.t(key, options));
    const visible = codexVoiceErrorText(translate, "provider_error", "rate_limit_exceeded");
    expect(visible).toContain("rate_limit_exceeded");
    expect(visible).not.toContain("{{");
    expect(visible).not.toEqual(translate("codexVoiceErrors.unknown"));
    expect(codexVoiceErrorText(translate, "provider_error")).toEqual(
      translate("codexVoiceErrors.provider_error"),
    );
    expect(codexVoiceErrorText(translate, "analyst_disconnected")).toEqual(
      translate("codexVoiceErrors.analyst_disconnected"),
    );
  });
});
