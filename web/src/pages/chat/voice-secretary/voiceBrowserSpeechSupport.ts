import type {
  BrowserAudioSupportIssue,
  BrowserMicrophoneSupportIssue,
  BrowserSpeechRecognition,
  BrowserSpeechRecognitionConstructor,
  BrowserSpeechSupportIssue,
} from "./voiceBrowserSpeechTypes";
import { isConnectFramePath } from "../../../features/connect/protocol";

/** Browser recognition services can reject a bare language even when it is valid BCP 47. */
export function resolveBrowserSpeechLanguage(language: string, systemLanguage: string): string {
  const configured = String(language || "").trim();
  const requested =
    !configured || configured === "auto" || configured === "mixed"
      ? String(systemLanguage || "").trim() || "en-US"
      : configured;
  try {
    const locale = new Intl.Locale(requested);
    const region = locale.region || locale.maximize().region;
    return region && !locale.region
      ? new Intl.Locale(locale.baseName, { region }).baseName
      : locale.baseName;
  } catch {
    // Keep an invalid/unsupported preference visible to the recognition service;
    // selecting another language would silently change the user's recording.
    return requested;
  }
}

export function getBrowserSpeechRecognitionConstructor(): BrowserSpeechRecognitionConstructor | null {
  if (typeof window === "undefined") return null;
  const speechWindow = window as typeof window & {
    SpeechRecognition?: BrowserSpeechRecognitionConstructor;
    webkitSpeechRecognition?: BrowserSpeechRecognitionConstructor;
  };
  return speechWindow.SpeechRecognition || speechWindow.webkitSpeechRecognition || null;
}

export function getBrowserSpeechSupportIssue(): BrowserSpeechSupportIssue {
  return getBrowserSpeechRecognitionConstructor() ? "" : "unsupported";
}

export function getBrowserMicrophoneSupportIssue(): BrowserMicrophoneSupportIssue {
  if (typeof window !== "undefined" && isConnectFramePath(window.location.pathname))
    return "embedded_workspace";
  if (typeof window !== "undefined" && window.isSecureContext === false) return "secure_context";
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia)
    return "get_user_media";
  return "";
}

export function getBrowserAudioSupportIssue(): BrowserAudioSupportIssue {
  return getBrowserMicrophoneSupportIssue();
}

export function mediaRecorderSupported(): boolean {
  return !getBrowserAudioSupportIssue();
}

export function stopMediaStream(stream: MediaStream | null): void {
  if (!stream) return;
  try {
    stream.getTracks().forEach((track) => track.stop());
  } catch {
    // Ignore browser cleanup failure.
  }
}

export function mediaStreamHasLiveAudio(stream: MediaStream | null): boolean {
  if (!stream) return false;
  try {
    return stream.getAudioTracks().some((track) => track.readyState === "live");
  } catch {
    return false;
  }
}

export function abortBrowserSpeechRecognition(recognition: BrowserSpeechRecognition | null): void {
  if (!recognition) return;
  recognition.onend = null;
  recognition.onerror = null;
  recognition.onresult = null;
  recognition.onspeechstart = null;
  recognition.onspeechend = null;
  try {
    recognition.abort();
  } catch {
    // Ignore browser cleanup failure.
  }
}
