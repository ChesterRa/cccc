import {
  startCodexVoiceCall,
  stopCodexVoiceCall,
  prepareCodexVoiceNotificationOutput,
  type CodexVoiceAnalystInfo,
  type CodexVoiceCallInfo,
} from "../../services/api";
import type { CodexVoicePreferences } from "./codexVoicePreferences";
import type { CodexVoiceSessionCallbacks } from "./codexVoiceTypes";
import { CodexVoiceEventSocket, type CodexVoiceServerMessage } from "./codexVoiceEventSocket";
import {
  applyOutputDevice,
  captureMicrophone,
  createClientSessionId,
  waitForDataChannelOpen,
  waitForIceGathering,
} from "./codexVoiceMedia";
import {
  boundedDelta,
  boundedText,
  failure,
  failureCode,
  normalizedErrorCode,
  RealtimeTranscriptAccumulator,
  realtimeTranscriptUpdate,
  realtimeProviderError,
  shouldForwardProviderEvent,
} from "./codexVoiceProtocol";
import { CodexVoiceProviderChannel } from "./codexVoiceProviderChannel";
import { CodexVoicePeerMonitor } from "./codexVoicePeerMonitor";

// Leave room for JSON escaping of 256-byte managed result IDs inside the
// server's 128 KiB frame limit, independently of its 1,024-ID observation limit.
const UNSENT_OUTPUT_REPORT_BATCH_SIZE = 64;

export {
  eventStreamCloseCode,
  RealtimeTranscriptAccumulator,
  realtimeTranscriptUpdate,
  shouldForwardProviderEvent,
} from "./codexVoiceProtocol";
export type { CodexVoicePhase, CodexVoiceSessionCallbacks } from "./codexVoiceTypes";

export class CodexVoiceBrowserSession {
  private readonly audio: HTMLAudioElement;
  private readonly callbacks: CodexVoiceSessionCallbacks;
  private readonly preferences: CodexVoicePreferences;
  private readonly clientSessionId = createClientSessionId();
  private stream: MediaStream | null = null;
  private peer: RTCPeerConnection | null = null;
  private readonly providerChannel: CodexVoiceProviderChannel;
  private readonly peerMonitor = new CodexVoicePeerMonitor();
  private eventSocket: CodexVoiceEventSocket | null = null;
  private call: CodexVoiceCallInfo | null = null;
  private analyst: CodexVoiceAnalystInfo | null = null;
  private requestAbort: AbortController | null = null;
  private stopping = false;
  private readonly transcripts = new RealtimeTranscriptAccumulator();

  constructor(args: {
    audio: HTMLAudioElement;
    preferences: CodexVoicePreferences;
    callbacks: CodexVoiceSessionCallbacks;
  }) {
    this.audio = args.audio;
    this.preferences = args.preferences;
    this.callbacks = args.callbacks;
    this.providerChannel = new CodexVoiceProviderChannel(
      (data) => void this.handleProviderMessage(data),
      (code) => {
        console.warn("Codex Voice output failed", { code, ...this.providerChannel.receipt() });
        void this.fail(code);
      },
      () => this.stopping,
      () => {
        // Bounded transport metadata only; no source text or credentials. Keep
        // failure evidence in the browser even without a host tracing subscriber.
        console.warn("Codex Voice output unconfirmed", this.providerChannel.receipt());
        this.sendServerMessage({ type: "provider_receipt", ...this.providerChannel.receipt() });
        this.callbacks.onError("provider_context_unconfirmed");
      },
      (resultId) => {
        if (!this.sendServerMessage({ type: "notification_output_submitted", result_id: resultId }))
          void this.fail("event_stream_disconnected");
      },
      async (resultId, signal) => {
        if (!this.call || this.stopping) throw failure("event_stream_disconnected");
        const response = await prepareCodexVoiceNotificationOutput(
          this.call.generation,
          resultId,
          signal,
        );
        if (!response.ok) throw failure(response.error.code);
        return response.result.message;
      },
      () => {
        this.callbacks.onOutputStatus?.(this.providerChannel.outputStatus());
        this.sendServerMessage({ type: "provider_receipt", ...this.providerChannel.receipt() });
      },
    );
  }

  async start(): Promise<void> {
    this.callbacks.onPhase("preparing");
    try {
      if (!navigator.mediaDevices?.getUserMedia) {
        throw failure("microphone_unsupported");
      }
      if (typeof RTCPeerConnection === "undefined") {
        throw failure("webrtc_unsupported");
      }
      await applyOutputDevice(this.audio, this.preferences.outputDeviceId);
      if (this.stopping) return;
      const stream = await captureMicrophone(this.preferences.inputDeviceId);
      if (this.stopping) {
        for (const track of stream.getTracks()) track.stop();
        return;
      }
      this.stream = stream;

      const peer = new RTCPeerConnection();
      this.peer = peer;
      this.peerMonitor.bind({
        peer,
        audio: this.audio,
        resumeAudio: () => this.resumeAudio(),
        onPhase: (phase) => this.callbacks.onPhase(phase),
        onFailure: (code) => void this.fail(code),
        isStopping: () => this.stopping,
      });
      for (const track of this.stream.getAudioTracks()) peer.addTrack(track, this.stream);
      const dataChannel = peer.createDataChannel("oai-events");
      this.providerChannel.bind(dataChannel);

      const offer = await peer.createOffer();
      if (this.stopping) return;
      await peer.setLocalDescription(offer);
      if (this.stopping) return;
      await waitForIceGathering(peer);
      if (this.stopping) return;
      const offerSdp = peer.localDescription?.sdp;
      if (!offerSdp?.trim()) throw failure("webrtc_offer_failed");

      this.callbacks.onPhase("connecting");
      const requestAbort = new AbortController();
      this.requestAbort = requestAbort;
      const response = await startCodexVoiceCall({
        clientSessionId: this.clientSessionId,
        offerSdp,
        voice: this.preferences.voice,
        signal: requestAbort.signal,
      });
      this.requestAbort = null;
      if (!response.ok) throw failure(response.error.code);
      if (this.stopping) {
        // Abort can lose to a completed HTTP response. Release only this
        // generation; it must not occupy the lease or stop a newer call.
        const stopped = await stopCodexVoiceCall(response.result.call.generation);
        if (!stopped.ok) throw failure(stopped.error.code);
        return;
      }
      this.call = response.result.call;
      this.analyst = response.result.analyst;
      this.callbacks.onCall(this.call);
      this.callbacks.onAnalyst(this.analyst);

      const eventSocket = new CodexVoiceEventSocket(
        this.call,
        (message) => this.handleServerMessage(message),
        (code) => void this.fail(code),
        () => this.stopping,
      );
      this.eventSocket = eventSocket;
      await eventSocket.connect();
      if (this.stopping) return;
      await peer.setRemoteDescription({ type: "answer", sdp: response.result.answer_sdp });
      if (this.stopping) return;
      await waitForDataChannelOpen(peer, () => this.providerChannel.readyState());
      if (this.stopping) return;
      this.callbacks.onPhase("listening");
    } catch (error) {
      // A cancelled startup is finished. Rejecting it later would let its
      // controller failure handler interfere with a newly started call.
      if (this.stopping) return;
      this.callbacks.onError(failureCode(error));
      this.callbacks.onPhase("failed");
      await this.stop({ notifyPhase: false });
      throw error;
    }
  }

  cancelInvestigation(): boolean {
    return this.sendServerMessage({ type: "cancel_current" });
  }

  setMicrophoneMuted(muted: boolean): boolean {
    const tracks = this.stream?.getAudioTracks() || [];
    if (tracks.length === 0) return false;
    for (const track of tracks) track.enabled = !muted;
    return true;
  }

  async resumeAudio(): Promise<boolean> {
    if (this.stopping) return false;
    try {
      await this.audio.play();
      if (this.stopping) return false;
      this.callbacks.onPlaybackBlocked(false);
      return true;
    } catch {
      if (!this.stopping) this.callbacks.onPlaybackBlocked(true);
      return false;
    }
  }

  async stop(options: { notifyPhase?: boolean } = {}): Promise<void> {
    if (this.stopping) return;
    this.stopping = true;
    if (options.notifyPhase !== false) this.callbacks.onPhase("stopping");
    this.requestAbort?.abort();
    this.requestAbort = null;
    this.peerMonitor.close();

    const call = this.call;
    const unsent = this.providerChannel.unsentResultIds();
    for (let i = 0; i < unsent.length; i += UNSENT_OUTPUT_REPORT_BATCH_SIZE) {
      this.sendServerMessage({
        type: "notification_output_not_submitted",
        result_ids: unsent.slice(i, i + UNSENT_OUTPUT_REPORT_BATCH_SIZE),
      });
    }
    this.sendServerMessage({ type: "provider_receipt", ...this.providerChannel.receipt() });
    this.sendServerMessage({ type: "stop" });
    this.eventSocket?.close();
    this.eventSocket = null;
    this.providerChannel.close();
    this.peer?.close();
    this.peer = null;
    for (const track of this.stream?.getTracks() || []) track.stop();
    this.stream = null;
    this.audio.pause();
    this.audio.srcObject = null;
    this.callbacks.onPlaybackBlocked(false);
    this.call = null;
    this.callbacks.onCall(null);

    if (call) {
      const response = await stopCodexVoiceCall(call.generation);
      if (!response.ok) this.callbacks.onError(response.error.code);
    }
    if (options.notifyPhase !== false) this.callbacks.onPhase("idle");
  }

  private handleServerMessage(message: CodexVoiceServerMessage): void {
    if (this.stopping) return;
    switch (message.type) {
      case "notification_status":
        this.callbacks.onNotificationPaused?.(message.paused === true);
        break;
      case "provider_command":
        this.providerChannel.send(
          message.message,
          typeof message.result_id === "string" ? message.result_id : undefined,
        );
        break;
      case "analyst_working":
        this.callbacks.onAnalystProgress("");
        this.callbacks.onAnalystResult("");
        if (this.analyst) {
          this.analyst = { ...this.analyst, tui_ready: true, phase: "working", warning: "" };
          this.callbacks.onAnalyst(this.analyst);
        }
        break;
      case "analyst_progress":
        this.callbacks.onAnalystProgress(boundedDelta(message.text));
        break;
      case "analyst_result":
        {
          const result = boundedText(message.text) || "";
          this.callbacks.onAnalystResult(result);
          if (this.analyst) {
            this.analyst = { ...this.analyst, phase: "ready", last_result: result };
            this.callbacks.onAnalyst(this.analyst);
          }
        }
        break;
      case "analyst_terminal":
        if (this.analyst) {
          this.analyst = { ...this.analyst, phase: "ready" };
          this.callbacks.onAnalyst(this.analyst);
        }
        break;
      case "analyst_cancelling":
        break;
      case "error":
        this.callbacks.onError(normalizedErrorCode(message.code) || "unknown");
        break;
      case "heartbeat":
        break;
      default:
        break;
    }
  }

  private async handleProviderMessage(data: unknown): Promise<void> {
    let text: string;
    if (typeof data === "string") text = data;
    else if (data instanceof Blob) text = await data.text();
    else if (data instanceof ArrayBuffer) text = new TextDecoder().decode(data);
    else return;
    // Blob decoding can finish after orderly stop. A late event must not
    // overwrite the stopped/failed call's status or the next call's UI.
    if (this.stopping) return;
    let event: unknown;
    try {
      event = JSON.parse(text);
    } catch {
      return;
    }
    if (this.providerChannel.observe(event)) {
      this.sendServerMessage({ type: "provider_receipt", ...this.providerChannel.receipt() });
    }
    this.transcripts.observeTurn(event);
    const transcript = realtimeTranscriptUpdate(event);
    if (transcript) {
      const accumulatedText = this.transcripts.apply(transcript);
      this.callbacks.onConversation?.(this.transcripts.history());
      if (transcript.role === "user") {
        this.callbacks.onUserTranscript(accumulatedText);
        if (transcript.final) this.callbacks.onPhase("responding");
      } else {
        this.callbacks.onAssistantTranscript(accumulatedText);
        this.callbacks.onPhase(transcript.final ? "listening" : "speaking");
      }
    }
    if (shouldForwardProviderEvent(event)) {
      this.sendServerMessage({ type: "provider_event", event });
    }
    const providerError = realtimeProviderError(event);
    if (providerError && !this.stopping) {
      const { message, ...diagnostic } = providerError;
      console.warn("Codex Realtime Voice provider error", { ...diagnostic, message });
      this.sendServerMessage({ type: "provider_error", error: diagnostic });
      await this.fail("provider_error", providerError.code || undefined);
    }
  }

  private sendServerMessage(message: unknown): boolean {
    return this.eventSocket?.send(message) || false;
  }

  private async fail(code: string, providerCode?: string): Promise<void> {
    if (this.stopping) return;
    this.callbacks.onError(normalizedErrorCode(code) || "unknown", providerCode);
    this.callbacks.onPhase("failed");
    await this.stop({ notifyPhase: false });
  }
}
